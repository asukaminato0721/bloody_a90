// SPDX-License-Identifier: MIT
// Decompile verified PE function entry addresses. Does not execute the image.
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import java.nio.file.Files;
import java.nio.file.Path;

public class A90DecompilePE extends GhidraScript {
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 2) throw new IllegalArgumentException("Supply an output file and function entry addresses");
        DecompInterface decompiler = new DecompInterface();
        decompiler.openProgram(currentProgram);
        StringBuilder output = new StringBuilder();
        try {
            for (int i = 1; i < args.length; i++) {
                var address = toAddr(Long.parseLong(args[i].replaceFirst("^0[xX]", ""), 16));
                var function = getFunctionAt(address);
                if (function == null) {
                    disassemble(address);
                    function = createFunction(address, null);
                }
                if (function == null) throw new IllegalArgumentException("Invalid entry: " + address);
                var result = decompiler.decompileFunction(function, 45, monitor);
                output.append("\n/* ADDRESS ").append(address).append(" */\n");
                if (!result.decompileCompleted()) throw new IllegalStateException(result.getErrorMessage());
                output.append(result.getDecompiledFunction().getC());
                println("Decompiled " + address);
            }
        } finally { decompiler.dispose(); }
        Files.writeString(Path.of(args[0]), output.toString());
    }
}
