#!/bin/bash
set -e

echo "Initializing ClickHouse database..."

# Wait for ClickHouse to be ready
until clickhouse-client --query "SELECT 1" >/dev/null 2>&1; do
  echo "Waiting for ClickHouse to start..."
  sleep 1
done

echo "Running migrations..."

# Run migrations in order
clickhouse-client --multiquery < /docker-entrypoint-initdb.d/001_core_tables.sql
echo "✓ Core tables created"

clickhouse-client --multiquery < /docker-entrypoint-initdb.d/002_live_cells.sql
echo "✓ Live cells tables created"

clickhouse-client --multiquery < /docker-entrypoint-initdb.d/003_assets.sql
echo "✓ Assets tables created"

clickhouse-client --multiquery < /docker-entrypoint-initdb.d/004_statistics.sql
echo "✓ Statistics tables created"

echo "ClickHouse initialization complete!"
