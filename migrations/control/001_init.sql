-- ============================================
-- CKBadger Control Plane Database Schema
-- Manages multiple database instances, sync jobs, and system configuration
-- ============================================

-- ===========================================
-- 1. Database Instances
-- ===========================================

-- Database instance management
CREATE TABLE instances (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(100) NOT NULL UNIQUE,
    
    -- Connection
    database_url TEXT NOT NULL,
    ckb_rpc_url TEXT NOT NULL,
    network VARCHAR(20) NOT NULL DEFAULT 'mainnet',
    
    -- Status
    status VARCHAR(20) NOT NULL DEFAULT 'created',
    -- created: just created, no sync started
    -- syncing: core sync in progress
    -- rebuilding: rebuild phase in progress
    -- ready: fully synced and rebuilt, can be activated
    -- active: currently serving API requests
    -- failed: sync or rebuild failed
    -- archived: no longer in use
    
    -- Sync Phase (detailed phase within syncing/rebuilding)
    sync_phase VARCHAR(30) NOT NULL DEFAULT 'pending',
    -- pending: waiting to start
    -- core_sync: syncing core blockchain data
    -- rebuild_live_cells: rebuilding live_cells table
    -- rebuild_balances: rebuilding address_balances
    -- rebuild_script_usage: rebuilding script_usage_stats
    -- rebuild_statistics: rebuilding daily/hourly/epoch stats
    -- rebuild_indexes: creating indexes
    -- rebuild_address_tx: background address_transactions fill
    -- completed: all phases done
    
    -- Progress
    current_block BIGINT NOT NULL DEFAULT 0,
    target_block BIGINT,
    sync_speed FLOAT,  -- blocks/sec (rolling average)
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    sync_started_at TIMESTAMPTZ,
    sync_completed_at TIMESTAMPTZ,
    rebuild_started_at TIMESTAMPTZ,
    rebuild_completed_at TIMESTAMPTZ,
    last_activity_at TIMESTAMPTZ DEFAULT NOW(),
    
    -- Configuration (JSON for flexibility)
    config JSONB NOT NULL DEFAULT '{
        "batch_size": 3000,
        "parallel_fetch_size": 64,
        "pipeline_buffer": 4,
        "bulk_sync_mode": true,
        "skip_live_cells": true,
        "skip_address_transactions": true,
        "skip_statistics": true
    }'::jsonb,
    
    -- Metrics
    metrics JSONB DEFAULT '{}'::jsonb,
    -- Example: {"total_blocks": 15000000, "total_transactions": 50000000, 
    --           "total_cells": 200000000, "database_size_bytes": 50000000000}
    
    -- Error tracking
    last_error TEXT,
    last_error_at TIMESTAMPTZ,
    error_count INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_instances_status ON instances(status);
CREATE INDEX idx_instances_name ON instances(name);

-- ===========================================
-- 2. Active Instance (API routing)
-- ===========================================

-- Tracks which instance the API should use
CREATE TABLE active_instance (
    id INTEGER PRIMARY KEY DEFAULT 1,
    instance_id UUID REFERENCES instances(id) ON DELETE SET NULL,
    switched_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    switched_by VARCHAR(100),  -- username or 'system'
    previous_instance_id UUID,
    switch_reason TEXT,
    
    CONSTRAINT single_row CHECK (id = 1)
);

-- Initialize with NULL (no active instance)
INSERT INTO active_instance (id, instance_id) VALUES (1, NULL);

-- ===========================================
-- 3. Sync Jobs
-- ===========================================

-- Individual sync/rebuild jobs
CREATE TABLE sync_jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id UUID NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    
    -- Job type
    job_type VARCHAR(30) NOT NULL,
    -- core_sync: main blockchain sync
    -- rebuild_live_cells: rebuild live_cells table
    -- rebuild_balances: rebuild address_balances
    -- rebuild_script_usage: rebuild script_usage_stats
    -- rebuild_daily_stats: rebuild daily_statistics
    -- rebuild_hourly_stats: rebuild hourly_statistics
    -- rebuild_epoch_stats: rebuild epoch_statistics
    -- rebuild_miner_stats: rebuild miner_statistics
    -- rebuild_dao_snapshots: rebuild dao_daily_snapshots
    -- rebuild_indexes: create database indexes
    -- rebuild_address_tx: background address_transactions fill
    
    -- Status
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    -- pending: waiting to start
    -- queued: in queue, will start soon
    -- running: currently executing
    -- paused: manually paused
    -- completed: successfully finished
    -- failed: error occurred
    -- cancelled: manually cancelled
    
    -- Progress
    progress_current BIGINT NOT NULL DEFAULT 0,
    progress_total BIGINT,
    progress_percent FLOAT GENERATED ALWAYS AS (
        CASE WHEN progress_total > 0 AND progress_total IS NOT NULL
        THEN LEAST(100.0, (progress_current::float / progress_total * 100))
        ELSE 0 END
    ) STORED,
    
    -- Performance
    rows_processed BIGINT NOT NULL DEFAULT 0,
    rows_per_second FLOAT,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    
    -- Error handling
    error_message TEXT,
    error_details JSONB,
    retry_count INTEGER NOT NULL DEFAULT 0,
    max_retries INTEGER NOT NULL DEFAULT 3,
    
    -- Execution details
    worker_id VARCHAR(100),  -- identifier of the worker process
    checkpoint JSONB,  -- for resumable jobs
    -- Example: {"last_block": 1000000, "last_partition": 2}
    
    -- Metrics
    metrics JSONB DEFAULT '{}'::jsonb
    -- Example: {"elapsed_ms": 12345, "memory_mb": 512}
);

