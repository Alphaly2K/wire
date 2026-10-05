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
| dlls/win32u/defwnd.c | windowpos.c / geometry.c; default window messages and nonclient geometry |
| include/d3d9.h | unmodified d3d9.h; derived d3d9-methods.tsv, COM slot/arity ABI only |

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

- `geometry.c`: `07df351f137ce4f11215a449997ed3f324981b8034bb5728fe3e42dd3844892b`
- `d3d9.h`: `fb49b3c44acec5e100a5c0ff1622f7d0207842d87fc513b49047a818e8c351b2`
- `d3d9-methods.tsv`: `bca412d281e8b1ce1002653bc39e9e01a5781efccdb2b080d32f46871ff51d03`

D3D9 header copyright 2002–2003 Jason Edmeades and Raphael Junqueira.
The derived table includes the COM `this` DWORD in every x86 stdcall size.
The bounded CPU rasterizer in src/d3d9/ is independently implemented;
no Wine D3D/wined3d renderer, shader compiler, driver or executable code
is copied or linked. Reviewed LGPL-2.1-or-later include/d3d9types.h and
include/d3d9caps.h define the public data layouts and constants.
