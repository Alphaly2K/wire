# Checked native Mono custom attributes

## Runtime boundary

Patch 0022 exports checked class/field attribute metadata queries, single attribute
construction, and attribute array construction. MonoError storage remains entirely
inside the locked Mono runtime; callers receive only a uint16 error code. A failed
constructor returns an error rather than asserting or pretending no attribute exists.
The non-dynamic image paths own their MonoCustomAttrInfo until mono_custom_attrs_free.

The runtime is independently built in `wire-out/native-mono-attributes-runtime`:

- runtime SHA256: `94a8252dfb4bf7aa3584be4b1888afd128431cc7ea70b7bd6773fc299cab0a5e`
- corelib support SHA256: `7cd421d0e370b09a865848a59e47aa2b1362fd9ff484804fe3b2c1b4a5bc8029`

Nine original fixture suites (core, interop, threads, gc, layout, remoting,
collation, static-roots, attrs) pass both normal and Unity incremental=3 profiles.
The attribute fixture constructs class/field attributes, checks constructor and
named field bytes before and after collection, roots a real Attribute[] array,
and verifies constructor exceptions in both single and array paths. A NULL class
is refused with an error code. Five noexec negative routes exit 86 as expected.
Patch-stack build tests: 5 passed.

Frozen results are `probe/results-normal.json`, `probe/results-incremental.json`;
logs are `wire-out/native-mono-attributes-fixtures-02.log`,
`wire-out/native-mono-attributes-incremental-fixtures.log`, and
`wire-out/native-mono-attributes-build-tests.log`. These validate the native runtime
fixtures, not complete game admission. The earlier 21-patch runtime is unchanged.

## Actual Unity bridge

The `attributes` finite diagnostic boundary uses only the 22-patch runtime.
Class/field metadata are kind-checked opaque handles. Attribute info handles and
managed strong roots have separate ownership; forged handles, double frees and
retired access fail. Attribute objects expose no native headers. Primitive/enum
instance fields return owned bytes after class/field validation and guest output
permission checks. Static, reference, struct, pointer and byref fields are refused.
The main-stack barrier accepts only an already owned rooted token.

`wire-out/native-mono-unity-attributes-probe-02` exits 0, noexec guard 231/0:
8 ordered managed startup steps, 130 image cycles, 59279 metadata queries,
12 actual attribute instances and 36 primitive field reads. Strong GC roots and
field byte snapshots survive forced collection. The prior 585 arrays/handles,
7 RuntimeTypes and 12 strings also pass GC/cleanup. Info and attribute roots
release before image retirement. Original x64 Mono does not attach. Next pending
boundary is `mono_error_init`, preceding `mono_unity_custom_attrs_construct`.

20 targeted startup Rust tests and all-target feature Clippy pass. Probe-01 failed
because the stack-root barrier initially lacked attribute identities; it has no
successful final GC validation and is not completion evidence. This phase does
not enable the full-game backend or demonstrate a whole-game speed improvement.

## Interpreter integration

Merge `21db3ab` incorporates main `cafe7e8` with the completed FP/SIMD and bounded
translation reuse optimizations. Runtime code merged without conflicts. Full
feature Rust regression (420 passed, 7 ignored) and all-target Clippy pass (logs:
`native-mono-integrated-feature-tests.log`, `native-mono-integrated-clippy.log`).
`native-mono-unity-attributes-integrated-01` independently passes the same actual
attribute boundary and GC/resource lifecycle, guard 231/0. Its timing is a finite
functional probe, not an AB/BA performance comparison.

## Attribute arrays and MonoError contract under development

The locked original PE verifies `mono_error_init` at RVA 0x776d0 as
`33c08901c3` (zero its 4B init word), `mono_error_ok` at 0x77820 as
`33c06639010f94c0c3` (test the 16-bit error code), and
`mono_error_get_error_code` at 0x77830 as `0fb701c3`. Source external layout is
104B on Win64: code/flags init word, padding, and 12 hidden pointer fields.
Only a status snapshot is exposed to the guest; native error storage is never
passed to interpreted x64 code. Checked construction cleans native error
resources and returns a uint16 status, preserving a real failure as nonzero.
Guest init preserves the hidden bytes and writes the original 4B init word.
Snapshot cleanup writes the original 0xffff sentinel. Unknown, modified, double
cleaned, readonly, unaligned, non-stack and retired snapshots are rejected.

Real Attribute[] results are rooted before element traversal. An owned readonly
view contains only NULL bounds, checked length, and rooted opaque element tokens.
No vtable, synchronization word or native element pointer crosses into the guest.
Array and element identities are checked again after forced GC; views revoke only
after roots release. This remains a bounded startup contract, not arbitrary arrays.

`native-mono-unity-attribute-arrays-02` exits 0, guard 231/0: one actual
Attribute[] array with five elements, 33 rooted attribute objects, 85 field reads,
and 72858 metadata queries. The array and every element retain identity across
forced GC; field byte snapshots match. One native construction status is observed
by actual Unity `mono_error_ok`. Arrays, error snapshots, handle cells and roots
retire on cleanup. Next pending boundary is `mono_assembly_name_parse`.

Unity directly reads strong v2 handles in its C++ fast path, as documented by
Mono's Boehm alloc_handle implementation. The bridge now publishes only an owned
readonly 8B object-token cell at each aligned strong handle token; adjacent bytes
fault. Explicit free first unmaps/revokes that cell. This does not expose native
handle pointers or allow writes. 586 actual external handles are used here.
The initial array probe stopped on that unmapped direct-read cell, and did not
complete its final GC verification. 22 targeted Rust tests and all-target feature
Clippy pass, including direct-cell readonly bounds and one-release/revocation.
