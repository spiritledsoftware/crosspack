#!/usr/bin/env bash
set -euo pipefail

crit_failures=0
warn_count=0
pass_count=0

if command -v cargo >/dev/null 2>&1; then
  CARGO_CMD=(cargo)
elif command -v rustup >/dev/null 2>&1; then
  CARGO_CMD=(rustup run stable cargo)
else
  CARGO_CMD=()
fi

status_line() {
  local level="$1"
  local check_id="$2"
  local message="$3"
  local hint="$4"
  printf '%s [%s] %s\n' "$level" "$check_id" "$message"
  if [ -n "$hint" ]; then
    printf '  hint: %s\n' "$hint"
  fi
}

record_pass() {
  pass_count=$((pass_count + 1))
  status_line "PASS" "$1" "$2" "$3"
}

record_warn() {
  warn_count=$((warn_count + 1))
  status_line "WARN" "$1" "$2" "$3"
}

record_crit() {
  crit_failures=$((crit_failures + 1))
  status_line "CRIT" "$1" "$2" "$3"
}

run_crit_check() {
  local check_id="$1"
  local description="$2"
  local hint="$3"
  shift 3

  if [ ${#CARGO_CMD[@]} -eq 0 ]; then
    record_crit "$check_id" "$description" "cargo toolchain unavailable. Install Rust or ensure rustup is on PATH."
    return
  fi

  if "${CARGO_CMD[@]}" "$@"; then
    record_pass "$check_id" "$description" ""
  else
    record_crit "$check_id" "$description" "$hint"
  fi
}

printf 'snapshot-flow validation started\n'

if command -v rg >/dev/null 2>&1; then
  record_pass "SF-000" "ripgrep is available for fast contract checks" ""
else
  record_warn "SF-000" "ripgrep not found; static contract checks will use grep and run slower" "install ripgrep for faster local validation"
fi

run_crit_check \
  "SF-101" \
  "mixed snapshot IDs are rejected" \
  "run ${CARGO_CMD[*]} test -p crosspack-cli resolve_transaction_snapshot_id_rejects_mixed_ready_snapshots -- --test-threads=1 and inspect failure details" \
  test -p crosspack-cli resolve_transaction_snapshot_id_rejects_mixed_ready_snapshots -- --test-threads=1

run_crit_check \
  "SF-102" \
  "shared snapshot ID is accepted" \
  "run ${CARGO_CMD[*]} test -p crosspack-cli resolve_transaction_snapshot_id_uses_shared_snapshot_id -- --test-threads=1 and confirm fixture snapshots match" \
  test -p crosspack-cli resolve_transaction_snapshot_id_uses_shared_snapshot_id -- --test-threads=1

run_crit_check \
  "SF-103" \
  "registry index fails cleanly when no ready snapshot exists" \
  "run ${CARGO_CMD[*]} test -p crosspack-registry configured_index_fails_when_no_ready_snapshot_exists -- --test-threads=1 and inspect source readiness fixtures" \
  test -p crosspack-registry configured_index_fails_when_no_ready_snapshot_exists -- --test-threads=1

if command -v rg >/dev/null 2>&1; then
  if rg -n --fixed-strings 'metadata snapshot mismatch across configured sources' crates/crosspack-cli/src >/dev/null && \
     rg -n --fixed-strings 'assert!(rendered.contains("metadata snapshot mismatch across configured sources"));' crates/crosspack-cli/src >/dev/null; then
    record_pass "SF-201" "snapshot mismatch error text contract is enforced in tests" ""
  else
    record_crit "SF-201" "snapshot mismatch error text contract is missing" "restore the mismatch message assertion in crosspack-cli snapshot tests"
  fi
else
  if grep -R -F 'metadata snapshot mismatch across configured sources' crates/crosspack-cli/src >/dev/null && \
     grep -R -F 'assert!(rendered.contains("metadata snapshot mismatch across configured sources"));' crates/crosspack-cli/src >/dev/null; then
    record_pass "SF-201" "snapshot mismatch error text contract is enforced in tests" ""
  else
    record_crit "SF-201" "snapshot mismatch error text contract is missing" "restore the mismatch message assertion in crosspack-cli snapshot tests"
  fi
fi

if [ -x scripts/check-snapshot-mismatch-health.sh ]; then
  health_output="$(scripts/check-snapshot-mismatch-health.sh 2>&1)" || {
    record_crit "SF-301" "snapshot mismatch health check alerted repeated failures" "$health_output"
    health_output=""
  }
  if [ -n "${health_output:-}" ]; then
    if printf '%s\n' "$health_output" | grep -q '^WARN \[SM-102\]'; then
      record_warn "SF-301" "snapshot mismatch health check observed recent mismatches" "$(printf '%s\n' "$health_output" | tail -n 2 | tr '\n' ' ')"
    else
      record_pass "SF-301" "snapshot mismatch health check did not alert repeated failures" ""
    fi
  fi
else
  record_crit "SF-301" "snapshot mismatch health check script missing or not executable" "restore scripts/check-snapshot-mismatch-health.sh and executable bit"
fi

printf '\nsummary: pass=%s warn=%s crit=%s\n' "$pass_count" "$warn_count" "$crit_failures"

if [ "$crit_failures" -gt 0 ]; then
  printf 'result: CRIT - snapshot flow validation failed. Fix CRIT checks and rerun scripts/validate-snapshot-flow.sh\n'
  exit 1
fi

if [ "$warn_count" -gt 0 ]; then
  printf 'result: WARN - snapshot flow validation passed with warnings. Review hints above.\n'
else
  printf 'result: PASS - snapshot flow validation is healthy.\n'
fi
