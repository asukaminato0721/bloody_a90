/* hid.dll proxy for the official program under Wine.
 *
 * Wine does not implement IOCTL_HID_GET_INDEXED_STRING, so
 * HidD_GetIndexedString always fails and the official program never
 * recognizes a Bloody mouse. This proxy forwards every import of
 * BloodyWorkShop8.exe to Wine's builtin hid.dll. When HidD_GetIndexedString
 * fails for USB 09da:fafd, it returns the A90 string descriptors instead:
 * 1 "COMPANY " and 2 "USB Device ", as reported by the kernel.
 *
 * Put the built hid.dll next to BloodyWorkShop8.exe and set a native-first
 * override for that program only (see build.sh). It does not touch reports.
 */
typedef unsigned char BOOLEAN;
typedef unsigned short USHORT;
typedef unsigned long ULONG;
typedef void *HANDLE;
typedef void *HMODULE;
typedef long (__stdcall *FARPROC)(void);
typedef unsigned short WCHAR;

typedef struct {
    ULONG Size;
    USHORT VendorID;
    USHORT ProductID;
    USHORT VersionNumber;
} HIDD_ATTRIBUTES;

__declspec(dllimport) HMODULE __stdcall LoadLibraryW(const WCHAR *name);
__declspec(dllimport) FARPROC __stdcall GetProcAddress(HMODULE module, const char *name);
__declspec(dllimport) void __stdcall OutputDebugStringA(const char *text);

__declspec(dllimport) int __stdcall VirtualProtect(void *address, unsigned long size,
                                                  unsigned long protect, unsigned long *old);

static HMODULE real;

/* After identifying the mouse, the official program decides at EXE
 * 0044cfde whether to post message 0x41b, which puts the mouse into firmware
 * update mode (28/1 clears the boot marker, 00 resets). Jump to the
 * no-update branch 0044d067 instead. Only this process's memory changes;
 * the EXE file and the mouse stay as they are. */
static void skip_update(void)
{
    unsigned char *at = (unsigned char *)0x0044cfde;
    unsigned long old;

    if (at[0] != 0x0f || at[1] != 0x84
        || *(long *)(at + 2) != 0x0044d067 - (0x0044cfde + 6))
        return;
    if (!VirtualProtect(at, 6, 0x40, &old))
        return;
    at[0] = 0xe9;
    *(long *)(at + 1) = 0x0044d067 - (0x0044cfde + 5);
    at[5] = 0x90;
    VirtualProtect(at, 6, old, &old);
    OutputDebugStringA("hid proxy: skipped the firmware update check\n");
}

static FARPROC get(const char *name)
{
    if (!real) {
        real = LoadLibraryW(L"C:\\windows\\syswow64\\hid.dll");
        skip_update();
    }
    return real ? GetProcAddress(real, name) : 0;
}

#define FORWARD(ret, name, params, args)                      \
    ret __stdcall name params                                  \
    {                                                          \
        typedef ret (__stdcall *type) params;                  \
        type function = (type)get(#name);                      \
        return function ? function args : (ret)0;              \
    }

FORWARD(BOOLEAN, HidD_FreePreparsedData, (void *data), (data))
FORWARD(BOOLEAN, HidD_GetAttributes, (HANDLE file, HIDD_ATTRIBUTES *attributes), (file, attributes))
FORWARD(BOOLEAN, HidD_GetFeature, (HANDLE file, void *buffer, ULONG length), (file, buffer, length))
FORWARD(BOOLEAN, HidD_GetPreparsedData, (HANDLE file, void **data), (file, data))

FORWARD(BOOLEAN, HidD_SetFeature, (HANDLE file, void *buffer, ULONG length), (file, buffer, length))
FORWARD(long, HidP_GetCaps, (void *data, void *caps), (data, caps))

void __stdcall HidD_GetHidGuid(void *guid)
{
    typedef void (__stdcall *type)(void *);
    type function = (type)get("HidD_GetHidGuid");
    if (function)
        function(guid);
}

BOOLEAN __stdcall HidD_GetIndexedString(HANDLE file, ULONG index, void *buffer, ULONG length)
{
    typedef BOOLEAN (__stdcall *type)(HANDLE, ULONG, void *, ULONG);
    type function = (type)get("HidD_GetIndexedString");
    HIDD_ATTRIBUTES attributes = {sizeof(attributes)};
    const WCHAR *text;
    WCHAR *out = buffer;
    ULONG i;

    if (function && function(file, index, buffer, length))
        return 1;
    if (!HidD_GetAttributes(file, &attributes) || attributes.VendorID != 0x09da
        || attributes.ProductID != 0xfafd)
        return 0;
    if (index == 1)
        text = L"COMPANY ";
    else if (index == 2)
        text = L"USB Device ";
    else
        return 0;
    for (i = 0; text[i]; i++)
        if ((i + 2) * sizeof(WCHAR) > length)
            return 0;
    for (i = 0; text[i]; i++)
        out[i] = text[i];
    out[i] = 0;
    OutputDebugStringA("hid proxy: returned A90 string descriptor\n");
    return 1;
}
