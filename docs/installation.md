# Installation

Rilgate releases are on [GitHub Releases](https://github.com/ViGrise/rilgate/releases). The
install scripts download a release binary, verify it against `SHA256SUMS` and install it.
Release files are signed with the release key
`F396 7F20 4825 CDE6 07A8  1452 B990 C5BC 82BD 2323`; see [verify a download](#verify-a-download).

## Linux, macOS and FreeBSD

```bash
curl -fsSL https://github.com/ViGrise/rilgate/releases/latest/download/install.sh | bash
```

The script installs to `/usr/local/bin` and uses `sudo` when that directory is
not writable. Set these variables to change the defaults:

| Variable      | Default          | Description                                                                 |
| ------------- | ---------------- | --------------------------------------------------------------------------- |
| `INSTALL_DIR` | `/usr/local/bin` | Destination directory                                                       |
| `VERSION`     | `latest`         | Release tag, for example `v0.5.0`                                           |
| `LIBC`        | detected         | Linux amd64 and arm64 only: `gnu` for the glibc build (glibc 2.36 or newer), `musl` for the static build |
| `REQUIRE_SIGNATURE` | `0`        | `1` refuses to install when `gpg` is missing                                |

When `gpg` is installed, the script also verifies `SHA256SUMS.asc` against the release
key that the script embeds. Without `gpg`, it checks the checksum only. The script that
`releases/latest/download` serves comes from the same release as the files it checks;
to verify it independently, download `install.sh` and `install.sh.asc` and verify them
as described in [verify a download](#verify-a-download) before you run it.

```bash
curl -fsSL https://github.com/ViGrise/rilgate/releases/latest/download/install.sh | VERSION=v0.5.0 INSTALL_DIR="$HOME/.local/bin" bash
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
| `-Version`    | `latest`                      | Release tag, for example `v0.5.0` |

```powershell
& ([scriptblock]::Create((irm https://github.com/ViGrise/rilgate/releases/latest/download/install.ps1))) -Version v0.5.0
```

The release also has `rilgate-windows-amd64.msi` and `rilgate-windows-arm64.msi`. The
installer puts `rilgate.exe`, `LICENSE.txt` and the example `rilgate.yml` into
`C:\Program Files\Rilgate` and adds that directory to the system `PATH`.

## Verify a download

`SHA256SUMS` has the checksum of each release file whose name starts with `rilgate`:

```bash
sha256sum --check --ignore-missing SHA256SUMS
```

Each release file also has a detached GPG signature `<file>.asc`. The release key has
the fingerprint `F396 7F20 4825 CDE6 07A8  1452 B990 C5BC 82BD 2323`, which this page,
the README and `install.sh` publish outside the release. Import the key, check that its
fingerprint matches, then verify:

```bash
gpg --import rilgate-release-key.asc
gpg --fingerprint F3967F204825CDE607A81452B990C5BC82BD2323
gpg --verify SHA256SUMS.asc SHA256SUMS
```

`install.ps1` checks the checksum only. On Windows, verify the signature with Gpg4win
when the download needs it.

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
the package manager. `rilgate upgrade` verifies `SHA256SUMS.asc` against the release
keys built into the binary, then checks the downloaded binary against `SHA256SUMS`. It
refuses a release without a valid signature.

```bash
rilgate upgrade
```
