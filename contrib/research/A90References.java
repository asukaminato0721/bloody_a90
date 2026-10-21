// SPDX-License-Identifier: MIT
// Export references and their containing functions from the Ghidra database.
import ghidra.app.script.GhidraScript;
import java.nio.file.Files;
import java.nio.file.Path;

public class A90References extends GhidraScript {
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 2) {
            throw new IllegalArgumentException("Supply an output path and hexadecimal addresses");
        }
        StringBuilder output = new StringBuilder("target\tfrom\tfunction\ttype\n");
        for (int i = 1; i < args.length; i++) {
            var address = toAddr(Long.parseLong(args[i].replaceFirst("^0[xX]", ""), 16));
            for (var reference : getReferencesTo(address)) {
                var from = reference.getFromAddress();
                var function = getFunctionContaining(from);
                output.append(address).append('\t').append(from).append('\t')
                    .append(function == null ? "" : function.getEntryPoint()).append('\t')
                    .append(reference.getReferenceType()).append('\n');
            }
        }
        Files.writeString(Path.of(args[0]), output.toString());
    }
}
