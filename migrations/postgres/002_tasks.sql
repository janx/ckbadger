-- ============================================
-- Task System Migration
-- Background task management for ckbadger
-- ============================================

-- ===========================================
-- 1. Tasks Table
-- ===========================================

CREATE TABLE tasks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_type VARCHAR(50) NOT NULL,  -- 'cycles_backfill', 'index_rebuild', 'label_import'
    status VARCHAR(20) NOT NULL DEFAULT 'pending',  -- 'pending', 'running', 'completed', 'failed', 'cancelled', 'paused'
    priority INTEGER DEFAULT 0,
    
    -- Configuration (task-specific JSON)
    config JSONB NOT NULL DEFAULT '{}',
    
    -- Progress tracking
    progress_total BIGINT DEFAULT 0,
    progress_current BIGINT DEFAULT 0,
    progress_message TEXT,
    
    -- Result/error
    result JSONB,
    error_message TEXT,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    heartbeat_at TIMESTAMPTZ,
    
    -- Runtime metadata
    runner_id VARCHAR(100),  -- Identifies which runner instance is executing
    retry_count INTEGER DEFAULT 0,
    max_retries INTEGER DEFAULT 3,
    
    -- Rate tracking for ETA calculation
    rate_samples JSONB DEFAULT '[]',  -- Recent samples: [{ts: epoch, v: progress_current}, ...]
    rate_ema DOUBLE PRECISION,  -- Exponential moving average (items/sec)
    
    -- Log tail for TUI display
    log_tail TEXT  -- Last N lines of log output
);

-- Indexes for task queries
CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_tasks_type_status ON tasks(task_type, status);
CREATE INDEX idx_tasks_created_at ON tasks(created_at DESC);
CREATE INDEX idx_tasks_runner ON tasks(runner_id) WHERE runner_id IS NOT NULL;

-- Comments for documentation
COMMENT ON TABLE tasks IS 'Background task management for cycles backfill, index rebuild, and label import';
COMMENT ON COLUMN tasks.task_type IS 'cycles_backfill | index_rebuild | label_import';
COMMENT ON COLUMN tasks.status IS 'pending | running | completed | failed | cancelled | paused';
COMMENT ON COLUMN tasks.config IS 'Task-specific configuration JSON';
COMMENT ON COLUMN tasks.result IS 'Task result/progress details JSON (e.g., index rebuild progress)';
COMMENT ON COLUMN tasks.rate_ema IS 'Exponential moving average rate for ETA calculation';

-- ===========================================
-- 2. Clean up sync_status table
-- Remove integrity/label/index tracking fields
-- ===========================================

ALTER TABLE sync_status
    DROP COLUMN IF EXISTS integrity_heartbeat,
    DROP COLUMN IF EXISTS integrity_pending_count,
    DROP COLUMN IF EXISTS integrity_total_count,
    DROP COLUMN IF EXISTS integrity_processed_count,
    DROP COLUMN IF EXISTS integrity_started_at,
    DROP COLUMN IF EXISTS udt_info_running,
    DROP COLUMN IF EXISTS udt_info_total_count,
    DROP COLUMN IF EXISTS udt_info_processed_count,
    DROP COLUMN IF EXISTS udt_info_started_at,
    DROP COLUMN IF EXISTS udt_info_last_check_at,
    DROP COLUMN IF EXISTS script_info_running,
    DROP COLUMN IF EXISTS script_info_total_count,
    DROP COLUMN IF EXISTS script_info_processed_count,
    DROP COLUMN IF EXISTS script_info_started_at,
    DROP COLUMN IF EXISTS script_info_last_check_at,
    DROP COLUMN IF EXISTS indexes_deferred,
    DROP COLUMN IF EXISTS indexes_dropped_at,
    DROP COLUMN IF EXISTS indexes_rebuild_started_at,
    DROP COLUMN IF EXISTS indexes_rebuild_completed_at,
    DROP COLUMN IF EXISTS indexes_rebuild_progress;

-- ===========================================
-- 3. Drop integrity_recent_fixes table
-- ===========================================

DROP TABLE IF EXISTS integrity_recent_fixes;
