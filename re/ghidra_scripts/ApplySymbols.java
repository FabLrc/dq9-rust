// Applies re/symbols.txt (the versioned source of truth for names) to the program.
// Line format:   <address> <name> [arm|thumb] [# comment]
//   address: 0x020d91ec, or ov_017::0218c0f4 for overlay code. Blank lines and lines
//   starting with '#' are ignored. If code starts at the address, a function is created
//   (or renamed); otherwise a label. `arm`/`thumb` marks a function Ghidra did not find
//   (e.g. only reached through a pointer table): it is disassembled in that mode first.
//   The comment becomes the plate comment.
// Args: [path to symbols.txt]
//@category NDS
//@runtime Java

import java.io.File;
import java.nio.file.Files;
import java.math.BigInteger;
import java.util.List;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.lang.Register;
import ghidra.program.model.listing.CodeUnit;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.SourceType;
import ghidra.program.model.symbol.Symbol;

public class ApplySymbols extends GhidraScript {

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        File file = args.length > 0 ? new File(args[0]) : askFile("symbols.txt", "Apply");
        List<String> lines = Files.readAllLines(file.toPath());
        int applied = 0;
        for (int n = 0; n < lines.size(); n++) {
            String line = lines.get(n).strip();
            if (line.isEmpty() || line.startsWith("#")) {
                continue;
            }
            String comment = null;
            int hash = line.indexOf('#');
            if (hash >= 0) {
                comment = line.substring(hash + 1).strip();
                line = line.substring(0, hash).strip();
            }
            String[] parts = line.split("\\s+");
            if (parts.length < 2 || parts.length > 3
                    || (parts.length == 3 && !parts[2].equals("arm") && !parts[2].equals("thumb"))) {
                throw new IllegalArgumentException(file + ":" + (n + 1) + ": expected '<address> <name> [arm|thumb]'");
            }
            String text = parts[0].startsWith("0x") ? parts[0].substring(2) : parts[0];
            Address addr = currentProgram.getAddressFactory().getAddress(text);
            if (addr == null || currentProgram.getMemory().getBlock(addr) == null) {
                throw new IllegalArgumentException(file + ":" + (n + 1) + ": unmapped address " + parts[0]);
            }
            if (parts.length == 3) {
                disassembleAs(addr, parts[2].equals("thumb"));
            }
            apply(addr, parts[1], comment);
            applied++;
        }
        println("ApplySymbols: applied " + applied + " symbols from " + file);
    }

    private void disassembleAs(Address addr, boolean thumb) throws Exception {
        if (getInstructionAt(addr) != null) {
            return;
        }
        Register tmode = currentProgram.getRegister("TMode");
        currentProgram.getProgramContext().setValue(tmode, addr, addr,
            thumb ? BigInteger.ONE : BigInteger.ZERO);
        if (!disassemble(addr)) {
            throw new IllegalStateException("cannot disassemble " + addr);
        }
    }

    private void apply(Address addr, String name, String comment) throws Exception {
        Function f = getFunctionAt(addr);
        if (f == null && getInstructionAt(addr) != null) {
            f = createFunction(addr, name);
        }
        if (f != null) {
            f.setName(name, SourceType.USER_DEFINED);
        } else {
            Symbol s = getSymbolAt(addr);
            if (s != null && s.getSource() != SourceType.DEFAULT) {
                s.setName(name, SourceType.USER_DEFINED);
            } else {
                createLabel(addr, name, true, SourceType.USER_DEFINED);
            }
        }
        if (comment != null && !comment.isEmpty()) {
            currentProgram.getListing().setComment(addr, CodeUnit.PLATE_COMMENT, comment);
        }
    }
}
