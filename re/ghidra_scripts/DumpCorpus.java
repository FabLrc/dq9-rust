// Decompiles every `game` function of one module from re/functions.txt into a single text file,
// so that RE questions ("what calls X", "where is constant Y", "show Z") are answered with a
// grep over a corpus instead of one 8-30 s analyzeHeadless run per question.
// One block per function, in map order:
//     // <addr> <size> <name>
//     <decompiled C>
// A function whose decompilation fails or times out keeps its header and gets
// `// decompile failed: <reason>` as body: the block stays in place and `corpus: failed` lines
// count it. Function names in headers come from re/functions.txt (authoritative); the C body
// carries the program's names (re/symbols.txt wins there).
// Args: <outFile> <functions.txt> <module> [timeoutSeconds]
//   <module> is `arm9` or `ov_NNN`; addresses are `0x020d91ec` or `ov_017::0218c0f4` as in the
//   map. timeoutSeconds is the per-function decompiler deadline (default 60): one pathological
//   function cannot stall the run.
// Output is written to <outFile>.tmp and renamed only on success, so an interrupted run never
// leaves a file that a resumable caller would take for complete.
// Run through scripts/ghidra-corpus.ps1 (read-only: the project is never modified).
//@category NDS
//@runtime Java

import java.io.File;
import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.List;

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSpace;
import ghidra.program.model.listing.Function;

public class DumpCorpus extends GhidraScript {

    /** One line of re/functions.txt that belongs to the requested module. */
    private static class Entry {
        final String text;
        final int size;
        final String name;

        Entry(String text, int size, String name) {
            this.text = text;
            this.size = size;
            this.name = name;
        }
    }

    private final List<String> failures = new ArrayList<>();

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 3) {
            throw new IllegalArgumentException(
                "usage: DumpCorpus <outFile> <functions.txt> <module> [timeoutSeconds]");
        }
        String out = args[0];
        File map = new File(args[1]);
        String module = args[2];
        int timeout = args.length > 3 ? Integer.parseInt(args[3]) : 60;

        List<Entry> entries = readEntries(map, module);
        if (entries.isEmpty()) {
            throw new IllegalArgumentException("no game function for module " + module + " in " + map);
        }
        long started = System.nanoTime();
        DecompInterface decomp = new DecompInterface();
        if (!decomp.openProgram(currentProgram)) {
            throw new IllegalStateException("cannot open the program in the decompiler");
        }
        println("corpus: start " + module + " entries=" + entries.size() + " timeout=" + timeout + "s");

        int done = 0;
        int failed = 0;
        Path temporary = Path.of(out + ".tmp");
        try (PrintWriter writer = new PrintWriter(Files.newBufferedWriter(temporary,
                StandardCharsets.UTF_8))) {
            for (Entry entry : entries) {
                if (monitor.isCancelled()) {
                    throw new IllegalStateException("cancelled after " + done + " functions");
                }
                String reason = null;
                String body = null;
                try {
                    Address address = parse(entry.text);
                    Function function = getFunctionAt(address);
                    if (function == null) {
                        function = getFunctionContaining(address);
                    }
                    if (function == null) {
                        reason = "no function at " + address;
                    } else {
                        DecompileResults r = decomp.decompileFunction(function, timeout, monitor);
                        if (r.decompileCompleted()) {
                            body = r.getDecompiledFunction().getC();
                        } else {
                            reason = r.getErrorMessage();
                        }
                    }
                } catch (Throwable t) {
                    reason = t.getClass().getSimpleName() + ": " + t.getMessage();
                }
                print(writer, "// " + entry.text + " 0x" + Integer.toHexString(entry.size) + " "
                    + entry.name);
                if (body == null) {
                    failed++;
                    String text = "// decompile failed: "
                        + (reason == null ? "unknown" : reason.replace('\n', ' ').trim());
                    print(writer, text);
                    failures.add(entry.text + " " + entry.name + " - "
                        + (reason == null ? "unknown" : reason.replace('\n', ' ').trim()));
                } else {
                    for (String line : body.split("\n")) {
                        print(writer, line);
                    }
                }
                print(writer, "");
                done++;
                if (done % 500 == 0) {
                    println("corpus: progress " + module + " " + done + "/" + entries.size()
                        + " failed=" + failed);
                }
            }
        }
        decomp.dispose();
        Files.move(temporary, Path.of(out), StandardCopyOption.REPLACE_EXISTING);

        for (String failure : failures) {
            println("corpus: failed " + module + " " + failure);
        }
        long ms = (System.nanoTime() - started) / 1_000_000;
        println("corpus: done " + module + " functions=" + done + " failed=" + failed + " ms=" + ms
            + " bytes=" + Files.size(Path.of(out)));
    }

    private void print(PrintWriter writer, String line) {
        writer.print(line.stripTrailing());
        writer.print('\n');
    }

    /** Map order, `game` category only, restricted to the requested module. */
    private List<Entry> readEntries(File map, String module) throws Exception {
        List<Entry> entries = new ArrayList<>();
        for (String line : Files.readAllLines(map.toPath())) {
            String text = line.trim();
            if (text.isEmpty() || text.startsWith("#")) {
                continue;
            }
            String[] parts = text.split("\\s+");
            if (parts.length < 4 || !parts[2].equals("game") || !moduleOf(parts[0]).equals(module)) {
                continue;
            }
            long size = Long.parseLong(parts[1].substring(2), 16);
            entries.add(new Entry(parts[0], (int) size, parts[3]));
        }
        return entries;
    }

    private static String moduleOf(String address) {
        int sep = address.indexOf("::");
        return sep < 0 ? "arm9" : address.substring(0, sep);
    }

    /** `0x020d91ec` (ARM9) or `ov_017::0218c0f4` (overlay space), independent of Ghidra's own
     * address-string syntax: the space is looked up by name and the offset parsed by hand. */
    private Address parse(String text) {
        AddressSpace space = currentProgram.getAddressFactory().getDefaultAddressSpace();
        String offset = text;
        int sep = text.indexOf("::");
        if (sep >= 0) {
            String name = text.substring(0, sep);
            space = currentProgram.getAddressFactory().getAddressSpace(name);
            if (space == null) {
                for (AddressSpace candidate : currentProgram.getAddressFactory()
                        .getAddressSpaces()) {
                    if (candidate.getName().endsWith(name)) {
                        space = candidate;
                        break;
                    }
                }
            }
            if (space == null) {
                throw new IllegalArgumentException("no address space " + name);
            }
            offset = text.substring(sep + 2);
        }
        if (offset.startsWith("0x")) {
            offset = offset.substring(2);
        }
        return space.getAddress(Long.parseLong(offset, 16));
    }
}
