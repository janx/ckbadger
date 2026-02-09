-- Migration: Drop unused indexes identified by pg_stat_user_indexes (0 scans)
-- These indexes were removed from 001_init.sql schema definition but existing
-- databases still have them. This migration cleans them up.
--
-- Background: idx_cells_lock_live, idx_cells_lock_script_details, and
-- idx_cells_type_live are redundant with idx_cells_list_covering and
-- idx_cells_type_code_hash_live. They showed 0 scans in pg_stat_user_indexes
-- and consume significant disk space across all partitions.

-- Drop parent-level indexes (automatically drops partition-level indexes)
DROP INDEX IF EXISTS idx_cells_lock_live;
DROP INDEX IF EXISTS idx_cells_lock_script_details;
DROP INDEX IF EXISTS idx_cells_type_live;

-- Also drop the unused transaction short hash index
DROP INDEX IF EXISTS idx_tx_short_hash;

-- In case partition-level indexes were created independently or the parent
-- indexes were already dropped but partition indexes remain, clean those up too.
DROP INDEX IF EXISTS cells_p00_lock_script_hash_status_idx;
DROP INDEX IF EXISTS cells_p01_lock_script_hash_status_idx;
DROP INDEX IF EXISTS cells_p02_lock_script_hash_status_idx;
DROP INDEX IF EXISTS cells_p03_lock_script_hash_status_idx;
DROP INDEX IF EXISTS cells_p04_lock_script_hash_status_idx;
DROP INDEX IF EXISTS cells_p05_lock_script_hash_status_idx;
DROP INDEX IF EXISTS cells_p06_lock_script_hash_status_idx;
DROP INDEX IF EXISTS cells_p07_lock_script_hash_status_idx;
DROP INDEX IF EXISTS cells_p08_lock_script_hash_status_idx;
DROP INDEX IF EXISTS cells_p09_lock_script_hash_status_idx;
