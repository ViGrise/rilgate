//! Replaces the running binary with a release published on GitHub.
use anyhow::{Context, Result, ensure};
use semver::Version;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

const RELEASES: &str = "https://github.com/ViGrise/rilgate/releases";

pub async fn run(version: Option<String>, force: bool) -> Result<()> {
  let asset = asset_name().context("no release binary is published for this platform")?;
  let current = Version::parse(env!("CARGO_PKG_VERSION")).expect("package version is semver");
  let client = reqwest::Client::builder()
    .user_agent(concat!("rilgate/", env!("CARGO_PKG_VERSION")))
    .timeout(Duration::from_secs(300))
    .redirect(reqwest::redirect::Policy::none())
    .build()?;
  let tag = match version {
    Some(tag) => tag,
    None => latest_tag(&client).await?,
  };
  let target = Version::parse(tag.trim_start_matches('v'))
    .with_context(|| format!("release tag {tag} is not a semantic version"))?;
  if !force && target <= current {
    println!("rilgate {current} is up to date (release {tag})");
    return Ok(());
  }
  let downloads = reqwest::Client::builder()
    .user_agent(concat!("rilgate/", env!("CARGO_PKG_VERSION")))
    .timeout(Duration::from_secs(300))
    .build()?;
  let base = format!("{RELEASES}/download/{tag}");
  let sums = fetch(&downloads, &format!("{base}/SHA256SUMS")).await?;
  let expected = checksum_for(&String::from_utf8_lossy(&sums), asset)
    .with_context(|| format!("SHA256SUMS of {tag} has no entry for {asset}"))?;
  let binary = fetch(&downloads, &format!("{base}/{asset}")).await?;
  ensure!(
    hex::encode(Sha256::digest(&binary)) == expected,
    "checksum mismatch for {asset} in {tag}"
  );
  let executable = std::env::current_exe()
    .and_then(|path| path.canonicalize())
    .context("locating the running binary")?;
  replace_executable(&executable, &binary)?;
  println!(
    "Upgraded {} from {current} to {target}",
    executable.display()
  );
  Ok(())
}

/// The release asset holding a bare binary for the platform this binary targets.
pub fn asset_name() -> Option<&'static str> {
  Some(match (std::env::consts::OS, std::env::consts::ARCH) {
    ("linux", "x86_64") => "rilgate-linux-amd64",
    ("linux", "aarch64") => "rilgate-linux-arm64",
    ("macos", "x86_64") => "rilgate-darwin-amd64",
    ("macos", "aarch64") => "rilgate-darwin-arm64",
    ("windows", "x86_64") => "rilgate-windows-amd64.exe",
    ("windows", "aarch64") => "rilgate-windows-arm64.exe",
    _ => return None,
  })
}

/// Reads the latest release tag from the redirect of the `latest` page, which
/// avoids the rate-limited GitHub API.
async fn latest_tag(client: &reqwest::Client) -> Result<String> {
  let response = client
    .get(format!("{RELEASES}/latest"))
    .send()
    .await
    .context("requesting the latest release")?;
  let location = response
    .headers()
    .get(reqwest::header::LOCATION)
    .and_then(|value| value.to_str().ok())
    .context("no release has been published")?;
  let tag = location
    .rsplit_once("/tag/")
    .map(|(_, tag)| tag)
    .context("unexpected latest release location")?;
  Ok(tag.to_string())
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
  let response = client
    .get(url)
    .send()
    .await
    .with_context(|| format!("downloading {url}"))?;
  let status = response.status();
  ensure!(status.is_success(), "downloading {url}: {status}");
  Ok(response.bytes().await?.to_vec())
}

/// Finds the hex digest for `asset` in a `sha256sum` listing.
pub fn checksum_for(sums: &str, asset: &str) -> Option<String> {
  sums.lines().find_map(|line| {
    let (digest, name) = line.split_once(char::is_whitespace)?;
    (name.trim().trim_start_matches('*') == asset).then(|| digest.to_ascii_lowercase())
  })
}

/// Writes the new binary beside the old one and swaps it in. The running
/// binary keeps working because its file stays open; Windows only allows a
/// running executable to be renamed, so the previous file is kept as `.old`.
pub fn replace_executable(executable: &Path, binary: &[u8]) -> Result<()> {
  let directory = executable
    .parent()
    .context("the running binary has no parent directory")?;
  let staged = sibling(executable, "new");
  std::fs::write(&staged, binary).map_err(|error| {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
      anyhow::anyhow!(
        "{} is not writable; rerun the upgrade with permission to replace {}",
        directory.display(),
        executable.display()
      )
    } else {
      anyhow::Error::new(error).context(format!("writing {}", staged.display()))
    }
  })?;
  let result = swap(executable, &staged);
  if result.is_err() {
    let _ = std::fs::remove_file(&staged);
  }
  result
}

#[cfg(unix)]
fn swap(executable: &Path, staged: &Path) -> Result<()> {
  use std::os::unix::fs::PermissionsExt;
  let mode = std::fs::metadata(executable)?.permissions().mode();
  std::fs::set_permissions(staged, std::fs::Permissions::from_mode(mode | 0o111))?;
  std::fs::rename(staged, executable).context("replacing the running binary")
}

#[cfg(windows)]
fn swap(executable: &Path, staged: &Path) -> Result<()> {
  let previous = sibling(executable, "old");
  let _ = std::fs::remove_file(&previous);
  std::fs::rename(executable, &previous).context("moving the running binary aside")?;
  if let Err(error) = std::fs::rename(staged, executable) {
    let _ = std::fs::rename(&previous, executable);
    return Err(error).context("installing the new binary");
  }
  Ok(())
}

#[cfg(not(any(unix, windows)))]
fn swap(_: &Path, _: &Path) -> Result<()> {
  anyhow::bail!("upgrading is not supported on this platform")
}

fn sibling(path: &Path, extension: &str) -> PathBuf {
  let mut name = path.file_name().unwrap_or_default().to_os_string();
  name.push(".");
  name.push(extension);
  path.with_file_name(name)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn checksums_are_found_by_exact_asset_name() {
    let sums = "AAA111  rilgate-linux-amd64\nbbb222  rilgate-linux-amd64.tar.gz\nccc333 *rilgate-windows-amd64.exe\n";
    assert_eq!(
      checksum_for(sums, "rilgate-linux-amd64").as_deref(),
      Some("aaa111")
    );
    assert_eq!(
      checksum_for(sums, "rilgate-windows-amd64.exe").as_deref(),
      Some("ccc333")
    );
    assert_eq!(checksum_for(sums, "rilgate-darwin-arm64"), None);
  }

  #[test]
  fn this_platform_has_a_release_asset() {
    if cfg!(any(
      target_os = "linux",
      target_os = "macos",
      target_os = "windows"
    )) && cfg!(any(target_arch = "x86_64", target_arch = "aarch64"))
    {
      assert!(asset_name().is_some());
    }
  }

  #[test]
  fn replacement_swaps_the_file_and_keeps_it_executable() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("rilgate");
    std::fs::write(&executable, b"old").unwrap();
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    replace_executable(&executable, b"new").unwrap();
    assert_eq!(std::fs::read(&executable).unwrap(), b"new");
    assert!(!sibling(&executable, "new").exists());
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      assert_eq!(
        std::fs::metadata(&executable).unwrap().permissions().mode() & 0o777,
        0o755
      );
    }
  }
}
