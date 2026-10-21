// SPDX-License-Identifier: MIT
// Use an x86, little-endian, 32-bit program with the borlandcpp compiler spec.
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.cmd.function.ApplyFunctionSignatureCmd;
import ghidra.app.util.cparser.C.CParserUtils;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.SourceType;
import java.nio.file.Files;
import java.nio.file.Path;

public class A90Decompile extends GhidraScript {
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 1) {
            throw new IllegalArgumentException("Supply an output file and optional function addresses");
        }
        for (String[] entry : new String[][] {
            {"13b2cc7", "void * __cdecl memset(void *dest, int value, unsigned int count);"},
            {"13b2afc", "void * __cdecl memcpy(void *dest, const void *src, unsigned int count);"},
            // Observed stack arguments and caller cleanup in the opcode parser.
            // Names describe operations. They are not recovered source symbols.
            {"13c4bd8", "void * __cdecl a90_string_init(void *dest);"},
            {"13c4c08", "void * __cdecl a90_string_ascii(void *dest, const char *src);"},
            {"13c4d3c", "void * __cdecl a90_string_utf16(void *dest, const unsigned short *src);"},
            {"13c4c44", "void * __cdecl a90_string_copy(void *dest, const void *src);"},
            {"13c4e38", "void * __cdecl a90_string_assign(void *dest, const void *src);"},
            // Each concatenation wrapper reads three stack arguments. It
            // returns the destination and leaves argument cleanup to callers.
            {"13c4e68", "void * __cdecl a90_string_concat(void *dest, const void *left, const void *right);"},
            {"13c5500", "void * __cdecl a90_string_concat_ascii(void *dest, const char *left, const void *right);"},
            {"13c55a4", "void * __cdecl a90_string_concat_utf16(void *dest, const unsigned short *left, const void *right);"},
            {"13c4e14", "void __cdecl a90_string_destroy(void *value, int flags);"},
            {"13c4eec", "bool __cdecl a90_string_equal(const void *left, const void *right);"},
            {"13c5454", "int __cdecl a90_string_integer(const void *value);"},
            // Wrappers call Trim and CharLowerBuffW through the Delphi RTL.
            {"13c5238", "void * __cdecl a90_string_trim(void *dest, const void *value);"},
            {"13c5110", "void * __cdecl a90_string_lower(void *dest, const void *value);"},
            {"13c509c", "void * __cdecl a90_string_make_unique(void *value);"},
            {"13c4b50", "void __cdecl a90_string_check_index(const void *value, int index);"},
            // 0040f140 reads destination/value at [ebp+8]/[ebp+c]. Callers
            // push both arguments and remove eight bytes after the call.
            {"40f140", "void * __cdecl a90_string_from_integer(void *dest, int value);"}
        }) {
            var address = toAddr(Long.parseLong(entry[0], 16));
            disassemble(address);
            if (getFunctionAt(address) == null) createFunction(address, null);
            var signature = CParserUtils.parseSignature(
                (ghidra.app.services.DataTypeManagerService)null, currentProgram, entry[1]);
            new ApplyFunctionSignatureCmd(address, signature, SourceType.USER_DEFINED)
                .applyTo(currentProgram);
        }
        long[] defaults = {
            0x59ed04, 0x59eddc, 0x59ee98,
            0x59f164, 0x59f1d8, 0x59f250, 0x5a01d4,
            0x5a0804, 0x5a0938, 0x5a09f4, 0x5a0ad0, 0x5a0bec, 0x5a0d4c,
            0x5a1b6c, 0x5a1be0, 0x5a1dc8,
            0x43b1e8, 0x43c5d8, 0x43c90c,
            0x45b478, 0x45b5d0, 0x45b730, 0x45b78c, 0x45bad8,
            0x608b30, 0x608dd0
        };
        long[] addresses = defaults;
        if (args.length > 1) {
            addresses = new long[args.length - 1];
            for (int i = 1; i < args.length; i++) {
                addresses[i - 1] = Long.parseLong(args[i].replaceFirst("^0[xX]", ""), 16);
            }
        }
        DecompInterface decompiler = new DecompInterface();
        decompiler.openProgram(currentProgram);
        StringBuilder output = new StringBuilder();
        try {
            for (long offset : addresses) {
                var address = toAddr(offset);
                disassemble(address);
                Function function = getFunctionAt(address);
                if (function == null) {
                    function = createFunction(address, "a90_" + Long.toHexString(offset));
                }
                if (function == null) throw new IllegalStateException("Invalid function: " + address);
                var result = decompiler.decompileFunction(function, 45, monitor);
                output.append("\n/* ADDRESS " + address + " */\n");
                if (result.decompileCompleted()) {
                    output.append(result.getDecompiledFunction().getC());
                } else {
                    output.append(result.getErrorMessage());
                }
                println("Decompiled " + address);
            }
        } finally {
            decompiler.dispose();
        }
        Files.writeString(Path.of(args[0]), output.toString());
    }
}
