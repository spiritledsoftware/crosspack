#!/usr/bin/env bash
set -euo pipefail

threshold=3
window_seconds=900

if [ -n "${CROSSPACK_PREFIX:-}" ]; then
  prefix_path="$CROSSPACK_PREFIX"
elif [ -n "${LOCALAPPDATA:-}" ]; then
  # Match CLI default on Windows: LOCALAPPDATA/Crosspack
  prefix_path="${LOCALAPPDATA//\\//}/Crosspack"
else
  prefix_path="$HOME/.crosspack"
fi

log_path=""

usage() {
  cat <<'EOF'
Usage: scripts/check-snapshot-mismatch-health.sh [--prefix <path>] [--log <path>] [--threshold <n>] [--window-seconds <n>]

Checks recent snapshot-id mismatch telemetry and alerts when repeated failures are detected.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --prefix)
      prefix_path="$2"
      shift 2
      ;;
    --log)
      log_path="$2"
      shift 2
      ;;
    --threshold)
      threshold="$2"
      shift 2
      ;;
    --window-seconds)
      window_seconds="$2"
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      printf 'unknown argument: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

case "$threshold" in
  ''|*[!0-9]*)
    printf 'invalid --threshold value: %s\n' "$threshold" >&2
    exit 2
    ;;
esac

case "$window_seconds" in
  ''|*[!0-9]*)
    printf 'invalid --window-seconds value: %s\n' "$window_seconds" >&2
    exit 2
    ;;
esac

if [ -z "$log_path" ]; then
  log_path="${prefix_path}/state/transactions/snapshot-monitor.log"
fi

now_unix="$(date +%s)"
cutoff_unix=$((now_unix - window_seconds))
recent_count=0
total_count=0

if [ ! -f "$log_path" ]; then
  printf 'PASS [SM-001] snapshot mismatch monitor log not found at %s (no mismatches recorded)\n' "$log_path"
  printf 'summary: total=0 recent=0 threshold=%s window_seconds=%s\n' "$threshold" "$window_seconds"
  exit 0
fi

while IFS= read -r line; do
  case "$line" in
    *"error_code=snapshot-id-mismatch"*)
      total_count=$((total_count + 1))
      timestamp="$(printf '%s\n' "$line" | sed -n 's/.*timestamp_unix=\([0-9][0-9]*\).*/\1/p')"
      if [ -n "$timestamp" ] && [ "$timestamp" -ge "$cutoff_unix" ] && [ "$timestamp" -le "$now_unix" ]; then
        recent_count=$((recent_count + 1))
      fi
      ;;
  esac
done < "$log_path"

printf 'summary: total=%s recent=%s threshold=%s window_seconds=%s log=%s\n' \
  "$total_count" "$recent_count" "$threshold" "$window_seconds" "$log_path"

if [ "$recent_count" -ge "$threshold" ]; then
  printf 'CRIT [SM-101] ALERT repeated snapshot-id mismatch errors detected in the last %s seconds\n' "$window_seconds"
  printf '  action: run crosspack update, verify source snapshot consistency, and open launch blocker review if unresolved\n'
  exit 1
fi

if [ "$recent_count" -gt 0 ]; then
  printf 'WARN [SM-102] snapshot-id mismatch errors observed but below alert threshold\n'
  printf '  action: watch for repeats and re-run this check before launch decisions\n'
  exit 0
fi

printf 'PASS [SM-103] no recent snapshot-id mismatch errors\n'
