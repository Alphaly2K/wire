# Windows code pages

`portable.rs` implements the existing CP932 (including aliases 0/1/3), CP1252
and UTF-8 contract with static tables and Rust string handling. It imports no
conversion function. Wasm NLS services use the same checked API owner.

`tables.json` records generator/reference identities and each table's hash.
Decode records are little-endian Unicode scalars (`ffffffff` means invalid).
Encode records have 9 bytes: scalar u32, payload offset u32, payload length u8.
They are sorted by scalar. Payloads include the reference's successful
nonreversible expansions, rather than reversing the decode mapping. Replacement
with `?` and `used_default=true` happens only on conversion failure, matching the
original implementation. This preserves its existing behavior; it is not a
claim that every approximation matches every Windows version.

Regenerate in a separate existing directory using the system iconv reference:

```
clang -O2 -Wall -Wextra -Werror tools/probes/generate_codepages.c -liconv -o /tmp/wire-codepages
/tmp/wire-codepages /tmp/new-codepage-tables
```

Compare all files with the recorded hashes before replacing tables. Normal
builds never generate tables and do not need iconv. NLS regression checks in
`crates/wire-wasm/tests/nls.mjs` cover conversion output, aliases, invalid
codepages and guest memory faults using stable repository-owned inputs.
