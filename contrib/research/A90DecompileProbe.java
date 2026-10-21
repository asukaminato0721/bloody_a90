// SPDX-License-Identifier: MIT
// Normalize a verified stack probe, then restore its original bytes.
// Do not use this transformation to produce an executable file.
import ghidra.app.script.GhidraScript;
import ghidra.app.cmd.function.ApplyFunctionSignatureCmd;
import ghidra.app.util.cparser.C.CParserUtils;
import ghidra.program.model.symbol.SourceType;
import java.util.Arrays;

public class A90DecompileProbe extends GhidraScript {
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 1 || args.length > 2) {
            throw new IllegalArgumentException("Supply the output path and optional function address");
        }
        String target = args.length == 2 ? args[1].replaceFirst("^0[xX]", "") : "43b1e8";
        boolean macro = target.equalsIgnoreCase("62200c");
        if (!macro && !target.equalsIgnoreCase("43b1e8")) {
            throw new IllegalArgumentException("The function has no verified stack probe");
        }
        var function = toAddr(macro ? 0x62200c : 0x43b1e8);
        var start = toAddr(macro ? 0x622010 : 0x43b1ec);
        byte[] expected = {
            (byte)0xb8, (byte)(macro ? 0x78 : 0x10), 0, 0, 0,
            (byte)0x81, (byte)0xc4, 4, (byte)0xf0, (byte)0xff, (byte)0xff,
            0x50, 0x48, 0x75, (byte)0xf6
        };
        byte[] normalized = new byte[expected.length];
        Arrays.fill(normalized, (byte)0x90);
        // Each iteration subtracts 0xffc and pushes four bytes. Replace the
        // verified 16- or 120-page loop with one stack adjustment for analysis.
        int delta = -(macro ? 0x78000 : 0x10000);
        byte[] subtract = {(byte)0x81, (byte)0xc4, (byte)delta,
            (byte)(delta >> 8), (byte)(delta >> 16), (byte)(delta >> 24)};
        System.arraycopy(subtract, 0, normalized, 0, subtract.length);
        byte[] original = getBytes(start, expected.length);
        if (!Arrays.equals(original, expected) && !Arrays.equals(original, normalized)) {
            throw new IllegalStateException("The stack probe bytes do not match the checked EXE");
        }
        try {
            clearListing(start, start.add(expected.length - 1));
            setBytes(start, normalized);
            disassemble(start);
            var signature = CParserUtils.parseSignature(
                (ghidra.app.services.DataTypeManagerService)null, currentProgram,
                macro ? "void __fastcall a90_expand_macro(void *assignment);" :
                    "int __fastcall a90_save_buttons(int handle);");
            new ApplyFunctionSignatureCmd(function, signature, SourceType.USER_DEFINED)
                .applyTo(currentProgram);
            runScript("A90Decompile.java", new String[] {args[0], target});
        } finally {
            // A nested transaction cannot roll back before its outer script
            // transaction ends. Restore the bytes explicitly in this block.
            clearListing(start, start.add(expected.length - 1));
            setBytes(start, expected);
            disassemble(start);
        }
        if (!Arrays.equals(getBytes(start, expected.length), expected)) {
            throw new IllegalStateException("The original probe bytes were not restored");
        }
        println("Original stack probe bytes restored. The EXE file was not changed.");
    }
}
