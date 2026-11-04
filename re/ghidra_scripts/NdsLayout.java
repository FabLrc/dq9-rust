// Builds the ARM9 memory map of an NDS program from `dq9re extract`'s layout.json.
//
// Expects the program to be `arm9.bin` imported raw at its load address (see
// scripts/ghidra-import.ps1). Then:
//   * trims the autoload payload off the end of the static module,
//   * maps every autoload block (ITCM, DTCM, ...) at its run-time address, plus its .bss,
//   * maps the static .bss,
//   * maps each overlay into its own overlay address space (ov_NNN), .bss zero-filled,
//   * marks the entry point and seeds functions from each overlay's static-initializer table.
// Run as a headless -preScript so auto-analysis sees the final layout.
//@category NDS
//@runtime Java

import java.io.File;
import java.math.BigInteger;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.file.Files;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSpace;
import ghidra.program.model.lang.Register;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.SourceType;

public class NdsLayout extends GhidraScript {

    private Memory mem;
    private AddressSpace ram;

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        File layoutFile = args.length > 0 ? new File(args[0]) : askFile("dq9re layout.json", "Load");
        File dir = layoutFile.getParentFile();
        JsonObject layout = JsonParser.parseString(Files.readString(layoutFile.toPath())).getAsJsonObject();

        mem = currentProgram.getMemory();
        ram = currentProgram.getAddressFactory().getDefaultAddressSpace();

        JsonObject arm9 = layout.getAsJsonObject("arm9");
        byte[] arm9Bytes = Files.readAllBytes(new File(dir, arm9.get("file").getAsString()).toPath());
        long loadAddr = arm9.get("load_addr").getAsLong();
        long staticEnd = arm9.get("static_end").getAsLong();

        // 1. The raw import contains the autoload payload after the static module: drop it.
        MemoryBlock main = mem.getBlock(ram.getAddress(loadAddr));
        if (main == null) {
            throw new IllegalStateException("arm9.bin is not mapped at " + Long.toHexString(loadAddr));
        }
        main.setName("arm9");
        main.setPermissions(true, true, true);
        if (main.getEnd().getOffset() >= staticEnd) {
            mem.split(main, ram.getAddress(staticEnd));
            mem.removeBlock(mem.getBlock(ram.getAddress(staticEnd)), monitor);
        }

        // 2. Autoload blocks at their run-time addresses.
        for (JsonElement e : layout.getAsJsonArray("autoloads")) {
            JsonObject a = e.getAsJsonObject();
            long addr = a.get("ram_addr").getAsLong();
            int size = a.get("size").getAsInt();
            int bss = a.get("bss_size").getAsInt();
            int offset = a.get("file_offset").getAsInt();
            String name = a.get("name").getAsString();
            byte[] data = new byte[size];
            System.arraycopy(arm9Bytes, offset, data, 0, size);
            createBlock(name, addr, data);
            if (bss > 0) {
                createBss(name + "_bss", addr + size, bss);
            }
        }

        // 3. Static .bss of the resident module.
        long bssStart = arm9.get("bss_start").getAsLong();
        long bssEnd = arm9.get("bss_end").getAsLong();
        if (bssEnd > bssStart) {
            createBss("arm9_bss", bssStart, bssEnd - bssStart);
        }

        // 4. Overlays, each in its own overlay space so overlapping RAM ranges coexist.
        for (JsonElement e : layout.getAsJsonArray("overlays")) {
            JsonObject o = e.getAsJsonObject();
            int id = o.get("id").getAsInt();
            long addr = o.get("ram_addr").getAsLong();
            int bss = o.get("bss_size").getAsInt();
            byte[] code = Files.readAllBytes(new File(dir, o.get("file").getAsString()).toPath());
            byte[] image = new byte[code.length + bss];
            System.arraycopy(code, 0, image, 0, code.length);
            String name = String.format("ov_%03d", id);
            MemoryBlock block = mem.createInitializedBlock(name, ram.getAddress(addr),
                new java.io.ByteArrayInputStream(image), image.length, monitor, true);
            block.setPermissions(true, true, true);
            block.setComment(String.format("overlay %d: %d code bytes + %d bss", id, code.length, bss));
            seedStaticInitializers(block.getStart().getAddressSpace(), code, addr,
                o.get("sinit_start").getAsLong(), o.get("sinit_end").getAsLong());
        }

        // 5. Entry point.
        Address entry = ram.getAddress(arm9.get("entry").getAsLong());
        addEntryPoint(entry);
        createLabel(entry, "arm9_entry", true, SourceType.IMPORTED);
        disassemble(entry);

        println("NdsLayout: mapped " + layout.getAsJsonArray("autoloads").size() + " autoload blocks and "
            + layout.getAsJsonArray("overlays").size() + " overlays");
    }

    private void createBlock(String name, long addr, byte[] data) throws Exception {
        MemoryBlock block = mem.createInitializedBlock(name, ram.getAddress(addr),
            new java.io.ByteArrayInputStream(data), data.length, monitor, false);
        block.setPermissions(true, true, true);
    }

    private void createBss(String name, long addr, long size) throws Exception {
        Address start = ram.getAddress(addr);
        if (mem.intersects(start, start.add(size - 1))) {
            println("NdsLayout: skipping " + name + " (overlaps an existing block)");
            return;
        }
        MemoryBlock block = mem.createUninitializedBlock(name, start, size, false);
        block.setPermissions(true, true, false);
    }

    /** Each overlay's [sinit_start, sinit_end) holds pointers to its static constructors. */
    private void seedStaticInitializers(AddressSpace space, byte[] code, long base, long start, long end) {
        if (start == 0 || end <= start || start < base || end - base > code.length) {
            return;
        }
        ByteBuffer buf = ByteBuffer.wrap(code).order(ByteOrder.LITTLE_ENDIAN);
        Register tmode = currentProgram.getRegister("TMode");
        for (long p = start; p < end; p += 4) {
            long target = buf.getInt((int) (p - base)) & 0xFFFFFFFFL;
            if (target == 0) {
                continue;
            }
            boolean thumb = (target & 1) != 0;
            Address fn = space.getAddress(target & ~1L);
            try {
                if (tmode != null) {
                    currentProgram.getProgramContext().setValue(tmode, fn, fn,
                        thumb ? BigInteger.ONE : BigInteger.ZERO);
                }
                disassemble(fn);
                createFunction(fn, null);
            } catch (Exception ex) {
                println("NdsLayout: sinit " + fn + ": " + ex.getMessage());
            }
        }
    }
}
