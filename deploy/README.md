# Deployment examples

Tagged releases publish binaries on GitHub Releases and multi-architecture
images at `ghcr.io/vigrise/rilgate`; see
[release, rollout and rollback](../docs/production.md#release-rollout-and-rollback).
Build a native binary from the repository root with `scripts/build.sh --release`.
The Pingora transport requires OpenSSL at build time and runtime. On Debian or
Ubuntu, install `pkg-config` and `libssl-dev` to build, and `libssl3` plus
`ca-certificates` to run. The Docker image includes these dependencies.

For Docker Compose, review `config/rilgate.production.yml` and supply the signing
secret through the deployment secret store:

```bash
export RILGATE_SECURITY_SECRET="$(openssl rand -hex 32)"
docker compose -f deploy/docker/compose.yml up --build
```

To build the image separately:

```bash
docker build -f deploy/docker/Dockerfile -t rilgate:local .
```

The Compose file mounts `config/rilgate.production.yml`, runs as a non-root user
with a read-only filesystem, and limits memory, CPU and process count. It binds
port 8080 only to loopback. Edit upstream addresses for the deployment and
configure HTTPS at the edge. The image also uses the production template by
default and refuses to start without strong signing secrets. The production
template serves health, readiness and metrics on the operations listener at
`127.0.0.1:9090`; Compose moves it to `0.0.0.0:9090` inside the container and
publishes it on host loopback for Prometheus. The image healthcheck probes
`127.0.0.1:9090` under the default web namespace; override it when a mounted
configuration changes the operations address or namespace.

For systemd, create a `rilgate` user, install the binary under `/opt/rilgate`,
the production configuration under `/etc/rilgate`, and a writable `/var/lib/rilgate` directory.
Provide `/etc/rilgate/environment` with `RILGATE_SECURITY_SECRET` and any `RILGATE_*` setting variables. If filesystem web overrides are enabled, set `site.resources.directory` to
`/var/lib/rilgate/web`. `systemctl reload rilgate` requests a web snapshot reload.
The production template serves operational routes on `127.0.0.1:9090`, so local
probes and a Prometheus instance on the same host can reach them.

The Caddy and nginx examples forward the host and support the gateway's P2Pool
WebSocket API and upstream application WebSockets. Set the
public hostname and configure TLS at the edge; the nginx example only contains
an HTTP listener. Configure `server.trusted_proxies` with only the actual proxy
addresses to obtain independent client proof quotas. The nginx example enforces
per-client request and connection quotas at the edge; Caddy does not add a
shared quota service. Both examples block public operational endpoints.

See [production operations](../docs/production.md) for monitoring examples,
capacity settings, key rotation, token replay semantics, release checks and
rollback. Run `scripts/test/container.sh IMAGE` to check a built image.
