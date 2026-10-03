// wire control program ("hello-world" grade Win32): file I/O + window + GDI blit.
// Used to isolate loader/interpreter issues before running the real target.
// Build: tests/control/build.sh (requires mingw-w64).
#include <windows.h>

int WINAPI WinMain(HINSTANCE hInst, HINSTANCE hPrev, LPSTR lpCmd, int nShow) {
    (void)hPrev; (void)lpCmd; (void)nShow;

    /* 1. file I/O */
    HANDLE h = CreateFileA("wire_control_test.txt", GENERIC_WRITE, 0, NULL,
                           CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (h != INVALID_HANDLE_VALUE) {
        DWORD written = 0;
        const char *msg = "wire control program\n";
        WriteFile(h, msg, 21, &written, NULL);
        CloseHandle(h);
    }

    /* 2. window (DefWindowProcA used directly as WndProc: avoids exercising
           the callback mechanism, which is M2+ scope) */
    WNDCLASSA wc;
    ZeroMemory(&wc, sizeof(wc));
    wc.lpfnWndProc = DefWindowProcA;
    wc.hInstance = hInst;
    wc.lpszClassName = "WireControl";
    wc.hbrBackground = (HBRUSH)(COLOR_WINDOW + 1);
    RegisterClassA(&wc);

    HWND hwnd = CreateWindowExA(0, "WireControl", "wire control",
                                WS_OVERLAPPEDWINDOW, CW_USEDEFAULT, CW_USEDEFAULT,
                                320, 240, NULL, NULL, hInst, NULL);
    if (hwnd) {
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);

        /* 3. GDI blit */
        HDC hdc = GetDC(hwnd);
        HDC memdc = CreateCompatibleDC(hdc);
        HBITMAP bmp = CreateCompatibleBitmap(hdc, 64, 64);
        SelectObject(memdc, bmp);
        BitBlt(hdc, 0, 0, 64, 64, memdc, 0, 0, SRCCOPY);
        ReleaseDC(hwnd, hdc);
        DeleteObject(bmp);
        DeleteDC(memdc);

        MSG msg;
        while (PeekMessageA(&msg, NULL, 0, 0, PM_REMOVE)) {
            TranslateMessage(&msg);
            DispatchMessageA(&msg);
        }
    }
    return 0;
}
