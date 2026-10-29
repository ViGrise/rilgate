#!/usr/bin/env bash
# Performance regression gate for the gateway.
#
#   scripts/perf-gate.sh
#
# Reports health, challenge, and proxy throughput; checks responses, log volume,
# and challenge latency relative to health. Requires release binaries, wrk, Node.js, curl, and ps.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BINARY="${PERF_GATE_BINARY:-./target/release/rilgate}"
UPSTREAM_BINARY="${PERF_GATE_UPSTREAM_BINARY:-./target/release/examples/null-upstream}"
RUNDIR="${PERF_GATE_RUNDIR:-/tmp/rilgate-perf-gate}"
PORT="${PERF_GATE_PORT:-8080}"
OPERATIONS_PORT="${PERF_GATE_OPERATIONS_PORT:-9090}"
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
require ps
CLOCK_TICKS="$(getconf CLK_TCK 2>/dev/null || echo 100)"
if [[ ! -x "$BINARY" || ! -x "$UPSTREAM_BINARY" ]]; then
  echo "perf-gate: build the release gateway and null-upstream example first (see docs/development.md)" >&2
  exit 2
fi

wait_for_http() {
  local url="$1" pid="$2" tries=200
  while ((tries-- > 0)); do
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "perf-gate: server exited before readiness; inspect logs in $RUNDIR" >&2
      return 1
    fi
    if curl -fsS -o /dev/null "$url" 2>/dev/null; then return 0; fi
    sleep 0.1
  done
  echo "perf-gate: $url never became ready" >&2
  return 1
}

# Prints "pid cpu_seconds rss_kib" per server process; CPU time is cumulative user plus system.
process_snapshot() {
  local pid stat fields rss
  for pid in "${PIDS[@]}"; do
    if [[ -r "/proc/$pid/stat" ]]; then
      # procps reports CPU time in whole seconds, so Linux reads clock ticks from procfs.
      stat="$(<"/proc/$pid/stat")"
      read -r -a fields <<<"${stat##*) }"
      rss="$(awk '$1=="VmRSS:" {print $2}' "/proc/$pid/status")"
      awk -v pid="$pid" -v ticks="$((fields[11] + fields[12]))" -v hz="$CLOCK_TICKS" -v rss="$rss" \
        'BEGIN { printf "%s %.2f %s\n", pid, ticks / hz, rss }'
    else
      ps -p "$pid" -o time=,rss= | awk -v pid="$pid" '{
        days = 0; time = $1
        if (time ~ /-/) { split(time, part, "-"); days = part[1]; time = part[2] }
        n = split(time, part, ":"); seconds = 0
        for (i = 1; i <= n; i++) seconds = seconds * 60 + part[i]
        printf "%s %.2f %s\n", pid, days * 86400 + seconds, $2
      }'
    fi
  done
}

# Reports throughput and p50/p99 latency in microseconds from wrk.
measure_latency() {
  local path="$1" ua="$2" out p50 p99 port="${4:-$PORT}"
  local result_prefix="$RUNDIR/$port-${path//\//_}${3:+-authenticated}"
  local request_headers=(-H "User-Agent: $ua")
  if [[ -n "${3:-}" ]]; then request_headers+=(-H "Cookie: $3"); fi
  wrk -t"$THREADS" -c"$CONNECTIONS" -d"$WARMUP" "${request_headers[@]}" \
    "http://127.0.0.1:$port$path" >/dev/null 2>&1
  process_snapshot >"$result_prefix-process-before.txt"
  out="$(TIMEFORMAT='real %R user %U sys %S'; { time wrk -t"$THREADS" -c"$CONNECTIONS" -d"$DURATION" --latency \
    "${request_headers[@]}" "http://127.0.0.1:$port$path"; } 2>"$result_prefix-client-time.txt")"
  process_snapshot >"$result_prefix-process-after.txt"
  printf '%s\n' "$out" >"$result_prefix-wrk.txt"
  curl -fsS "http://127.0.0.1:$OPERATIONS_PORT/.well-known/rilgate/api/metrics" >"$result_prefix-metrics.txt"
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
  p99="$(awk '$1=="99%" {print $2}' <<<"$out")"
  if [[ -z "$p50" || -z "$p99" ]]; then
    echo "perf-gate: missing latency for $path" >&2
    return 1
  fi
  awk -v rps="$(awk '/^Requests\/sec:/ {print $2}' <<<"$out")" -v p="$p50" -v tail="$p99" \
    'function micros(value) { if (value ~ /ms$/) return value * 1000; if (value ~ /us$/) return value + 0; return value * 1000000 } BEGIN { if (micros(p) <= 0 || micros(tail) <= 0) exit 1; printf "%d %d %d", rps, micros(p), micros(tail) }'
}

check_upstream() {
  local url="$1" headers status request_headers=()
  if [[ -n "${2:-}" ]]; then request_headers+=(-H "Cookie: $2"); fi
  headers="$(curl -fsS -D - -o /dev/null ${request_headers[@]+"${request_headers[@]}"} "$url")"
  status="$(awk 'NR==1 {print $2}' <<<"$headers")"
  if [[ "$status" != 200 ]] || ! tr -d '\r' <<<"$headers" | awk 'tolower($1)=="x-perf-upstream:" && $2=="dev-null" {found=1} END {exit !found}'; then
    echo "perf-gate: $url did not reach the null upstream" >&2
    return 1
  fi
}

