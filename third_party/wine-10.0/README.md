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

## Menu, common USER32 and unified ABI additions (2026-10-06)

user32/menu.c and win32u/menu.c are complete unchanged source snapshots.
user32/dialog-controls.c and window-text.c are unchanged excerpts with original
headers. They are adapted in src/menu.rs and src/user32_common.rs, replacing
Wine backend calls with guest handles, checked memory and guest callbacks.
Eleven unchanged abi/*.spec files supply named/ordinal x86 stdcall contracts
to src/win32_abi.rs; they do not establish functional API coverage.
All sources use the same pinned Wine 10.0 commit above.

- `abi/comctl32.spec`: `575cf9d7ececcce0b42875e622ce578b6bb40d577693ee6ee27a79a61f0b08a3`
- `abi/comdlg32.spec`: `7ee7eb6ef510b49164ff2f8c2e7aebea13a18209c5f9a0f50ead38f572b83c3b`
- `abi/gdi32.spec`: `05e185aadfc199bdf9ba09db763c60155684bbb042cb0bab41146a1cfbeed6cb`
- `abi/kernel32.spec`: `ff1804fb97457a32d42c07b9da285847cbd2abfcd23f0c3729fda967645eceff`
- `abi/mpr.spec`: `825a52b81a8245e3b80a2d5ca0283ab3e8b1bce33683f88afced3627c195fd70`
- `abi/ole32.spec`: `124fc1cf35aa234b5496fe5e1ebe85a3dd81da32da9920ffae28ccbd83a21f14`
- `abi/oleaut32.spec`: `d8682694db5c65e1ac04b6a1c2f682bf34255a587d2198d1dac222e2af4abac2`
- `abi/shell32.spec`: `3c654161b9c4adf9ba05af42a6c3d60ec270700eabaa06ab5fdd0582482bff90`
- `abi/shlwapi.spec`: `93acc83072b0df0b43d1e6d6c104397f7bc6de221f23b4f57b3905b2112c979c`
- `abi/user32.spec`: `0aa00fa0a8f7c648d919be22afe823fb52488366e5d9ec0ca2dd8c292e364930`
- `abi/wininet.spec`: `a7ca36a207ad4f2edd3808b01a08e1232770f1c8c29e1e3e52ae24af46e6a4a3`
- `user32/dialog-controls.c`: `df980dbb42ebffa0c3c884d15537471af25aa915ba4ac7cadf1c6fcff1eae97f`
- `user32/menu.c`: `da08c13f8f817a526bf07a71f00779ccf8ef388aa311e0aa5c9cae01855cd0a2`
- `user32/window-text.c`: `161c3e9b0b4bb04f967810e9cd7f3101ac41c03ae1de50f6a2294f273c000cb8`
- `win32u/menu.c`: `1896b37ae70667e1ffafa73c13a4d1330093ae3da7f847f27cd9bc2d971b2a92`

## AMD64 version profile (2026-10-07)

`ntdll/version.c` is an unchanged Wine 10.0 snapshot, SHA-256
`d56533c640b093ba6e0ccd535783596c7b5e95adff17d5694ba1bc2a0f1b2b72`.
The comparison/update helpers, RtlVerifyVersionInfo and VerSetConditionMask are
adapted in `src/x64/runtime/version.rs`, retaining the original copyright notice
and selecting LGPL 2.1. Guest pointers use checked memory; version queries use
wire's explicit Windows 10.0.19045 workstation profile. Wine's registry,
manifest-version behavior and process-version-selection code are not ported.
The C snapshot is not built or linked. Copyright 1997 Marcus Meissner;
1998 Patrik Stridvall; 1998, 2003 Andreas Mohr; 1997, 2003 Alexandre Julliard.
