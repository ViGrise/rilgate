#!/usr/bin/env bash
# Performance regression gate for the gateway.
#
#   scripts/perf-gate.sh
#
# Reports throughput and checks successful responses, log volume, and challenge
# latency relative to health. Requires a release binary, wrk, Node.js, and curl.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BINARY="${PERF_GATE_BINARY:-./target/release/rilgate}"
RUNDIR="${PERF_GATE_RUNDIR:-/tmp/rilgate-perf-gate}"
PORT="${PERF_GATE_PORT:-8080}"
UPSTREAM_PORT="${PERF_GATE_UPSTREAM_PORT:-3300}"
CONNECTIONS="${PERF_GATE_CONNECTIONS:-32}"
THREADS="${PERF_GATE_THREADS:-2}"
DURATION="${PERF_GATE_DURATION:-10s}"
WARMUP="${PERF_GATE_WARMUP:-3s}"

MAX_LOG_BYTES="${PERF_GATE_MAX_LOG_BYTES:-65536}"
# Relative latency is less sensitive to machine speed than absolute throughput.
MAX_CHALLENGE_PCT="${PERF_GATE_MAX_CHALLENGE_PCT:-250}"

BROWSER_UA='Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 Chrome/120.0'

PIDS=()
cleanup() {
  local pid
  for pid in "${PIDS[@]:-}"; do
    kill "$pid" 2>/dev/null || true
  done
  wait 2>/dev/null || true
}
trap cleanup EXIT

require() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "perf-gate: $1 is required" >&2
    exit 2
  }
}
require wrk
require node
require curl

wait_for_http() {
  local url="$1" tries=200
  while ((tries-- > 0)); do
    if curl -fsS -o /dev/null "$url" 2>/dev/null; then return 0; fi
    sleep 0.1
  done
  echo "perf-gate: $url never became ready" >&2
  return 1
}

# p50 in microseconds, from a wrk --latency run.
measure_latency() {
  local path="$1" ua="$2" out p50
  wrk -t"$THREADS" -c"$CONNECTIONS" -d"$WARMUP" -H "User-Agent: $ua" \
    "http://127.0.0.1:$PORT$path" >/dev/null 2>&1
  out="$(wrk -t"$THREADS" -c"$CONNECTIONS" -d"$DURATION" --latency \
    -H "User-Agent: $ua" "http://127.0.0.1:$PORT$path")"
  if [[ "$out" == *"Non-2xx or 3xx responses:"* || "$out" == *"Socket errors:"* ]]; then
    echo "perf-gate: unsuccessful load run for $path" >&2
    echo "$out" >&2
    return 1
  fi
  if ! awk '/^Requests\/sec:/ { found = 1; if ($2 <= 0) exit 1 } END { if (!found) exit 1 }' <<<"$out"; then
    echo "perf-gate: no throughput for $path" >&2
    return 1
  fi
  p50="$(awk '$1=="50%" {print $2}' <<<"$out")"
  if [[ -z "$p50" ]]; then
    echo "perf-gate: missing latency for $path" >&2
    return 1
  fi
  awk -v rps="$(awk '/^Requests\/sec:/ {print $2}' <<<"$out")" -v p="$p50" \
    'BEGIN { unit = p; if (p ~ /ms$/) { sub(/ms$/, "", unit); unit = unit * 1000 } else if (p ~ /us$/) { sub(/us$/, "", unit) } else { sub(/s$/, "", unit); unit = unit * 1000000 } if (unit <= 0) exit 1; printf "%d %d", rps, unit }'
}

mkdir -p "$RUNDIR"
rm -f "$RUNDIR"/gateway.log

node scripts/perf/upstream.mjs "$UPSTREAM_PORT" >"$RUNDIR/upstream.log" 2>&1 &
PIDS+=($!)
wait_for_http "http://127.0.0.1:$UPSTREAM_PORT/"

cat >"$RUNDIR/rilgate.yml" <<EOF
serve:
  port: $PORT
upstream:
  url: "http://127.0.0.1:$UPSTREAM_PORT"
pow:
  signing_key: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  auth_signing_key: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  rate_limit:
    enabled: false
  bots:
    - name: protected
      path_regex: ^/protected$
      action: CHALLENGE
      challenge:
        algorithm: fast
        difficulty: 1
  thresholds: []
EOF

# The shipping default is what a deployment gets, so the gate measures it.
# PERF_GATE_RUST_LOG overrides the filter, which is how the log-volume assertion
# is exercised against a noisier configuration.
if [[ -n "${PERF_GATE_RUST_LOG+x}" ]]; then
  RUST_LOG="$PERF_GATE_RUST_LOG" "$BINARY" --config "$RUNDIR/rilgate.yml" \
    >"$RUNDIR/gateway.log" 2>&1 &
else
  env -u RUST_LOG "$BINARY" --config "$RUNDIR/rilgate.yml" \
    >"$RUNDIR/gateway.log" 2>&1 &
fi
PIDS+=($!)
wait_for_http "http://127.0.0.1:$PORT/.well-known/rilgate/api/health"

health_body="$(curl -fsS "http://127.0.0.1:$PORT/.well-known/rilgate/api/health")"
node --input-type=module -e 'if (JSON.parse(process.argv[1]).status !== "ok") process.exit(1)' "$health_body"
challenge_body="$(curl -fsS -H "User-Agent: $BROWSER_UA" "http://127.0.0.1:$PORT/protected")"
if [[ "$challenge_body" != *"window.POW_PRESENTATION_TOKEN = "* ]]; then
  echo "perf-gate: /protected did not return a challenge page" >&2
  exit 1
fi

log_bytes_before="$(stat -c%s "$RUNDIR/gateway.log" 2>/dev/null || stat -f%z "$RUNDIR/gateway.log" 2>/dev/null || echo 0)"

health_result="$(measure_latency /.well-known/rilgate/api/health rilgate-perf-gate/1.0)"
challenge_result="$(measure_latency /protected "$BROWSER_UA")"
read -r health_rps health_p50 <<<"$health_result"
read -r challenge_rps challenge_p50 <<<"$challenge_result"

log_bytes_after="$(stat -c%s "$RUNDIR/gateway.log" 2>/dev/null || stat -f%z "$RUNDIR/gateway.log" 2>/dev/null || echo 0)"
log_bytes=$((log_bytes_after - log_bytes_before))

printf 'perf-gate: conns=%s duration=%s\n' "$CONNECTIONS" "$DURATION"
printf '  %-22s %12s req/s  p50 %sus\n' "health" "$health_rps" "$health_p50"
printf '  %-22s %12s req/s  p50 %sus\n' "challenge page" "$challenge_rps" "$challenge_p50"
printf '  %-22s %12s bytes\n' "log written" "$log_bytes"

status=0

if ((log_bytes > MAX_LOG_BYTES)); then
  printf 'FAIL: log volume is %s bytes, limit %s\n' \
    "$log_bytes" "$MAX_LOG_BYTES" >&2
  status=1
fi

if ((health_p50 > 0)) && ((challenge_p50 * 100 > health_p50 * MAX_CHALLENGE_PCT)); then
  printf 'FAIL: challenge page p50 is %sus against health %sus, limit %s%%\n' \
    "$challenge_p50" "$health_p50" "$MAX_CHALLENGE_PCT" >&2
  status=1
fi

if ((status == 0)); then
  echo "perf-gate: ok"
fi
exit "$status"