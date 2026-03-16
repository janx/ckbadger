# Scripts, Code Cells, and References

This document records the script model ckbadger should use when reasoning about script detail
pages, code cells, versions, and labels.

## Why This Exists

Several different concepts have been mixed together in past code:

- runtime script references
- script bytecode versions
- concrete code cells on chain
- human-readable labels
- lock/type usage statistics

They are related, but they are not the same thing. The explorer should model them separately.

## Core Concepts

### Script Code

- The RISC-V binary stored in a code cell's data
- This is the actual bytecode executed by CKB-VM

### Script Version

- Identity: `H(script_code)`
- In practice this is the code cell's `data_hash`
- One script version means one exact byte-for-byte script code
- Different code means different version
- Same code in multiple cells is still the same version

Important:

- `version` is not a cell outpoint
- `version` is not a script label
- `version` is not the full script hash of `(code_hash, hash_type, args)`

### Code Cell Instance

- Identity: outpoint
- One concrete cell that stores a script version on chain
- A single version can have many code cell instances
- Any live code cell instance of the same version can be used as a valid dep when assembling a tx

### Runtime Script Reference

- Identity: `(reference_hash, hash_type)`
- This is how a lock script or type script locates code at verification time

Important:

- `hash_type=type` means "match dep cells by type script hash"
- it does **not** mean the code cell is automatically protected by Type ID
- Type ID is one possible type script pattern, not the definition of type reference

There are two reference families:

1. Data-hash family
   - `reference_hash = data_hash = H(script_code)`
   - `hash_type` is one of `data`, `data1`, `data2`
   - These all reference the code by its data hash
   - The different `hash_type` values also encode VM-version semantics

2. Type-hash family
   - `reference_hash = type_hash_of_code_cell`
   - `hash_type = type`
   - This references code via the code cell's type script hash

So for one concrete code cell:

- it always has a data-hash reference family
- it has a type-hash reference family only if the code cell itself has a type script

### Label Family

- Optional human-readable grouping
- A label may cover one or many script versions
- A version may have a readable label, or may remain unlabeled

Labels are product metadata, not chain truth.

## Two Different Axes That Must Not Be Mixed

### `hash_type`

- Meaning: how the VM locates code
- Values: `data`, `type`, `data1`, `data2`

### script role / usage kind

- Meaning: whether the script appears as a cell's `lock` or `type`
- Values: `lock`, `type`

These are independent.

Examples:

- A script may be used as a lock script while still referencing code by `type`
- A script may be used as a type script while referencing code by `data1`

Therefore:

- `hash_type` is correctness-critical for code resolution
- `lock/type` is a usage-view dimension
- UI `kind` is not a substitute for `hash_type`

## Multiplicity Rules

### Version to Code Cells

- One version can have many code cell instances
- All those code cell instances have the same script bytecode
- Therefore all those code cell instances share the same `version = H(script_code)`

### Reference to Version

For data-hash references:

- this is direct, because `reference_hash = version_hash`

For type-hash references:

- a reference may match multiple code cells
- this is because type reference matches by type script hash, not by Type ID specifically
- this is only safe if all matched code cells have identical code
- therefore the explorer should resolve the type reference to one version if all matched cells
  share the same `data_hash`
- if matched code cells have different `data_hash` values, the reference is ambiguous and should
  be treated as invalid/broken for correctness purposes

This is the general CKB rule for `type` reference. Multiple matches are possible in principle.

### Type ID and Uniqueness

Type ID is a special case, not the general rule.

- A Type ID type script guarantees there is only one **live** cell with that specific type script
  hash at a time
- `cell_deps` must be live cells
- therefore if a code cell's type script is backed by Type ID, a `type` reference will normally
  have at most one distinct live code cell match at the current chain state

However, this still does **not** make a type reference version-stable over time.

- the old code cell can be consumed
- a new live code cell can be created with the same Type ID type hash
- the new cell may contain different code
- the same `type` reference then resolves to a new current version

So:

- `data` / `data1` / `data2` references are version-stable
- `type` references are chain-state-dependent
- `Type ID` gives current live uniqueness, not eternal version immutability

This aligns with CKB's multiple-match semantics for identical code.

### Resolution Matrix

This is the shortest way to reason about reference behavior:

| Reference form                         | Current-state live matches                                                         | Version stability          | Explorer interpretation                                                 |
| -------------------------------------- | ---------------------------------------------------------------------------------- | -------------------------- | ----------------------------------------------------------------------- |
| `data` / `data1` / `data2`             | may match multiple dep cells, but they all represent the same code by construction | stable                     | treat as one `version`, then list its code cell instances               |
| `type` on a generic type script        | may match multiple live code cells                                                 | not stable                 | valid only if all matched live code cells resolve to the same `version` |
| `type` on a Type ID-backed type script | usually one live code cell at a time                                               | not stable across upgrades | treat as a unique current live match, but not as an eternal version id  |

Two important consequences:

- "currently unique" is not the same thing as "historically immutable"
- when analyzing a historical transaction, the canonical answer comes from that transaction's
  actual `cell_deps`, not from today's live-state lookup

### Historical Attribution

For explorer correctness, current-state resolution and historical attribution must stay separate.

- current-state resolution answers: "what does this bare reference resolve to now?"
- historical attribution answers: "what exact script code did this transaction execute then?"

For `data` / `data1` / `data2`, those answers line up naturally.

For `type`, they do not.

- a `type` reference may resolve to a different version after an upgrade
- therefore version-level historical usage must be attributed from each transaction's actual
  `cell_deps`
