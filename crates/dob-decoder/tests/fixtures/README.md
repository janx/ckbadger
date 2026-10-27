# DOB pattern fixture

`nervape-legacy-pattern.json` is the literal `dob.pattern` from the Nervape
cluster `0xd5852c19fa4fa394d64915cafe026cdeb702ce53cf2b839c6ace501e8dead41c`.
It was extracted from the approved design's captured `data-cluster/detail.json`
response, whose decoder reference is the binary data hash
`0xb82abd59ade361a014f0abb692f71b0feb880693c3ccb95b9137b73551d872ce`.
This is a public chain-data fixture; its test does not assert the live chain's
current cluster state or re-execute the decoder.

The pattern parser describes byte provenance, not expressed traits. The source
schema and sequential byte consumption are documented in the upstream
[legacy Molecule schema](https://github.com/sporeprotocol/spore-dob-0/blob/5648032/src/schema/dob_0.mol)
and [decoder](https://github.com/sporeprotocol/spore-dob-0/blob/5648032/src/decoder/mod.rs).
The binary identity comes from the official
[mainnet deployment record](https://github.com/sporeprotocol/spore-dob-0/blob/a592680c9a507a014ebf505075cbef0b21382799/migration/mainnet/2024-04-25-014449.json).

JSON DOB/0 uses explicit, potentially overlapping ranges. Its reviewed
[schema](https://github.com/sporeprotocol/spore-dob-0/blob/a592680c9a507a014ebf505075cbef0b21382799/src/decoder/types.rs)
and [decoder](https://github.com/sporeprotocol/spore-dob-0/blob/a592680c9a507a014ebf505075cbef0b21382799/src/decoder/mod.rs)
clip the declared ranges to the actual DNA length. The helper reports declared
and actual ranges with a `truncated` flag rather than hiding that distinction.
The standard JSON deployment is
[13cac78a…](https://github.com/sporeprotocol/spore-dob-0/blob/a592680c9a507a014ebf505075cbef0b21382799/migration/mainnet/2024-06-24-124552.json).

Nervape's JSON dialect adds named rules, described in its
[schema](https://github.com/nervape/nervape-dob-decoder/blob/68a43dc5582c8c38321b515891c3914e95689ccc/src/decoder/types.rs).
The reviewed deployment identities are
[9c3d6729…](https://github.com/nervape/nervape-dob-decoder/blob/68a43dc5582c8c38321b515891c3914e95689ccc/migration/mainnet/2025-12-26-155936.json)
and [279af749…](https://github.com/nervape/nervape-dob-decoder/blob/68a43dc5582c8c38321b515891c3914e95689ccc/migration/mainnet/2026-01-29-022029.json).
The helper does not claim to have independently rebuilt these binaries. A Type
ID is not a binary identity: callers must resolve it to the current canonical
executable's actual data hash. Other binaries explicitly return unsupported.

Rule arguments remain literal. Legacy numeric pools use decimal strings so that
the complete `u64` range, including rational numerators and denominators, remains
exact through JSON. No allele selection, modulo arithmetic or float trait
calculation is duplicated here; stored CKB-VM results supply expressed traits.

## Mainnet 3X binary oracle

`nervape3x-98b7af5e.elf` is the exact 86,904-byte `output_data` of mainnet
outpoint `0xa83ad17ec7f70abd06bc3de0120e0fa39c5b2869b9e3b14189d927f737276f82:0`,
created at block 20,743,475. It was read from the local node on 2026-10-27.
The test recomputes its CKB-personalized Blake2b-256 hash and requires
`0x98b7af5e071c0d4af030447b25bb436ee4e9dd687daac7bca4bee7a0393b78e1`.
At capture the cell's Type ID args were
`0xf9e6e61b29ef7356cdacce217065bc1ec7ec95cf33f62d2c800b913a66688e3d`.
That mutable reference is not used to select the helper's grammar.

This deployment is not in the upstream repository's migration list. Support
therefore includes direct, offline CKB-VM regression evidence from the fixed
chain binary, rather than a claim that we reproduced its source build:

- The actual six-gene `nervape3x-pattern.json` and 53-byte object DNA produce
  the same values when each gene's byte range is supplied alone at offset zero.
- All 49 offset/length combinations over the boundary corpus read the same bytes
  as the helper, including overlaps, empty slices and clipped declarations.
- The 25 implemented rule names use the same segment in the full and isolated
  DNA cases. Two parser-accepted but unimplemented serial rules exit with code
  101; their byte descriptions do not claim a successful expressed value.

These are fixed protocol regression tests, not an independent source-build
proof. They run in the existing CKB-VM and need no local node or network. The
pattern was captured from cluster
`0x26a1fe3f4e84ae2cd1dc3e319ca416e27624c5ebe2f1b0aa731ca18d67784c4c`;
the DNA belongs to object
`0x51c75ad6c509e05ec5933e32093a5b32eeab7fe59dbe9ecc05546306002796f2`.
