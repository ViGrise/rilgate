#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
image="${1:-rilgate:production-check}"
name="rilgate-smoke-$$"
network="rilgate-smoke-$$"
cleanup() {
  docker rm -f "$name" "$name-origin" >/dev/null 2>&1 || true
  docker network rm "$network" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker network create "$network" >/dev/null
docker run -d --name "$name-origin" --network "$network" --network-alias origin \
  -e WEB_BASE_PATH=/origin-internal \
  -e POW_SIGNING_KEY=container-origin-proof-secret-with-32-bytes \
  -e POW_AUTH_SIGNING_KEY=container-origin-auth-secret-with-32-bytes \
  "$image" >/dev/null
docker run -d --name "$name" --network "$network" --publish 127.0.0.1::8080 --read-only --cap-drop ALL \
  --security-opt no-new-privileges:true --pids-limit 128 --memory 1g \
  --tmpfs /tmp:size=16m,noexec,nosuid \
  -e POW_SIGNING_KEY=container-test-proof-secret-with-32-bytes \
  -e POW_AUTH_SIGNING_KEY=container-test-auth-secret-with-32-bytes \
  -e UPSTREAM_URL=http://origin:8080 "$image" >/dev/null
[[ "$(docker inspect --format '{{.Config.User}}' "$name")" == "10001:10001" ]]
for attempt in $(seq 1 60); do
  if docker exec "$name" curl -fsS --max-time 2 http://127.0.0.1:8080/.well-known/rilgate/api/ready >/dev/null; then break; fi
  if [[ "$attempt" == 60 ]]; then docker logs "$name"; exit 1; fi
  sleep 1
done
docker exec "$name" /app/rilgate --config /app/config/rilgate.yml check-config >/dev/null
for agent in curl Mozilla ''; do
  page="$(docker exec "$name" curl -fsS -A "$agent" http://127.0.0.1:8080/protected)"
  [[ "$page" == *POW_PRESENTATION* ]]
done
status="$(docker exec "$name" curl -sS -o /dev/null -w '%{http_code}' -X POST http://127.0.0.1:8080/protected)"
[[ "$status" == 401 ]]
address="$(docker port "$name" 8080/tcp)"
base="http://127.0.0.1:${address##*:}"
cookie="$(node tests/container/proof.mjs "$base")"
proxied="$(curl -fsS --max-time 10 -H "Cookie: $cookie" "$base/origin-internal/api/live")"
[[ "$proxied" == *'"status":"ok"'* ]]
docker stop "$name-origin" >/dev/null
status="$(docker exec "$name" curl -sS -o /dev/null -w '%{http_code}' http://127.0.0.1:8080/.well-known/rilgate/api/ready)"
[[ "$status" == 503 ]]
docker exec "$name" curl -fsS http://127.0.0.1:8080/.well-known/rilgate/api/live >/dev/null
docker start "$name-origin" >/dev/null
docker exec "$name" curl --retry 10 --retry-all-errors --retry-delay 1 -fsS --max-time 3 http://127.0.0.1:8080/.well-known/rilgate/api/ready >/dev/null
proxied="$(curl -fsS --max-time 10 -H "Cookie: $cookie" "$base/origin-internal/api/live")"
[[ "$proxied" == *'"status":"ok"'* ]]
docker stop --time 45 "$name" >/dev/null
[[ "$(docker inspect --format '{{.State.ExitCode}}' "$name")" == 0 ]]
printf '%s\n' 'Container proof, proxying, non-root operation, dependency failure/recovery and shutdown passed.'
