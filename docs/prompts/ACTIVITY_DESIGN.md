# Activity System Design

## Philosophy

Activities are **interpretations, not facts**. A simple form of activity is the interpretation of a per-owner position change in a single transaction: how much CKB moved, how much occupied capacity changed, which assets were affected, and who the counterparties were.

More sophisticated activity systems may interpret two owners' position changes in a single transaction as a single 'swap' activity rather than two separate activities. Since UTXO transactions are atomic action bundles involving multiple parties, the combination possibilities and thus possible interpretations are endless.