- it must not be recomputed later from today's live cells

This matters for indexing:

- reference-level stats can be aggregated by `(reference_hash, hash_type)`
- version-level stats must be aggregated by the exact `version = H(script_code)` resolved at the
  time the transaction is indexed

### Label to Versions

- One label family may point to many versions
- One version belongs to zero or one label family in ckbadger's current product model

The label relation must come from imported metadata. It should not be inferred from chain data.

## Recommended Explorer Model

Use these four entities:

1. `version`
   - identity: `version_hash = H(script_code)`

2. `reference`
   - identity: `(reference_hash, hash_type)`
   - data-hash references identify one version directly
   - type-hash references resolve to a version relative to the current chain state or the
     transaction's actual `cell_deps`

3. `code cell instance`
   - identity: outpoint
   - belongs to one `version`

4. `label family`
   - optional grouping over versions

Relationship summary:

```text
label family (optional)
        |
        v
    version = H(script_code)
      /   |    \
     /    |     \
reference reference code cell instance(s)
```

More precisely:

- `label family -> many versions`
- `version -> many code cell instances`
- `version <- many references`
- `type` reference resolution depends on which live code cells exist, unless uniqueness is
  guaranteed by the referenced type script pattern such as Type ID

## Route Semantics

### `/script/{hash}`

Treat `{hash}` as a reference-hash input, not automatically as a version id.

Backend behavior should be:

1. find all references whose `reference_hash == {hash}`
2. if the caller specified `hashType`, narrow to that exact reference
3. resolve the reference to a `version`
4. list code cell instances for that version

This route should not require frontend-provided `hashType` for correctness.
Query params may still be used as optional selectors or view hints.

For `type` references, this route is a "current chain state" view:

- if the referenced type script is Type ID-backed, the current live resolution is usually unique
- if not, multiple live code cell matches are possible
- if those matches disagree on code, the backend should surface ambiguity instead of guessing

Historical transactions are a separate problem. For a historical tx, the exact code cell resolution
must ultimately come from that transaction's actual `cell_deps`, not from today's live-state lookup.

### `/scripts/{name}`

Treat this as a label-family route.

Backend behavior should be:

1. load all versions that belong to that label family
2. render versions separately
3. for each version, show its code cell instances and usage stats

This route is label-driven, not reference-driven.

## What The Explorer Should Show

For a script version detail page:

- version identity (`version_hash`)
- available references
- live code cell instances
- total known code cell instances if useful
- optional label metadata
- usage stats split by lock/type role

It should not assume:

- one version has only one code cell instance
- one reference maps to only one code cell instance
- one label implies one version

## Naming Guidance

Avoid using `deployment` as a catch-all term.

Prefer:

- `version` for logical script bytecode identity
- `code cell instance` for one concrete outpoint
- `reference` for `(reference_hash, hash_type)`
- `label family` for optional human-readable grouping

If `deployment` is still used in some code or UI, it should mean a publication of a version on
chain, not the version itself.

## Common Modeling Mistakes

### Mistake 1: treating bare `code_hash` as the universal script identity

Wrong because:

- for `data` / `data1` / `data2`, `code_hash` is the version hash
- for `type`, `code_hash` is a reference hash, not the version hash
- for `type` with Type ID, it may uniquely identify the current live code cell, but it still does
  not become the immutable version identity

### Mistake 2: storing only one `hash_type` per `code_hash`

Wrong because:

- `hash_type` belongs to the reference identity
- the canonical key is `(reference_hash, hash_type)`

### Mistake 3: treating one code cell outpoint as the version

Wrong because:

- the same version may be deployed in multiple interchangeable code cells

### Mistake 4: relying on label import for code resolution correctness

Wrong because:

- labels are optional
- unlabeled versions must still be fully queryable and resolvable from chain-derived indexes

## Practical Implications For ckbadger

The backend should own correctness with this chain:

`reference -> version -> code cell instances`

With one clarification:

- for data-hash references, `reference -> version` is direct and immutable
- for type-hash references, `reference -> current live version` is a resolution step that depends
  on current live code cells unless the actual transaction `cell_deps` are known

The frontend should only own presentation state such as:

- which reference variant the user selected
- whether the user is viewing lock or type usage
- which code cell instance row is expanded

In particular:

- `hashType` is a backend correctness input, not a frontend guess
- `kind` is a usage-view hint only

## Related References

- [docs/rfcs/rfcs/0022-transaction-structure/0022-transaction-structure.md](./rfcs/rfcs/0022-transaction-structure/0022-transaction-structure.md)
- [docs/rfcs/rfcs/0029-allow-script-multiple-matches-on-identical-code/0029-allow-script-multiple-matches-on-identical-code.md](./rfcs/rfcs/0029-allow-script-multiple-matches-on-identical-code/0029-allow-script-multiple-matches-on-identical-code.md)
- [docs/rfcs/rfcs/0032-ckb-vm-version-selection/0032-ckb-vm-version-selection.md](./rfcs/rfcs/0032-ckb-vm-version-selection/0032-ckb-vm-version-selection.md)
- [docs/rfcs/rfcs/0051-ckb2023/0051-ckb2023.md](./rfcs/rfcs/0051-ckb2023/0051-ckb2023.md)
- [docs/docs.nervos.org/website/docs/script/type-id.mdx](./docs.nervos.org/website/docs/script/type-id.mdx)
- [docs/docs.nervos.org/website/docs/tech-explanation/data-type-diff.md](./docs.nervos.org/website/docs/tech-explanation/data-type-diff.md)
