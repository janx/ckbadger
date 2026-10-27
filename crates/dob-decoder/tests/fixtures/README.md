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
