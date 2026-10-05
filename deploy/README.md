# Deployment examples

Build from the repository root with `scripts/build.sh --release`. Docker builds
use `docker build -f deploy/docker/Dockerfile -t rilgate:local .`; Compose uses
`docker compose -f deploy/docker/compose.yml up --build`.

The Compose file mounts root `config/` and connects to an upstream and P2Pool on
the host. Edit those addresses for the deployment. `config/rilgate.yml`
is the default configuration file and `config/rilgate.example.yml` is full configuration reference. Use
`--config` or `CONFIG` to select a deployment-specific file. `config/development.yml` uses Fast proof
for `/protected` without requiring P2Pool.

For systemd, create a `rilgate` user, install the binary under `/opt/rilgate`,
configuration under `/etc/rilgate`, and a writable `/var/lib/rilgate` directory.
Provide `/etc/rilgate/environment` with production settings and strong signing
keys. If filesystem web overrides are enabled, set `web.root` to
`/var/lib/rilgate/web`. `systemctl reload rilgate` requests a web snapshot reload.

The Caddy and nginx examples forward the original host and support WebSockets.
Set the public hostname and configure TLS at the edge. Client rate limits use
socket addresses; forwarded IP headers are not trusted by the gateway.