CREATE INDEX idx_sync_jobs_instance ON sync_jobs(instance_id);
CREATE INDEX idx_sync_jobs_status ON sync_jobs(status) WHERE status IN ('pending', 'queued', 'running');
CREATE INDEX idx_sync_jobs_type ON sync_jobs(job_type, status);

-- ===========================================
-- 4. Sync Events (Audit Log)
-- ===========================================

-- Event log for debugging and monitoring
CREATE TABLE sync_events (
    id BIGSERIAL PRIMARY KEY,
    instance_id UUID REFERENCES instances(id) ON DELETE CASCADE,
    job_id UUID REFERENCES sync_jobs(id) ON DELETE CASCADE,
    
    -- Event classification
    event_type VARCHAR(30) NOT NULL,
    -- instance_created, instance_deleted, instance_activated
    -- sync_started, sync_paused, sync_resumed, sync_completed, sync_failed
    -- rebuild_started, rebuild_completed, rebuild_failed
    -- phase_changed, checkpoint_saved
    -- error, warning, info
    
    severity VARCHAR(10) NOT NULL DEFAULT 'info',
    -- debug, info, warning, error, critical
    
    -- Event data
    message TEXT NOT NULL,
    details JSONB,
    
    -- Context
    source VARCHAR(50),  -- 'indexer', 'tui', 'api', 'system'
    
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_sync_events_instance ON sync_events(instance_id, created_at DESC);
CREATE INDEX idx_sync_events_job ON sync_events(job_id, created_at DESC) WHERE job_id IS NOT NULL;
CREATE INDEX idx_sync_events_type ON sync_events(event_type, created_at DESC);
CREATE INDEX idx_sync_events_severity ON sync_events(severity, created_at DESC) 
    WHERE severity IN ('warning', 'error', 'critical');

-- Cleanup old events (keep 30 days by default)
-- This would be run by a maintenance job

-- ===========================================
-- 5. System Configuration
-- ===========================================

-- Global system settings
CREATE TABLE system_config (
    key VARCHAR(100) PRIMARY KEY,
    value JSONB NOT NULL,
    description TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_by VARCHAR(100)
);

-- Default configuration
INSERT INTO system_config (key, value, description) VALUES
('default_sync_config', '{
    "batch_size": 3000,
    "parallel_fetch_size": 64,
    "pipeline_buffer": 4,
    "bulk_sync_mode": true,
    "skip_live_cells": true,
    "skip_address_transactions": true,
    "skip_statistics": true
}'::jsonb, 'Default configuration for new sync instances'),

('rebuild_config', '{
    "parallel_partitions": 10,
    "address_tx_background": true,
    "index_concurrently": true
}'::jsonb, 'Configuration for rebuild phase'),

('retention_days', '30'::jsonb, 'Days to keep sync events before cleanup'),

('api_config', '{
    "health_check_interval_ms": 5000,
    "switchover_delay_ms": 1000
}'::jsonb, 'API service configuration');

-- ===========================================
-- 6. Database Snapshots (for backup/restore)
-- ===========================================

-- Track database snapshots for quick restore
CREATE TABLE snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id UUID REFERENCES instances(id) ON DELETE SET NULL,
    
    -- Snapshot info
    name VARCHAR(200) NOT NULL,
    block_number BIGINT NOT NULL,
    block_hash BYTEA,
    
    -- Storage
    storage_type VARCHAR(20) NOT NULL DEFAULT 'local',
    -- local, s3, gcs, url
    storage_path TEXT NOT NULL,
    size_bytes BIGINT,
    
    -- Status
    status VARCHAR(20) NOT NULL DEFAULT 'creating',
    -- creating, ready, uploading, available, deleted, failed
    
    -- Metadata
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,  -- NULL = never expires
    
    -- Checksums
    checksum_sha256 VARCHAR(64),
    
    -- Notes
    description TEXT,
    created_by VARCHAR(100)
);

CREATE INDEX idx_snapshots_instance ON snapshots(instance_id);
CREATE INDEX idx_snapshots_status ON snapshots(status) WHERE status = 'available';
CREATE INDEX idx_snapshots_block ON snapshots(block_number DESC);

-- ===========================================
-- 7. Helper Functions
-- ===========================================

