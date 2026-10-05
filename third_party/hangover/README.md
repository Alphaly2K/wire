# Hangover reference and source excerpts

Requested upstream: https://github.com/AndreRH/hangover
Hangover commit: `56e385b7ff490edd281b288f420677093563560b`
Its Wine submodule: https://github.com/AndreRH/wine
Wine commit: `3ea3a14941b6a122b892c84141ce9662c008081e`
Reviewed/adapted: 2026-10-06. Both project licenses are LGPL 2.1. Every C
file used below expressly permits LGPL 2.1 or later; wire selects LGPL 2.1.

| Wine file | Local snapshot / adaptation |
| --- | --- |
| dlls/oleaut32/oleaut.c | oleaut-bstr.c; BSTR operations in src/common_api.rs |
| dlls/win32u/defwnd.c | windowpos.c; default WM_MOVE/WM_SIZE in src/user32_common.rs |

Copyright 1999, 2000 Marcus Meissner (oleaut.c).
Copyright 1993, 1996 Alexandre Julliard, 1995 Alex Korobka,
2022 Jacek Caban for CodeWeavers (win32u/defwnd.c).
The snapshots preserve original notices and unmodified function excerpts.
They are not built. Guest heaps, bounds checks and callback control transfers
replace Wine's allocators, native pointers and message backend. BSTR caching
is not imported. Wire zeroes newly grown BSTR storage.

Also reviewed, without importing implementation: dlls/wow64win/user.c and
gdi.c (Copyright 2021 Jacek Caban for CodeWeavers),
wow64win_private.h and dlls/wow64/syscall.c (Copyright 2021 Alexandre Julliard).
Their pointer/handle, MSG32, CREATESTRUCT32, WINDOWPOS32 and DIB output
conversion boundaries informed the ABI review. All four headers are
LGPL-2.1-or-later. They require a working native Wine backend; wire does
not link Hangover/Wine, its emulators, Wine server or Linux drivers.

COPYING.LIB is copied unchanged from the pinned Wine submodule.
SHA-256 checksums follow.

- `COPYING.LIB`: `e237fa56668030e928551ddd60f05df5fe957f75eab874bbd017e085ed722e7c`
- `oleaut-bstr.c`: `373527f447da12eb208bb3e4656cb02edc920a96b316ddf9352ec0fc4da9f197`
- `windowpos.c`: `596c87b7489753600e1220d4a135cf403aa084a6d5dd078aa105017193173c9d`
