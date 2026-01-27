## Task 1.1: Update docker-compose.yml (Completed)

### Changes Made

- Removed PostgreSQL service (lines 11-30 deleted)
- Removed postgres-data volume
- Updated indexer service:
  - depends_on: clickhouse (was: postgres)
  - CLICKHOUSE_URL, CLICKHOUSE_USER, CLICKHOUSE_PASSWORD, CLICKHOUSE_DATABASE env vars
- Updated api service:
  - depends_on: clickhouse + redis (was: postgres + redis)
  - Same CLICKHOUSE\_\* env vars

### Verification

- docker compose config validates successfully
- All services properly configured
- Health check dependencies maintained

### Pattern

Standard docker-compose service update: change depends_on + update environment variables