-- Function to get current active instance
CREATE OR REPLACE FUNCTION get_active_instance()
RETURNS UUID AS $$
    SELECT instance_id FROM active_instance WHERE id = 1;
$$ LANGUAGE SQL STABLE;

-- Function to set active instance with audit
CREATE OR REPLACE FUNCTION set_active_instance(
    p_instance_id UUID,
    p_switched_by VARCHAR(100) DEFAULT 'system',
    p_reason TEXT DEFAULT NULL
) RETURNS VOID AS $$
DECLARE
    v_previous_id UUID;
    v_instance_status VARCHAR(20);
BEGIN
    -- Get current active instance
    SELECT instance_id INTO v_previous_id FROM active_instance WHERE id = 1;
    
    -- Verify new instance is ready
    IF p_instance_id IS NOT NULL THEN
        SELECT status INTO v_instance_status FROM instances WHERE id = p_instance_id;
        IF v_instance_status IS NULL THEN
            RAISE EXCEPTION 'Instance % not found', p_instance_id;
        END IF;
        IF v_instance_status NOT IN ('ready', 'active') THEN
            RAISE EXCEPTION 'Instance % is not ready (status: %)', p_instance_id, v_instance_status;
        END IF;
    END IF;
    
    -- Update previous instance status
    IF v_previous_id IS NOT NULL AND v_previous_id != p_instance_id THEN
        UPDATE instances SET status = 'ready' WHERE id = v_previous_id AND status = 'active';
    END IF;
    
    -- Update new instance status
    IF p_instance_id IS NOT NULL THEN
        UPDATE instances SET status = 'active' WHERE id = p_instance_id;
    END IF;
    
    -- Update active_instance
    UPDATE active_instance SET
        instance_id = p_instance_id,
        switched_at = NOW(),
        switched_by = p_switched_by,
        previous_instance_id = v_previous_id,
        switch_reason = p_reason
    WHERE id = 1;
    
    -- Log event
    INSERT INTO sync_events (instance_id, event_type, severity, message, details, source)
    VALUES (
        p_instance_id,
        'instance_activated',
        'info',
        CASE 
            WHEN p_instance_id IS NULL THEN 'Active instance cleared'
            ELSE 'Instance activated for API'
        END,
        jsonb_build_object(
            'previous_instance_id', v_previous_id,
            'new_instance_id', p_instance_id,
            'reason', p_reason
        ),
        'system'
    );
END;
$$ LANGUAGE plpgsql;

-- Function to update instance progress
CREATE OR REPLACE FUNCTION update_instance_progress(
    p_instance_id UUID,
    p_current_block BIGINT,
    p_target_block BIGINT DEFAULT NULL,
    p_sync_speed FLOAT DEFAULT NULL
) RETURNS VOID AS $$
BEGIN
    UPDATE instances SET
        current_block = p_current_block,
        target_block = COALESCE(p_target_block, target_block),
        sync_speed = COALESCE(p_sync_speed, sync_speed),
        last_activity_at = NOW()
    WHERE id = p_instance_id;
END;
$$ LANGUAGE plpgsql;

-- Function to transition instance phase
CREATE OR REPLACE FUNCTION transition_instance_phase(
    p_instance_id UUID,
    p_new_phase VARCHAR(30),
    p_new_status VARCHAR(20) DEFAULT NULL
) RETURNS VOID AS $$
DECLARE
    v_old_phase VARCHAR(30);
    v_old_status VARCHAR(20);
BEGIN
    SELECT sync_phase, status INTO v_old_phase, v_old_status 
    FROM instances WHERE id = p_instance_id;
    
    UPDATE instances SET
        sync_phase = p_new_phase,
        status = COALESCE(p_new_status, status),
        last_activity_at = NOW(),
        -- Set timestamps based on phase
        sync_started_at = CASE WHEN p_new_phase = 'core_sync' AND sync_started_at IS NULL THEN NOW() ELSE sync_started_at END,
        sync_completed_at = CASE WHEN p_new_phase LIKE 'rebuild_%' AND sync_completed_at IS NULL THEN NOW() ELSE sync_completed_at END,
        rebuild_started_at = CASE WHEN p_new_phase = 'rebuild_live_cells' AND rebuild_started_at IS NULL THEN NOW() ELSE rebuild_started_at END,
        rebuild_completed_at = CASE WHEN p_new_phase = 'completed' THEN NOW() ELSE rebuild_completed_at END
    WHERE id = p_instance_id;
    
    -- Log phase transition
    INSERT INTO sync_events (instance_id, event_type, severity, message, details, source)
    VALUES (
        p_instance_id,
        'phase_changed',
        'info',
        format('Phase changed: %s -> %s', v_old_phase, p_new_phase),
        jsonb_build_object(
            'old_phase', v_old_phase,
            'new_phase', p_new_phase,
            'old_status', v_old_status,
            'new_status', COALESCE(p_new_status, v_old_status)
        ),
        'system'
    );
END;
$$ LANGUAGE plpgsql;
