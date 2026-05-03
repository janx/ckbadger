#!/usr/bin/env bash
#
# Ad-hoc snapshot of live-sync health from the indexer log.
#
# Usage:
#   scripts/perf_check.sh                 # uses temp/run/logs/indexer.log
#   scripts/perf_check.sh /path/to/idx.log
#
# Reports:
#   - active run_id and time-since-restart
#   - parser cell-info timeouts since restart
#   - slow chunks since restart
#   - last 10 db_stage_write_ms values (write trend)
#   - block_cache_mb trajectory (last 10 samples)
#   - latest sync tip vs chain tip
#
# Designed to be safe to run repeatedly during the 24-48h observation
# window after a fix lands.

set -uo pipefail

LOG="${1:-temp/run/logs/indexer.log}"

if [[ ! -f "$LOG" ]]; then
  echo "indexer log not found: $LOG" >&2
  exit 1
fi

strip_ansi() { sed 's/\x1b\[[0-9;]*m//g'; }

echo "=== ckbadger live-sync perf snapshot ==="
echo "log: $LOG"
echo

# 1. Active run_id (last one mentioned). Tracing emits ANSI escape codes
# around field names, so strip them before regex matching.
RUN_ID=$(strip_ansi < "$LOG" | grep -aoE 'run_id=run-[0-9TZ\.\-]+pid[0-9]+' | tail -1 | sed 's/run_id=//')
if [[ -z "$RUN_ID" ]]; then
  echo "no run_id found — log may be from a pre-instrumentation build"
  exit 0
fi
echo "active run: $RUN_ID"

# Restart timestamp (UTC) parsed from run_id.
RUN_TS=$(echo "$RUN_ID" | sed -E 's/run-([0-9]{4})([0-9]{2})([0-9]{2})T([0-9]{2})([0-9]{2})([0-9]{2})\.[0-9]+Z.*/\1-\2-\3T\4:\5:\6Z/')
echo "restart at:  $RUN_TS"
echo

# Pre-strip ANSI to a temp file so subsequent awk passes can match
# field names (they're wrapped in escape codes by the tracing layer).
STRIPPED=$(mktemp)
trap 'rm -f "$STRIPPED"' EXIT
strip_ansi < "$LOG" > "$STRIPPED"

# 2. Timeouts since restart.
TIMEOUTS=$(awk -v r="$RUN_ID" '
  $0 ~ r {found=1}
  found && /DB query for cell info timed out/ {c++}
  END {print c+0}' "$STRIPPED")
echo "parser cell-info timeouts (since restart): $TIMEOUTS"

# 3. Slow chunks since restart.
SLOW=$(awk -v r="$RUN_ID" '
  $0 ~ r {found=1}
  found && /parser cell read chunk slow/ {c++}
  END {print c+0}' "$STRIPPED")
echo "parser cell read chunk slow (since restart): $SLOW"

# 4. Live-sync health monitor degradation warnings.
DEG=$(awk -v r="$RUN_ID" '
  $0 ~ r {found=1}
  found && /live-sync health: degradation detected/ {c++}
  END {print c+0}' "$STRIPPED")
echo "health monitor degradation warnings: $DEG"
echo

# 5. db_stage_write_ms last 10 samples.
echo "db_stage_write_ms (last 10 'Synced to block' lines):"
grep -aE '\[DB\] Synced to block' "$STRIPPED" \
  | tail -10 \
  | grep -oE 'db_stage_write_ms=Some\("[0-9.]+' \
  | sed 's/db_stage_write_ms=Some("//' \
  | awk '{printf "  %s\n", $0}'
echo

# 6. block_cache_mb trajectory.
echo "block_cache_mb (last 10 samples):"
grep -aE 'RocksDB stats' "$STRIPPED" \
  | tail -10 \
  | grep -oE 'block_cache_mb=[0-9]+' \
  | sed 's/block_cache_mb=/  /'
echo

# 7. Tip status.
TIP_LINE=$(grep -aE '\[DB\] Synced to block' "$STRIPPED" | tail -1)
echo "latest tip line:"
echo "  $TIP_LINE"
echo

# 8. CSV summary if available.
CSV="${CKBADGER_HEALTH_CSV:-temp/perf/live-sync-health.csv}"
if [[ -f "$CSV" ]]; then
  ROWS=$(wc -l < "$CSV")
  echo "live-sync-health.csv rows (incl header): $ROWS"
  if (( ROWS > 1 )); then
    echo "last 5 hourly rows:"
    head -1 "$CSV"
    tail -5 "$CSV"
    echo
    # Tight summary of write-side trend (added in PR-2 step A).
    echo "write-side trend (db_stage_avg, db_commit_avg, wbm_budget, flush_observed_in_window):"
    awk -F',' 'NR>1 {printf "  %s  stage=%s commit=%s wbm_budget_mb=%s flush_minutes=%s\n", $1, $4, $5, $18, $19}' "$CSV" | tail -10
  fi
fi
