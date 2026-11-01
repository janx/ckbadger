# Information Design

## Three Information Layers

ckbadger information is organized into three layers, from low to high:

1. Raw Data (Syntax Representations, Facts)
2. Domain Knowledge (Semantics, Opinions)
3. Aggregations and Intelligence

### Layer 0: Raw Data (Syntax Representations)

This layer contains on-chain primitive objects:

- blocks
- transactions
- cells
- scripts
- hardforks

Raw data is the factual foundation, because they come from and are verified by CKB nodes, however it should not be the default entry point for most users.

The append-only store should record all happened histories.

### Layer 1: Domain Knowledge (Semantics)

This layer contains user-facing semantic concepts:

- addresses
- assets
- asset standards
- activities
- Nervos DAO
- canonical chain and reorgs

Domain knowledge are like opinions about fact, for example, the activities of a transaction can be interpreted as either two payments or a swap, different nodes may see different canonical chain tip and reorgs. This is the primary focus of information presentation and navigation.

Reorgs could change opinions but don't change facts. The domain store should keep latest opinions.

### Layer 2: Aggregations and Intelligence

This layer contains higher-order semantics derived from syntax + semantics through broad synthesis and deep analysis, including:

- statistics
- user identity signals
- asset flow
- user intent inference
- historical analysis
- other advanced analytical conclusions

This layer has high value, but reliable automation is hard. Its display priority is medium.

The domain store should keep stats and intelligence.

## Information Display Priority

1. Highlight Domain Knowledge (Layer 1) as the default view and main navigation model.
2. Present Aggregations and Intelligence (Layer 2) as supportive, value-added context.
3. Keep Raw Data (Layer 0) always reachable for verification and deep investigation.

## UI/UX Design Principles

### 1) Intra-Layer Connectivity

Concepts within the same layer should be cross-linked to support continuous exploration.

Examples:

- address <-> activities
- asset <-> activities
- transaction <-> cells

### 2) Cross-Layer Traceability

Higher-layer concepts must link to their immediate lower-layer evidence so users can verify interpretation with concrete data.

Examples:

- statistics -> related assets/addresses -> source transactions/cells
- user identity signal -> activity evidence -> raw scripts/cells

### 3) Domain-First Interaction

The default UX should start from domain knowledge, while each important node should support:

- jumping upward to aggregations and intelligence
- drilling downward to raw data

## Interaction Goals

- Help users understand semantics first, then validate details.
- Ensure every high-level conclusion can be traced back to verifiable on-chain facts.
- Keep exploration in a closed loop: raw data -> domain knowledge -> intelligence -> raw data.

## Implementation Checklist

- Every core page includes at least one intra-layer navigation link.
- Every high-level card or metric includes a source/evidence entry to lower layers.
- Every domain page includes a clear raw-data drill-down path.
- Aggregation pages clearly state analysis scope and source object boundaries.
