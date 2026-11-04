// Read-only queries against the analyzed program, for scripted/agent-driven RE.
// Args: <command> <address>... where address is `0x020f27f8` or `ov_017::0218c0f4`.
//   decomp <addr>...   decompile the function containing each address (created, unsaved,
//                      when the address is code that analysis did not reach)
//   xrefs  <addr>...   references to each address, with the containing function
//   calls  <addr>...   callers and callees of the function containing each address
//   disasm <addr> [n]  n instructions (default 32) starting at addr
// Run through scripts/ghidra-query.ps1.
//@category NDS
//@runtime Java

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.symbol.Reference;

public class Query extends GhidraScript {

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 2) {
            throw new IllegalArgumentException("usage: Query <decomp|xrefs|calls|disasm> <addr>...");
        }
        String cmd = args[0];
        if (cmd.equals("disasm")) {
            disasm(parse(args[1]), args.length > 2 ? Integer.parseInt(args[2]) : 32);
            return;
        }
        DecompInterface decomp = null;
        if (cmd.equals("decomp")) {
            decomp = new DecompInterface();
            decomp.openProgram(currentProgram);
        }
        for (int i = 1; i < args.length; i++) {
            Address addr = parse(args[i]);
            switch (cmd) {
                case "decomp" -> decompile(decomp, addr);
                case "xrefs" -> xrefs(addr);
                case "calls" -> calls(addr);
                default -> throw new IllegalArgumentException("unknown command " + cmd);
            }
        }
    }

    private Address parse(String s) {
        String text = s.startsWith("0x") ? s.substring(2) : s;
        Address a = currentProgram.getAddressFactory().getAddress(text);
        if (a == null) {
            throw new IllegalArgumentException("bad address " + s);
        }
        return a;
    }

    private String describe(Address a) {
        Function f = getFunctionContaining(a);
        return f == null ? a.toString() : f.getName() + "@" + f.getEntryPoint() + "+" + a.subtract(f.getEntryPoint());
    }

    private void decompile(DecompInterface decomp, Address addr) {
        Function f = getFunctionContaining(addr);
        if (f == null) {
            // Code only reached through data tables (callback arrays) is not found by the
            // auto-analysis: create the function in memory (the project is opened read-only).
            disassemble(addr);
            f = createFunction(addr, null);
        }
        if (f == null) {
            println("== no function contains " + addr);
            return;
        }
        DecompileResults r = decomp.decompileFunction(f, 120, monitor);
        println("== " + f.getName() + " @ " + f.getEntryPoint());
        String text = r.decompileCompleted() ? r.getDecompiledFunction().getC() : "decompile failed: " + r.getErrorMessage();
        for (String line : text.split("\n")) {
            if (!line.isBlank()) {
                println(line);
            }
        }
    }

    private void xrefs(Address addr) {
        println("== xrefs to " + addr);
        for (Reference ref : getReferencesTo(addr)) {
            println("  " + ref.getReferenceType() + " from " + describe(ref.getFromAddress()));
        }
    }

    private void calls(Address addr) {
        Function f = getFunctionContaining(addr);
        if (f == null) {
            println("== no function contains " + addr);
            return;
        }
        println("== " + f.getName() + " @ " + f.getEntryPoint());
        for (Function c : f.getCallingFunctions(monitor)) {
            println("  caller " + c.getName() + " @ " + c.getEntryPoint());
        }
        for (Function c : f.getCalledFunctions(monitor)) {
            println("  callee " + c.getName() + " @ " + c.getEntryPoint());
        }
    }

    private void disasm(Address addr, int count) {
        Instruction ins = getInstructionAt(addr);
        if (ins == null) {
            ins = getInstructionAfter(addr);
        }
        for (int i = 0; i < count && ins != null; i++, ins = ins.getNext()) {
            println(ins.getAddress() + "  " + ins);
        }
    }
}
