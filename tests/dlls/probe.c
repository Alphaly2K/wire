#include <windows.h>

static volatile DWORD initialized;

BOOL WINAPI DllMain(HINSTANCE module, DWORD reason, LPVOID reserved) {
    (void)module;
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH)
        initialized = GetCurrentProcessId();
    else if (reason == DLL_PROCESS_DETACH)
        initialized = 0;
    return TRUE;
}

__declspec(dllexport) DWORD WINAPI ProbeValue(void) {
    return initialized + 37;
}
