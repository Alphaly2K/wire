# Wine source used by wire

Upstream: https://github.com/wine-mirror/wine
Version: Wine 10.0
Commit: `b073859675060c9211fcbccfd90e4e87520dc2c2`
Annotated tag: `2e9cb99369e5ead9d7fe9836dcd43c05e42890bc`

The imported C files retain their original copyright and LGPL notices. Wine
allows LGPL 2.1 or any later version; wire selects LGPL 2.1 for this combined
work. The original COPYING.LIB is preserved here in addition to wire's LICENSE.

| Upstream file | Local snapshot / port | Use |
| --- | --- | --- |
| dlls/gdiplus/matrix.c | gdiplus/matrix.c; src/gdiplus.rs | Matrix arithmetic, order, inversion, comparisons, point transforms |
| dlls/gdiplus/gdiplus.c | gdiplus/gdiplus.c; src/gdiplus.rs | Startup validation, zeroed allocation and shutdown contract |
| dlls/gdiplus/gdiplus_private.h | gdiplus/float-helpers.h; src/gdiplus.rs | Degree conversion and integer point rounding |
| dlls/gdiplus/gdiplus.spec | gdiplus/gdiplus.spec; src/gdiplus.rs | Named stdcall export ABI, including float and by-value GUID widths |
| dlls/user32/win.c | user32/win-enumeration.c; src/user32_enum.rs | Top-level/thread enumeration snapshot, skip destroyed windows, callback stop |

Copyright (C) 2007 Google (Evan Stade) for the GDI+ C implementations and helpers.
Copyright 1993, 1994 Alexandre Julliard for the user32 enumeration source.
The export specification has no per-file copyright header; it is preserved
unchanged under the Wine project's COPYING.LIB.

The user32 and float-helper snapshots are unmodified excerpts with the original
file headers, not complete source files. No C snapshots are built or linked; gdiplus.spec
is embedded as ABI data. Rust ports retain the original notices. Wire replaces
Wine allocation and native pointer dereferences with checked guest memory,
tracks object lifetimes and real startup tokens, bounds point arrays, and uses
the existing guest callback/SEH boundaries. Unsupported GDI+ exports return
NotImplemented, not Status::Ok. Notification hooks, drawing, fonts, codecs and
Wine server/window backends are not imported. Window ordering currently uses
wire's creation order rather than complete Wine z-order.

## Source checksums (SHA-256)

- `gdiplus/matrix.c`: `fcc6e357a2057312660ad4ce9f0724f90b41cbdf2edd250f6a575bd6c54c95d2`
- `gdiplus/gdiplus.c`: `8a70ca03724c0f3f446feb36b3b9026d311a1f7b419d8453c49203c227131e11`
- `gdiplus/gdiplus.spec`: `36ebe04076f7cc5a4ef7c26ed37e45231fd7ac471235898f55624103a052b009`
- `gdiplus/float-helpers.h`: `bae72d750f540db04df93839a7b6c75cd8e43ac4b2767fe6e9b2d9afc7742ac9`
- `user32/win-enumeration.c`: `1bc57909ec1ee4eb30bb3b90690f3d1af1531a9ff8639c0ef13c5072fdfe2543`
- `COPYING.LIB`: `e237fa56668030e928551ddd60f05df5fe957f75eab874bbd017e085ed722e7c`
- Original complete `dlls/user32/win.c`: `dbe619a36144c31416c3e6320243b1f22aa98d05a6bd59fa79659dcab8436c58`
- Original complete `dlls/gdiplus/gdiplus_private.h`: `942454ba766d0859c42e12eb0f8b4142f6f61206718fb4c9dc7b0f993399c256`
