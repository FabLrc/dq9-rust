// Imports the European function names and bounds from re/functions.txt.
// Run after NdsLayout/analysis, BEFORE ApplySymbols.java (local names win).
// Does not disassemble bytes or guess ARM/Thumb mode: the map contains neither mode.
// Zero-size entries are names only; overlapping ranges stop at the next entry.
// Args: [path to functions.txt]
//@category NDS
//@runtime Java

import java.io.File;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.Iterator;
import java.util.List;
import java.util.Map;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.CodeUnit;
import ghidra.program.model.listing.Function;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.SourceType;

public class ApplyFunctions extends GhidraScript {
    private static class Entry {
        final Address start;
        final Address end;
        final String name;
        final int line;
        AddressSet body;

        Entry(Address start, Address end, String name, int line) {
            this.start = start;
            this.end = end;
            this.name = name;
            this.line = line;
        }
    }

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        File file = args.length > 0 ? new File(args[0]) : askFile("functions.txt", "Apply");
        // Validate the entire map before modifying the program.
        List<Entry> entries = readEntries(file);
        entries.sort(Comparator.comparing(e -> e.start));
        Map<Address, Entry> byStart = new HashMap<>();
        AddressSet imported = new AddressSet();
        int clipped = 0;
        for (int i = 0; i < entries.size(); i++) {
            Entry e = entries.get(i);
            if (byStart.put(e.start, e) != null) {
                throw new IllegalArgumentException(file + ":" + e.line + ": duplicate entry " + e.start);
            }
            if (e.end == null) {
                continue;
            }
            Address end = e.end;
            if (i + 1 < entries.size()) {
                Address next = entries.get(i + 1).start;
                if (next.getAddressSpace().equals(e.start.getAddressSpace()) && end.compareTo(next) >= 0) {
                    end = next.subtract(1);
                    clipped++;
                    println("ApplyFunctions: clipped " + e.start + " end " + e.end + " to " + end);
                }
            }
            e.body = new AddressSet(e.start, end);
            imported.add(e.body);
        }

        // Free the authoritative ranges first: old auto-analysis bodies may span several
        // real entries. Preserve metadata at known starts, and functions outside the map.
        List<Address> affected = new ArrayList<>();
        Iterator<Function> it = currentProgram.getFunctionManager().getFunctionsOverlapping(imported);
        while (it.hasNext()) {
            affected.add(it.next().getEntryPoint());
        }
        int removed = 0;
        for (Address start : affected) {
            monitor.checkCancelled();
            // Ghidra invalidates cached Function objects while bodies are changed.
            Function f = getFunctionAt(start);
            if (f == null) {
                continue;
            }
            Entry e = byStart.get(start);
            if (e != null && e.body != null) {
                f.setBody(new AddressSet(start));
            } else {
                AddressSet remaining = new AddressSet(f.getBody());
                remaining.delete(imported);
                if (remaining.contains(start)) {
                    f.setBody(remaining);
                } else {
                    currentProgram.getFunctionManager().removeFunction(start);
                    removed++;
                }
            }
        }

        int functions = 0;
        int labels = 0;
        for (Entry e : entries) {
            monitor.checkCancelled();
            Function f = getFunctionAt(e.start);
            if (e.body == null) {
                // No invented one-byte function for a symbol with unknown bounds.
                if (f != null) {
                    f.setName(e.name, SourceType.IMPORTED);
                } else {
                    createLabel(e.start, e.name, true, SourceType.IMPORTED);
                }
                labels++;
            } else {
                if (f == null) {
                    // Analysis can mistake an undiscovered entry for data, or decode
                    // an instruction across it. Clear only that conflicting code unit.
                    CodeUnit unit = currentProgram.getListing().getDefinedDataContaining(e.start);
                    if (unit == null) {
                        unit = currentProgram.getListing().getInstructionContaining(e.start);
                        if (unit != null && unit.getMinAddress().equals(e.start)) {
                            unit = null;
                        }
                    }
                    if (unit != null) {
                        currentProgram.getListing().clearCodeUnits(
                            unit.getMinAddress(), unit.getMaxAddress(), false);
                    }
                    f = currentProgram.getFunctionManager().createFunction(
                        e.name, e.start, e.body, SourceType.IMPORTED);
                    if (f == null) {
                        throw new IllegalStateException("cannot create function at " + e.start);
                    }
                } else {
                    f.setBody(e.body);
                    f.setName(e.name, SourceType.IMPORTED);
                }
                functions++;
            }
        }
        println("ApplyFunctions: applied " + functions + " function bounds, " + labels
            + " names without bounds; clipped " + clipped + ", removed " + removed
            + " conflicting inferred functions from " + file);
    }

    private List<Entry> readEntries(File file) throws Exception {
        List<Entry> entries = new ArrayList<>();
        List<String> lines = Files.readAllLines(file.toPath());
        for (int i = 0; i < lines.size(); i++) {
            String line = lines.get(i).strip();
            if (line.isEmpty() || line.startsWith("#")) {
                continue;
            }
            String[] fields = line.split("\\s+");
            if (fields.length != 4 || !fields[1].matches("0x[0-9a-fA-F]+")
                    || !fields[2].matches("game|platform|excluded")) {
                throw new IllegalArgumentException(file + ":" + (i + 1)
                    + ": expected '<address> <hex size> <game|platform|excluded> <name>'");
            }
            String text = fields[0].startsWith("0x") ? fields[0].substring(2) : fields[0];
            Address start = currentProgram.getAddressFactory().getAddress(text);
            long size = Long.parseUnsignedLong(fields[1].substring(2), 16);
            MemoryBlock block = start == null ? null : currentProgram.getMemory().getBlock(start);
            if (block == null || !block.isInitialized() || size < 0 || size > 0x100000000L) {
                throw new IllegalArgumentException(file + ":" + (i + 1) + ": invalid range " + line);
            }
            Address end = size == 0 ? null : start.addNoWrap(size - 1);
            if (end != null && !block.contains(end)) {
                throw new IllegalArgumentException(file + ":" + (i + 1) + ": range leaves block " + line);
            }
            entries.add(new Entry(start, end, fields[3], i + 1));
        }
        return entries;
    }
}
