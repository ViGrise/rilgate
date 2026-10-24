# Deployment examples

Tagged releases publish binaries on GitHub Releases and multi-architecture images at
`ghcr.io/vigrise/rilgate`. See [release, rollout and rollback](../docs/production.md#release-rollout-and-rollback). `install.sh` verifies a
release archive against `SHA256SUMS` and installs it into `/usr/local/bin`.

To choose another directory, set `INSTALL_DIR`. To pin a release, set `VERSION=v<version>`.
`rilgate upgrade` installs newer releases.

Build a native binary from the repository root with `scripts/build.sh --release`. The Pingora transport
builds OpenSSL from source, so builds need Perl, `make`, `cmake` and a C compiler. The
binary contains OpenSSL. For runtime, install `ca-certificates`. The Docker image
includes it. On Linux, `install.sh` selects the dynamic glibc build or the static musl
build. Set `LIBC=gnu` or `LIBC=musl` to select one.

For Docker Compose:

1. Review `config/rilgate.production.yml`.
2. Supply the signing secret through the deployment secret store:

```bash
export RILGATE_SECURITY_SECRET="$(openssl rand -hex 32)"
docker compose -f deploy/docker/compose.yml up --build
```

To build the image separately:

```bash
docker build -f deploy/docker/Dockerfile -t rilgate:local .
```

The Compose file mounts `config/rilgate.production.yml`, runs as a non-root user with a read-only
filesystem, and limits memory, CPU and process count. It binds port 8080 only to
loopback.

3. Edit upstream addresses for the deployment.
4. Configure HTTPS at the edge.

The image also uses the production template by default and refuses to start without
strong signing secrets. The production template serves health, readiness and metrics on
the operations listener at `127.0.0.1:9090`. Compose moves it to `0.0.0.0:9090` inside the
container and publishes it on host loopback for Prometheus. The image healthcheck probes
`127.0.0.1:9090` under the default web namespace. If a mounted configuration changes the
operations address or namespace, override the healthcheck.

For systemd:

1. Create a `rilgate` user.
2. Install the binary under `/opt/rilgate`.
3. Install the production configuration under `/etc/rilgate`.
4. Create a writable `/var/lib/rilgate` directory.
5. Provide `/etc/rilgate/environment` with `RILGATE_SECURITY_SECRET` and any `RILGATE_*` setting variables.
6. If filesystem web overrides are enabled, set `site.resources.directory` to `/var/lib/rilgate/web`.

`systemctl reload rilgate` requests a web snapshot reload. The production template serves operational
routes on `127.0.0.1:9090`, so local probes and a Prometheus instance on the same host can
reach them.

The Caddy and nginx examples forward the host and support the gateway's Stratum pool WebSocket
API and upstream application WebSockets.

1. Set the public hostname.
2. Configure TLS at the edge.
3. Configure `server.trusted_proxies` with only the actual proxy addresses to get independent client
   proof quotas.

The nginx example contains only an HTTP listener. It enforces per-client request and
connection quotas at the edge. Caddy does not add a shared quota service.
Both examples block public operational endpoints.

See [production operations](../docs/production.md) for monitoring examples, capacity settings, key
rotation, token replay semantics, release checks and rollback. Run `scripts/test/container.sh IMAGE` to check
a built image.
