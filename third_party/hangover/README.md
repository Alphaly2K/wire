# Hangover reference and source excerpts

Requested upstream: https://github.com/AndreRH/hangover
Hangover commit: `56e385b7ff490edd281b288f420677093563560b`
Its Wine submodule: https://github.com/AndreRH/wine
Wine commit: `3ea3a14941b6a122b892c84141ce9662c008081e`
Reviewed/adapted: 2026-10-06–2026-10-08. Both project licenses are LGPL 2.1. Every C
file used below expressly permits LGPL 2.1 or later; wire selects LGPL 2.1.

| Wine file | Local snapshot / adaptation |
| --- | --- |
| dlls/oleaut32/oleaut.c | oleaut-bstr.c; BSTR operations in src/common_api.rs |
| dlls/win32u/defwnd.c | windowpos.c / geometry.c; default window messages and nonclient geometry |
| include/d3d9.h | unmodified d3d9.h; derived d3d9-methods.tsv, COM slot/arity ABI only |
| dlls/kernelbase/path.c | unmodified path.c; PathRemoveFileSpec, PathAddExtension, PathUnquoteSpaces A/W adapted in src/file_api.rs |

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
- `path.c`: `5ab6715af29660476cabc10ead698e53b91bc9e6d757b73299c17fe6c4784c05`

Path source copyright 2018 Nikolay Sivov, 2018 Zhiyi Zhang,
2021–2023 Zebediah Figura. This file explicitly permits LGPL-2.1-or-later;
wire selects LGPL 2.1. Native pointer scanning is replaced by checked guest
units/CP932 character boundaries. No native path/file backend is linked.

2026-10-08: semantic review of `dlls/combase/combase.c` at the same pinned
Wine commit for CoCreateGuid, GUID string functions and their HRESULTs.
Copyright 2005 Juan Lang, 2005–2006 Robert Shearman for CodeWeavers;
LGPL-2.1-or-later. `src/x64/runtime/guid.rs` independently serializes Windows
GUID fields, uses host OS entropy and checked 64-bit guest allocations.
No upstream RPC, parser or allocator implementation is copied or linked.
The isolated profile has no HKCR ProgID/activation-context registrations;
CLSIDFromString returns CO_E_CLASSSTRING for those unresolved names.

Semantic review on 2026-10-08 also covers `dlls/combase/string.c` and
`dlls/combase/roapi.c` at the same fixed Wine commit (LGPL-2.1-or-later).
Copyright 2014 Martin Storsjo and, for roapi.c, 2016 Michael Müller.
`src/x64/runtime/winrt.rs` uses independent checked guest storage for HSTRING
headers, immutable copies, reference counts and API-set dispatch. No Wine
implementation or allocator is imported. Missing WinRT classes fail with
REGDB_E_CLASSNOTREG; there is no invented activation factory or gamepad.

- `geometry.c`: `07df351f137ce4f11215a449997ed3f324981b8034bb5728fe3e42dd3844892b`
- `d3d9.h`: `fb49b3c44acec5e100a5c0ff1622f7d0207842d87fc513b49047a818e8c351b2`
- `d3d9-methods.tsv`: `0467452cc8f2e299d6acfc2433212ac374a67951204b83cbdf3ddcf9187fc94f`

D3D9 header copyright 2002–2003 Jason Edmeades and Raphael Junqueira.
The derived table includes the COM `this` DWORD in every x86 stdcall size.
The bounded CPU rasterizer in src/d3d9/ is independently implemented;
no Wine D3D/wined3d renderer, shader compiler, driver or executable code
is copied or linked. Reviewed LGPL-2.1-or-later include/d3d9types.h and
include/d3d9caps.h define the public data layouts and constants.

## PE32 compatibility additions (2026-10-07)

All snapshots below are unmodified files from the same fixed Wine commit,
`3ea3a14941b6a122b892c84141ce9662c008081e`. Source URLs are
`https://raw.githubusercontent.com/AndreRH/wine/3ea3a14941b6a122b892c84141ce9662c008081e/`
followed by the upstream path in this table.

| Upstream path | Local file | Used scope |
| --- | --- | --- |
| dlls/msvcrt/mbcs.c | mbcs.c | CP932 `_mbsinc` and byte-limited `_mbsnbcpy` rules |
| dlls/user32/wsprintf.c | wsprintf.c | Windows format/type/width/precision rules; checked i386 varargs |
| include/wbemcli.idl | wbemcli.idl | WMI COM layout and signatures; independent local OS provider |
| dlls/dsound/dsound.spec | dsound.spec | Named and ordinal i386 export ABI |
| dlls/advapi32/advapi32.spec | advapi32.spec | Export ABI; registry failures remain explicit |
| dlls/winmm/winmm.spec | winmm.spec | Export ABI, including MCI stack cleanup |
| include/dsound.h | dsound.h | DirectSound COM/data layouts; derived dsound-methods.tsv |

