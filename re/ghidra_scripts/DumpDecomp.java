// Decompiles every function whose entry lies in the given address ranges into one text file per
// range, for grep-driven exploration (the per-query startup cost makes Query.java slow in bulk).
// Args: <outFile> <start>-<end> [<start>-<end>...]   addresses as in Query.java.
// Run through scripts/ghidra-dump.ps1.
//@category NDS
//@runtime Java

import java.io.PrintWriter;

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;

public class DumpDecomp extends GhidraScript {

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 2) {
            throw new IllegalArgumentException("usage: DumpDecomp <outFile> <start>-<end>...");
        }
        AddressSet set = new AddressSet();
        for (int i = 1; i < args.length; i++) {
            String[] parts = args[i].split("-");
            set.add(parse(parts[0]), parse(parts[1]));
        }
        DecompInterface decomp = new DecompInterface();
        decomp.openProgram(currentProgram);
        int count = 0;
        try (PrintWriter out = new PrintWriter(args[0], "UTF-8")) {
            FunctionIterator it = currentProgram.getFunctionManager().getFunctions(set, true);
            while (it.hasNext() && !monitor.isCancelled()) {
                Function f = it.next();
                DecompileResults r = decomp.decompileFunction(f, 60, monitor);
                out.println("//== " + f.getName() + " @ " + f.getEntryPoint());
                out.println(r.decompileCompleted()
                        ? r.getDecompiledFunction().getC()
                        : "decompile failed: " + r.getErrorMessage());
                count++;
            }
        }
        println("dumped " + count + " functions");
    }

    private Address parse(String s) {
        String text = s.startsWith("0x") ? s.substring(2) : s;
        Address a = currentProgram.getAddressFactory().getAddress(text);
        if (a == null) {
            throw new IllegalArgumentException("bad address " + s);
        }
        return a;
    }
}
