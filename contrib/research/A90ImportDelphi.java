// SPDX-License-Identifier: MIT
// Import method labels from delphi-index.rs. Do not change function signatures.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.symbol.SourceType;
import java.nio.file.Files;
import java.nio.file.Path;

public class A90ImportDelphi extends GhidraScript {
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length != 1) {
            throw new IllegalArgumentException("Supply a Delphi index TSV file");
        }
        int count = 0;
        var symbols = currentProgram.getSymbolTable();
        for (String line : Files.readAllLines(Path.of(args[0]))) {
            monitor.checkCancelled();
            String[] fields = line.split("\t", -1);
            if (fields.length != 5 || !fields[0].equals("METHOD")) continue;
            var address = toAddr(Long.parseUnsignedLong(fields[1], 16));
            if (!currentProgram.getMemory().contains(address)) continue;
            String label = ("Delphi_" + fields[2] + "_" + fields[3])
                .replaceAll("[^A-Za-z0-9_]", "_");
            if (symbols.getGlobalSymbol(label, address) == null) {
                symbols.createLabel(address, label, SourceType.IMPORTED);
                count++;
            }
        }
        println("Imported " + count + " Delphi method labels");
    }
}
