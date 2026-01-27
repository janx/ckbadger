# Issues & Gotchas - ClickHouse Migration

## Known Issues from Research

1. **ClickHouse不支持UPDATE** - 使用Immutable数据模型解决
2. **live_cells表O(1)查找** - 需要特殊设计 (Phase 0验证)
3. **transaction_inputs缺少created_at_block** - JOIN可能昂贵
4. **UNNEST替代** - 使用ClickHouse arrayJoin

---

(Implementation-specific issues will be recorded here)

---

## 2026-01-27

- `cargo check -p ckbadger-indexer` fails after removing `Repository` from `Indexer`:
  - Multiple `self.repo` references remain in `sync/indexer.rs` (lines ~323, 348, 395, 596, 616, 853, 4611).
  - Follow-on E0277 errors for `Option<[u8]>` stem from the missing `repo.get_sync_tip()` return types.