mkdir -p "$RUNDIR"
rm -f "$RUNDIR"/gateway.log

if [[ -n "${PERF_GATE_UPSTREAM_WORKERS:-}" ]]; then
  TOKIO_WORKER_THREADS="$PERF_GATE_UPSTREAM_WORKERS" "$UPSTREAM_BINARY" "$UPSTREAM_PORT" >"$RUNDIR/upstream.log" 2>&1 &
else
  "$UPSTREAM_BINARY" "$UPSTREAM_PORT" >"$RUNDIR/upstream.log" 2>&1 &
fi
PIDS+=($!)
wait_for_http "http://127.0.0.1:$UPSTREAM_PORT/" "${PIDS[0]}"

cat >"$RUNDIR/rilgate.yml" <<EOF
server:
  listen: "127.0.0.1:$PORT"
  metrics_enabled: true
  decision_log: off
  operations:
    listen: "127.0.0.1:$OPERATIONS_PORT"
  upstream:
    url: "http://127.0.0.1:$UPSTREAM_PORT"
security:
  keys:
    challenge: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    auth: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
pow:
  rate_limit:
    enabled: false
policy:
  mode: custom
  rules:
    - name: protected
      when:
        path:
          exact: /protected
      action: challenge
      challenge:
        algorithm: fast
        difficulty: 1

EOF

# Use the shipping log filter unless PERF_GATE_RUST_LOG supplies an override.
if [[ -n "${PERF_GATE_RUST_LOG+x}" ]]; then
  RUST_LOG="$PERF_GATE_RUST_LOG" "$BINARY" --config "$RUNDIR/rilgate.yml" \
    >"$RUNDIR/gateway.log" 2>&1 &
else
  env -u RUST_LOG "$BINARY" --config "$RUNDIR/rilgate.yml" \
    >"$RUNDIR/gateway.log" 2>&1 &
fi
PIDS+=($!)
wait_for_http "http://127.0.0.1:$OPERATIONS_PORT/.well-known/rilgate/api/health" "${PIDS[1]}"

health_body="$(curl -fsS "http://127.0.0.1:$OPERATIONS_PORT/.well-known/rilgate/api/health")"
node --input-type=module -e 'if (JSON.parse(process.argv[1]).status !== "ok") process.exit(1)' "$health_body"
challenge_body="$(curl -fsS -H "User-Agent: $BROWSER_UA" "http://127.0.0.1:$PORT/protected")"
if [[ "$challenge_body" != *"window.POW_PRESENTATION_TOKEN = "* ]]; then
  echo "perf-gate: /protected did not return a challenge page" >&2
  exit 1
fi

auth_cookie="$(node scripts/perf/auth-cookie.mjs "http://127.0.0.1:$PORT")"
check_upstream "http://127.0.0.1:$UPSTREAM_PORT/protected"
check_upstream "http://127.0.0.1:$PORT/protected" "$auth_cookie"
check_upstream "http://127.0.0.1:$PORT/public"

log_bytes_before="$(stat -c%s "$RUNDIR/gateway.log" 2>/dev/null || stat -f%z "$RUNDIR/gateway.log" 2>/dev/null || echo 0)"

upstream_result="$(measure_latency /public rilgate-perf-gate/1.0 "" "$UPSTREAM_PORT")"
health_result="$(measure_latency /.well-known/rilgate/api/health rilgate-perf-gate/1.0 "" "$OPERATIONS_PORT")"
challenge_result="$(measure_latency /protected "$BROWSER_UA")"
authenticated_result="$(measure_latency /protected "$BROWSER_UA" "$auth_cookie")"
allowed_result="$(measure_latency /public rilgate-perf-gate/1.0)"
check_upstream "http://127.0.0.1:$PORT/protected" "$auth_cookie"
check_upstream "http://127.0.0.1:$PORT/public"
read -r upstream_rps upstream_p50 upstream_p99 <<<"$upstream_result"
read -r health_rps health_p50 health_p99 <<<"$health_result"
read -r challenge_rps challenge_p50 challenge_p99 <<<"$challenge_result"
read -r authenticated_rps authenticated_p50 authenticated_p99 <<<"$authenticated_result"
read -r allowed_rps allowed_p50 allowed_p99 <<<"$allowed_result"

log_bytes_after="$(stat -c%s "$RUNDIR/gateway.log" 2>/dev/null || stat -f%z "$RUNDIR/gateway.log" 2>/dev/null || echo 0)"
log_bytes=$((log_bytes_after - log_bytes_before))

printf 'perf-gate: conns=%s duration=%s\n' "$CONNECTIONS" "$DURATION"
printf '  %-22s %12s req/s  p50 %sus  p99 %sus\n' "null upstream direct" "$upstream_rps" "$upstream_p50" "$upstream_p99"
printf '  %-22s %12s req/s  p50 %sus  p99 %sus\n' "health" "$health_rps" "$health_p50" "$health_p99"
printf '  %-22s %12s req/s  p50 %sus  p99 %sus\n' "challenge page" "$challenge_rps" "$challenge_p50" "$challenge_p99"
printf '  %-22s %12s req/s  p50 %sus  p99 %sus\n' "authenticated proxy" "$authenticated_rps" "$authenticated_p50" "$authenticated_p99"
printf '  %-22s %12s req/s  p50 %sus  p99 %sus\n' "allowed proxy" "$allowed_rps" "$allowed_p50" "$allowed_p99"
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
