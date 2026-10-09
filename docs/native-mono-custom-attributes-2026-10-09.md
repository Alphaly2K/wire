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
