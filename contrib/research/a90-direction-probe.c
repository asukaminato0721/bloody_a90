/* Original pure-math reference probe. Do not call the EXE entry point.
 * Build for i686 Windows. No CRT, USER32, HID, USB or device imports. */
typedef unsigned short Wchar;
typedef unsigned int U32;
typedef unsigned char Byte;
#define WINCALL __attribute__((stdcall))
#define IMPORT __attribute__((dllimport))
IMPORT Wchar *WINCALL GetCommandLineW(void);
IMPORT void *WINCALL LoadLibraryExW(const Wchar *, void *, U32);
IMPORT void *WINCALL GetStdHandle(U32);
IMPORT U32 WINCALL GetLastError(void);
IMPORT int WINCALL VirtualProtect(void *, U32, U32, U32 *);
IMPORT int WINCALL WriteFile(void *, const void *, U32, U32 *, void *);
IMPORT void WINCALL ExitProcess(U32) __attribute__((noreturn));

static void *out;
static Wchar path[2048];
static void text(const char *s) {
    U32 length = 0, written;
    while (s[length]) length++;
    WriteFile(out, s, length, &written, (void *)0);
}
static char *number(char *p, int value) {
    char digits[12];
    int count = 0;
    U32 n;
    if (value < 0) { *p++ = '-'; n = 0u - (U32)value; } else n = (U32)value;
    do { digits[count++] = (char)('0' + n % 10); n /= 10; } while (n);
    while (count) *p++ = digits[--count];
    return p;
}
static char *hex(char *p, U32 value) {
    const char *digits = "0123456789abcdef";
    for (int shift = 28; shift >= 0; shift -= 4) *p++ = digits[(value >> shift) & 15];
    return p;
}
typedef double (WINCALL *Angle)(int, int, int, int);
typedef U32 (WINCALL *Sector)(double);
static Angle angle;
static Sector sector;
static unsigned short control;
static int relocate(Byte *image) {
    const U32 pe = *(U32 *)(image + 0x3c);
    const Byte *optional = image + pe + 24;
    const U32 image_size = *(const U32 *)(optional + 56);
    const U32 rva = *(const U32 *)(optional + 96 + 5 * 8);
    const U32 size = *(const U32 *)(optional + 100 + 5 * 8);
    const U32 delta = (U32)image - *(const U32 *)(optional + 28);
    if (!delta) return 1;
    if (image_size > 0x10000000 || rva > image_size || size > image_size - rva) return 0;
    U32 offset = 0;
    while (offset < size) {
        if (size - offset < 8) return 0;
        const U32 page = *(U32 *)(image + rva + offset);
        const U32 block_size = *(U32 *)(image + rva + offset + 4);
        if (block_size < 8 || block_size > size - offset || (block_size & 1) || page > image_size - 4096) return 0;
        const Wchar *entries = (const Wchar *)(image + rva + offset + 8);
        int cross_page = 0;
        for (U32 i = 0; i < (block_size - 8) / 2; i++) {
            if (entries[i] >> 12 == 3 && (entries[i] & 4095) > 4092) cross_page = 1;
        }
        U32 old, old_next = 0, unused;
        if (!VirtualProtect(image + page, 4096, 0x40, &old)) return 0;
        if (cross_page && (page > image_size - 8192 || !VirtualProtect(image + page + 4096, 4096, 0x40, &old_next))) return 0;
        for (U32 i = 0; i < (block_size - 8) / 2; i++) {
            const U32 entry = entries[i];
            if (entry >> 12 == 3) {
                *(U32 *)(image + page + (entry & 4095)) += delta;
            } else if (entry >> 12 != 0) return 0;
        }
        if (!VirtualProtect(image + page, 4096, old, &unused)) return 0;
        if (cross_page && !VirtualProtect(image + page + 4096, 4096, old_next, &unused)) return 0;
        offset += block_size;
    }
    return 1;
}
static void sample(int x, int y) {
    if (x == 0 && y == 0) return; /* The caller's distance filter excludes this. */
    __asm__ volatile ("fldcw %0" : : "m" (control));
    union { double value; U32 words[2]; } result;
    result.value = angle(x, y, 0, 0);
    U32 direction = sector(result.value), written;
    char row[90], *p = row;
    p = number(p, control); *p++ = '\t';
    p = number(p, x); *p++ = '\t';
    p = number(p, y); *p++ = '\t';
    p = number(p, direction); *p++ = '\t';
    p = hex(p, result.words[1]); p = hex(p, result.words[0]); *p++ = '\n';
    WriteFile(out, row, (U32)(p - row), &written, (void *)0);
}

void probe_start(void) {
    out = GetStdHandle((U32)-11);
    const Wchar *command = GetCommandLineW();
    int quoted = *command == '"';
    if (quoted) command++;
    while (*command && (quoted ? *command != '"' : *command != ' ')) command++;
    if (quoted && *command) command++;
    while (*command == ' ') command++;
    quoted = *command == '"';
    if (quoted) command++;
    int length = 0;
    while (*command && (quoted ? *command != '"' : *command != ' ')) {
        if (length == 2047) ExitProcess(2);
        path[length++] = *command++;
    }
    path[length] = 0;
    if (!length) { text("Supply the verified EXE path.\n"); ExitProcess(2); }
    /* 1 = DONT_RESOLVE_DLL_REFERENCES. No EXE startup or imported calls. */
    Byte *image = LoadLibraryExW(path, (void *)0, 1);
    if (!image) {
        char error[20], *end = hex(error, GetLastError()); *end++ = '\n'; *end = 0;
        text("Image mapping failed: "); text(error); ExitProcess(3);
    }
    const Byte prologue[] = {0x55, 0x8b, 0xec, 0x83, 0xc4, 0xe0, 0x53};
    for (U32 i = 0; i < sizeof(prologue); i++) {
        if (image[0xfef8a4 + i] != prologue[i]) { text("Unexpected function bytes.\n"); ExitProcess(4); }
    }
    /* Wine maps EXEs without applying base relocations in this load mode.
     * Apply the standard PE HIGHLOW relocations to this private memory view.
     * Restore page protections before calling the two math functions. */
    if ((*(U32 *)(image + 0xfef8bd) != (U32)(image + 0xfef944) && !relocate(image)) ||
        *(U32 *)(image + 0xfef8bd) != (U32)(image + 0xfef944)) {
        char detail[32], *end = hex(detail, (U32)image); *end++ = ' ';
        end = hex(end, *(U32 *)(image + 0xfef8bd)); *end++ = ' ';
        end = hex(end, (U32)(image + 0xfef944)); *end++ = '\n'; *end = 0;
        text("Image constants were not relocated: "); text(detail); ExitProcess(5);
    }
    angle = (Angle)(image + 0xfef8a4);
    sector = (Sector)(image + 0xfef954);
    text("control\tx\ty\tdirection\tangle_bits\n");
    for (int precision = 0; precision < 2; precision++) {
        control = precision ? 0x027f : 0x037f;
        for (int x = -24; x <= 24; x++) for (int y = -24; y <= 24; y++) sample(x, y);
        /* Scan the native 16-bit motion range around both diagonal boundaries. */
        for (int n = 25; n <= 32768; n++) {
            for (int difference = -28; difference <= 28; difference += 7) {
                const int x = n + difference;
                for (int sx = -1; sx <= 1; sx += 2) for (int sy = -1; sy <= 1; sy += 2) sample(sx * x, sy * n);
            }
        }
    }
    ExitProcess(0);
}
