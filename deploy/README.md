# Deployment examples

Build a native binary from the repository root with `scripts/build.sh --release`.
For Docker Compose, create the configuration before starting the service:

```bash
cp config/rilgate.example.yml config/rilgate.yml
docker compose -f deploy/docker/compose.yml up --build
```

To build the image separately:

```bash
docker build -f deploy/docker/Dockerfile -t rilgate:local .
```

The Compose file mounts root `config/` and connects to an upstream and P2Pool on
the host. Edit those addresses for the deployment. `config/rilgate.yml` is the
default configuration file; it is not included in Git. Without it, built-in
defaults allow requests at weight zero. Use `--config` or `CONFIG` to select
another file. The full example uses development mode; set `ENV=production`
and strong `POW_SIGNING_KEY` and `POW_AUTH_SIGNING_KEY` values for a public
deployment. See [configuration](../docs/configuration.md) for precedence and
validation. The example assigns Fast challenges to browser-like User-Agents;
only policies that select RandomX require P2Pool.

For systemd, create a `rilgate` user, install the binary under `/opt/rilgate`,
configuration under `/etc/rilgate`, and a writable `/var/lib/rilgate` directory.
Provide `/etc/rilgate/environment` with production settings and strong signing
keys. If filesystem web overrides are enabled, set `web.root` to
`/var/lib/rilgate/web`. `systemctl reload rilgate` requests a web snapshot reload.

The Caddy and nginx examples forward the host and support the gateway's P2Pool
WebSocket API. Rilgate does not proxy upstream application WebSockets. Set the
public hostname and configure TLS at the edge; the nginx example only contains
an HTTP listener. Client rate limits use socket addresses, so clients behind
the edge proxy share its quota. Tune the gateway limits or enforce client
quotas at the edge; forwarded IP headers are not trusted by the gateway.
