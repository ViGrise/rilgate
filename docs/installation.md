# Installation

Rilgate releases are on [GitHub Releases](https://github.com/ViGrise/rilgate/releases). The
install scripts download a release binary, verify it against `SHA256SUMS` and install it.

## Linux, macOS and FreeBSD

```bash
curl -fsSL https://github.com/ViGrise/rilgate/releases/latest/download/install.sh | bash
```

The script installs to `/usr/local/bin` and uses `sudo` when that directory is
not writable. Set these variables to change the defaults:

| Variable      | Default          | Description                                                                 |
| ------------- | ---------------- | --------------------------------------------------------------------------- |
| `INSTALL_DIR` | `/usr/local/bin` | Destination directory                                                       |
| `VERSION`     | `latest`         | Release tag, for example `v0.3.0`                                           |
| `LIBC`        | detected         | Linux amd64 and arm64 only: `gnu` for the glibc build (glibc 2.36 or newer), `musl` for the static build |

```bash
curl -fsSL https://github.com/ViGrise/rilgate/releases/latest/download/install.sh | VERSION=v0.3.0 INSTALL_DIR="$HOME/.local/bin" bash
```

Linux ppc64le and riscv64 have only the static build. The FreeBSD build is available
for amd64.

## Linux packages

Each release has a Debian package, an RPM package and a systemd system extension
image for amd64, arm64, ppc64le and riscv64. The packages hold the static build.

```bash
sudo apt install ./rilgate_<version>_amd64.deb
sudo dnf install ./rilgate-<version>-1.x86_64.rpm
```

The package installs `/usr/bin/rilgate`, the `rilgate.service` systemd unit and the
example configuration as `/etc/rilgate/rilgate.yml`. On the first installation, it
writes `/etc/rilgate/environment` with a generated `RILGATE_SECURITY_SECRET`. Set the
upstream URL in `/etc/rilgate/rilgate.yml`, then start the service:

```bash
sudo systemctl enable --now rilgate
```

The service runs as a dynamic system user with the state directory `/var/lib/rilgate`.
Upgrade with the package manager.

To use the system extension image, install it as `rilgate.raw`. The release file in the
image has the name `extension-release.rilgate`, so the image must have the same name:

```bash
sudo install -D -m 0644 rilgate-sysext-linux-amd64.raw /var/lib/extensions/rilgate.raw
sudo systemd-sysext refresh
sudo install -D -m 0644 /usr/share/rilgate/rilgate.yml /etc/rilgate/rilgate.yml
sudo systemctl daemon-reload
sudo systemctl enable --now rilgate
```

Put `RILGATE_SECURITY_SECRET` in `/etc/rilgate/environment` to keep access cookies
valid after a restart.

## Windows

```powershell
irm https://github.com/ViGrise/rilgate/releases/latest/download/install.ps1 | iex
```

The script installs to `%LOCALAPPDATA%\rilgate\bin` and adds that directory to
the user `PATH`. Set these parameters, or the `VERSION` and `INSTALL_DIR`
environment variables, to change the defaults:

| Parameter     | Default                       | Description                       |
| ------------- | ----------------------------- | --------------------------------- |
| `-InstallDir` | `%LOCALAPPDATA%\rilgate\bin`  | Destination directory             |
| `-Version`    | `latest`                      | Release tag, for example `v0.3.0` |

```powershell
& ([scriptblock]::Create((irm https://github.com/ViGrise/rilgate/releases/latest/download/install.ps1))) -Version v0.3.0
```

The release also has `rilgate-windows-amd64.msi` and `rilgate-windows-arm64.msi`. The
installer puts `rilgate.exe`, `LICENSE.txt` and the example `rilgate.yml` into
`C:\Program Files\Rilgate` and adds that directory to the system `PATH`.

## Verify a download

`SHA256SUMS` has the checksum of each release file whose name starts with `rilgate`:

```bash
sha256sum --check --ignore-missing SHA256SUMS
```

A signed release also has a detached GPG signature `<file>.asc` for each file and the
public key `rilgate-release-key.asc`:

```bash
gpg --import rilgate-release-key.asc
gpg --verify SHA256SUMS.asc SHA256SUMS
```

## Source

`rilgate-src-vendor-<version>.tar.gz` holds the source with all Cargo dependencies
vendored. It builds without network access:

```bash
scripts/build.sh --release --offline
```

## Docker

```bash
docker run -d -p 8080:8080 --add-host=host.docker.internal:host-gateway \
  -e RILGATE_SECURITY_SECRET="$(openssl rand -hex 32)" \
  -e RILGATE_SERVER_UPSTREAM_URL=http://host.docker.internal:3000 \
  ghcr.io/vigrise/rilgate:latest
```

`latest` uses Alpine. `latest-debian` uses Debian.

With Docker Compose:

```yaml
# compose.yml
services:
  rilgate:
    container_name: rilgate
    image: ghcr.io/vigrise/rilgate:latest
    restart: unless-stopped
    environment:
      RILGATE_SERVER_UPSTREAM_URL: "http://host.docker.internal:3000"
    ports:
      - "8080:8080"
    extra_hosts:
      - "host.docker.internal:host-gateway"
    read_only: true
    tmpfs:
      - /tmp:size=16m,noexec,nosuid
    logging:
      driver: json-file
      options:
        max-size: "10m"
```

```bash
docker compose up -d
```

See [deployment examples](../deploy/README.md) for a hardened Compose file and systemd.

## Upgrade

Run the install script again, or use the built-in command. For a Linux package, use
the package manager.

```bash
rilgate upgrade
```
