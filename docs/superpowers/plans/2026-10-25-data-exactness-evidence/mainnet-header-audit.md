# Mainnet header exactness audit — 2026-10-25

Read-only RPC scan of `http://127.0.0.1:8114`; no explorer store reads or writes.

- Captured tip: 20,824,832, `0x690e39af417009a29e6f5ebb0a583bb614d6b566018d37170ad743959f479a4b`.
- Complete contiguous header range: 0–20,824,832; 20,824,833 headers.
- Each returned height, parent hash and final captured tip hash was verified.
- Raw timestamp backward steps: 0.
- Blocks retimed by prefix maximum: 0.
- Changed daily attribution: 0; changed hourly attribution: 0.
- Therefore all mainnet daily/hourly bucket memberships remain unchanged through this captured tip; the new semantics still require synthetic backward-time tests and testnet validation.
- Elapsed scan: 712.6 seconds, 29223.66 headers/s, up to eight concurrent bounded RPC batches, with 1,000 headers per batch.
- Testnet configuration points to `127.0.0.1:8124`, which refused connection. No testnet scan or service restart was performed.

Daily buckets in this report use UTC+8, matching the shared statistics definition.

Difficulty was independently decoded from every compact target using the CKB 0.119 target definition, summed as Python integers, and rounded half-up only for display. The tables in `header-audit.json` contain every date's exact sum, block count, interval sum, end height and DAO header. This is independent RPC evidence; it does not claim equality with a rebuilt explorer DB, which has not been created.

For 2459 completed dates, at least one displayed integer differs between the old **bulk** computation (truncated daily average converted to binary64, then multiplied back for hash rate) and the new integer computation. Existing live-path error also depended on batch boundaries and is not reconstructed here. The incomplete tip date 2026-10-25 is excluded below.

| Date       |    Old bulk average | Exact rounded average |       Old bulk H/s |  Exact rounded H/s |
| ---------- | ------------------: | --------------------: | -----------------: | -----------------: |
| 2024-09-15 | 4908326343293138944 |   4908326343293138624 | 366396502974289728 | 366396502974289662 |
| 2025-01-10 | 4719910383215094784 |   4719910383215094523 | 427505559641548288 | 427505559641548254 |
| 2024-09-12 | 4200927883697405952 |   4200927883697406207 | 341124033627121856 | 341124033627121898 |
| 2024-06-18 | 3793648565711504896 |   3793648565711505151 | 371371410736135040 | 371371410736135026 |
| 2024-06-17 | 4170151910389639680 |   4170151910389639425 | 368621767409311296 | 368621767409311294 |
| 2024-05-20 | 4102079892327578624 |   4102079892327578369 | 384808340809220416 | 384808340809220341 |
| 2025-08-09 | 2705977981327979008 |   2705977981327979262 | 257901878971112960 | 257901878971113009 |
| 2025-06-13 | 2878078111992046080 |   2878078111992045826 | 271078035515581248 | 271078035515581256 |
| 2025-01-06 | 3890623324436176384 |   3890623324436176130 | 439665682035651264 | 439665682035651299 |
| 2024-08-05 | 3361037618644869120 |   3361037618644868866 | 377154707984198016 | 377154707984197982 |

The independent script and raw daily evidence are local verification artifacts, not application implementation. Re-sync validation and explorer X3/X4 checks against the new DB remain separate acceptance steps.