`mbcs.c`: Copyright 1999 Alexandre Julliard, 2000 Jon Griffths.
`wsprintf.c`: Copyright 1996 Alexandre Julliard.
`wbemcli.idl`: Copyright 2009 Hans Leidekker for CodeWeavers.
`dsound.h`: Copyright the Wine project.
These files expressly permit LGPL-2.1-or-later; wire selects LGPL 2.1.
The export specs retain upstream metadata and belong to the same LGPL Wine
source tree. The copies retain their original notices. No GPL QEMU code is
copied or linked. The independent CPU PCM mixer and public AudioQueue bridge
are not ports of Wine's audio driver.

`d3d9-methods.tsv` now also derives vertex/index-buffer and vertex-declaration
slots and arities from our unchanged `d3d9.h`. No upstream graphics renderer is included.

Semantic review only, without importing implementation:
`dlls/ntdll/loader.c` (Copyright 1995/2003 Alexandre Julliard,
2002 Dmitry Timoshkov; LGPL-2.1-or-later) for absent relocation directories;
`dlls/d3d9/d3d9_main.c` for the SDK version argument;
`dlls/win32u/message.c` (Copyright 2001 Alexandre Julliard,
2008 Maarten Lankhorst; LGPL-2.1-or-later)
for MsgWait queue masks and wait-all. These reviews do not provide a native
Wine server, loader, message dispatcher or renderer.

Additional SHA-256 checksums:
- `mbcs.c`: `53920d2cd072217734db62e378f0ec03ae5c7ec073a223bd5e8b352f439f3703`
- `wsprintf.c`: `e00663b26d1f8290afc1e6ad01a2c6e7d7d58170159701b0f8fea0916ed4d421`
- `wbemcli.idl`: `f3a230221e86ac8729c3079bed8982549ac2ef9e09239e883b304023f456fd6c`
- `dsound.spec`: `c349055e8cce1d8f285a249e521582b8dea551324ea2c66d542493dec40d24bb`
- `advapi32.spec`: `976ae51433e044ac9d7c4b59fec5ada727c868d0303decef8d0a6315ae1b05da`
- `winmm.spec`: `8139149e997e90aad0a0bdab62eb7b38da46c029f840d395e253128c996def4f`
- `dsound.h`: `afead216b15386912f1ac4e2d147064a542ed3ab5c112c09a4edf5617e3d9bcf`
- `dsound-methods.tsv`: `4a886e7e885836381a14167beb7ce1edba55f961de00274f5f1dd7f415094b8f`

## Bounded shader and loader review (2026-10-07)

Unmodified public headers from the same pinned Wine commit:

- `include/d3d9types.h` -> `d3d9types.h`: `ca6537e10480d694582ea1db785cb0805425df8268932d435b03fe9cc0e5820f`.
- `include/d3d9caps.h` -> `d3d9caps.h`: `c2d0e534dccaa47eb211dc491a668ab1823dea087cc5bbdc05e1d11ad46efa8f`.

Both expressly permit LGPL-2.1-or-later. Copyright 2002–2003 Jason Edmeades
and Raphael Junqueira; d3d9types.h also Copyright 2005 Oliver Stieber.
The derived d3d9-methods.tsv adds PixelShader9, VertexShader9 and StateBlock9 COM ABI.
The bounded PS2/VS2 interpreters and static Metal program are independently
implemented. No Wine shader compiler, renderer or driver is copied or linked.

Semantic review only, without importing implementation:
`dlls/d3d9/device.c` (Copyright 2002–2005 Jason Edmeades and Raphael Junqueira,
2005 Oliver Stieber) for successful Reset's returned presentation parameters
and SetViewport storage, plus Begin/End/CreateStateBlock recording boundaries;
`dlls/ntdll/loader.c` (Copyright 1995/2003 Alexandre
Julliard, 2002 Dmitry Timoshkov for CodeWeavers) for TLS callback / DllMain
notification ordering and DisableThreadLibraryCalls; `server/mapping.c`
(Copyright 1999 Alexandre Julliard) for page-rounded images and final section
file padding. Each file explicitly permits LGPL-2.1-or-later; wire selects
LGPL 2.1. These reviews do not import a native Wine loader, server or backend.
No GPL QEMU implementation is incorporated. Unicorn remains a development-only
oracle and is absent from the runtime dependency graph.
