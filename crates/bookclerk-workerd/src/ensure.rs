//! Download / refresh the pinned Cloudflare `workerd` binary.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

use crate::pin::{
    binary_name, download_url, host_asset, WORKERD_RELEASE_TAG, WORKERD_VERSION_STAMP,
};

/// Join a single path component under `root` after canonicalize + `starts_with`.
fn join_component_under(root: &Path, name: &str) -> Result<PathBuf> {
    if name.is_empty()
        || name.contains('\0')
        || name.contains('/')
        || name.contains('\\')
        || name == "."
        || name == ".."
        || name.contains("..")
    {
        bail!("refusing unsafe path component: {name}");
    }
    let root_norm =
        fs::canonicalize(root).with_context(|| format!("canonicalize root {}", root.display()))?;
    let out = root_norm.join(name);
    // Lexical under-root before any further FS probe on `out`.
    if !out.starts_with(&root_norm) {
        bail!(
            "path {} escapes root {}",
            out.display(),
            root_norm.display()
        );
    }
    Ok(out)
}

/// Ensure `dir` exists, canonicalize it, and require it stays under `$HOME` when possible.
fn prepare_cache_dir(dir: &Path) -> Result<PathBuf> {
    // Lexical rejection of `..` before mkdir/canonicalize of operator/cache roots.
    if dir.components().any(|c| matches!(c, Component::ParentDir)) {
        bail!("refusing cache dir with '..': {}", dir.display());
    }
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let canon =
        fs::canonicalize(dir).with_context(|| format!("canonicalize cache {}", dir.display()))?;
    if let Ok(home) = std::env::var("HOME") {
        if let Ok(home_norm) = fs::canonicalize(home) {
            if !canon.starts_with(&home_norm) {
                bail!(
                    "workerd cache {} must resolve under home {}",
                    canon.display(),
                    home_norm.display()
                );
            }
        }
    }
    Ok(canon)
}

/// Ensure `dir/workerd` matches [`WORKERD_RELEASE_TAG`], downloading if needed.
///
/// Returns the path to the executable. Honors `BOOKCLERK_WORKERD_BIN` when set:
/// if that absolute path's sibling version stamp matches the pin, it is returned;
/// otherwise ensure still installs into `dir` (override path is not overwritten).
/// Currency is stamp-only — never `--version`-probes an env/cache path.
///
/// # Arguments
///
/// * `dir` - Directory path for this operation.
///
/// # Returns
///
/// On success, the inner `PathBuf` value.
///
/// # Errors
///
/// Returns an error when the underlying I/O, parse, network, or store operation fails.
pub fn ensure_workerd(dir: &Path) -> Result<PathBuf> {
    if let Ok(override_bin) = std::env::var("BOOKCLERK_WORKERD_BIN") {
        if !override_bin.is_empty()
            && !override_bin.contains('\0')
            && Path::new(&override_bin).is_absolute()
        {
            let path = PathBuf::from(&override_bin);
            // Stamp-only: no exists/metadata/spawn of the raw env path first.
            if is_current_stamp_only(&path)? {
                return bookclerk_sandbox::require_spawn_executable(&path)
                    .with_context(|| format!("validate workerd override {}", path.display()));
            }
        }
        // Stale/missing override: fall through to managed install in `dir`.
    }

    let dir = prepare_cache_dir(dir)?;
    let dest = join_component_under(&dir, binary_name())?;
    if is_current_stamp_only(&dest)? {
        return bookclerk_sandbox::require_spawn_executable(&dest)
            .with_context(|| format!("validate workerd binary {}", dest.display()));
    }

    let asset = host_asset().with_context(|| {
        format!(
            "no pinned workerd asset for {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;

    let url = download_url(asset.artifact);
    eprintln!("bookclerk-workerd: fetching {url}");
    let compressed = download(&url).with_context(|| format!("download {url}"))?;
    verify_sha256(&compressed, asset.sha256_hex)?;

    let mut decoder = GzDecoder::new(compressed.as_slice());
    let mut binary = Vec::new();
    decoder
        .read_to_end(&mut binary)
        .context("gunzip workerd payload")?;

    let tmp = join_component_under(&dir, &format!("{}.tmp", binary_name()))?;
    {
        let mut f = File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        f.write_all(&binary)
            .with_context(|| format!("write {}", tmp.display()))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&tmp)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&tmp, perms)?;
    }
    fs::rename(&tmp, &dest).with_context(|| {
        format!(
            "install workerd → {} (replace {})",
            dest.display(),
            tmp.display()
        )
    })?;

    let stamp = join_component_under(&dir, WORKERD_VERSION_STAMP)?;
    fs::write(&stamp, format!("{WORKERD_RELEASE_TAG}\n"))
        .with_context(|| format!("write {}", stamp.display()))?;

    eprintln!(
        "bookclerk-workerd: installed {WORKERD_RELEASE_TAG} → {}",
        dest.display()
    );
    bookclerk_sandbox::require_spawn_executable(&dest)
        .with_context(|| format!("validate installed workerd {}", dest.display()))
}

/// Preferred install directory: beside this process, else `dir` argument from callers.
///
/// # Arguments
///
/// * `dir` - Directory path for this operation.
///
/// # Returns
///
/// `PathBuf` result.
#[must_use]
pub fn workerd_bin_path(dir: &Path) -> PathBuf {
    if let Ok(override_bin) = std::env::var("BOOKCLERK_WORKERD_BIN") {
        if !override_bin.is_empty()
            && !override_bin.contains('\0')
            && Path::new(&override_bin).is_absolute()
        {
            return PathBuf::from(override_bin);
        }
    }
    dir.join(binary_name())
}

/// True when the sibling version stamp matches [`WORKERD_RELEASE_TAG`].
///
/// Stamp-only: never `--version`-probes a cache or env path (command-injection
/// under local threat modeling).
fn is_current_stamp_only(bin: &Path) -> Result<bool> {
    let Some(parent) = bin.parent() else {
        return Ok(false);
    };
    // Prefer canonical parent when it exists; fall back to lexical join.
    let stamp = if parent.exists() {
        join_component_under(parent, WORKERD_VERSION_STAMP)?
    } else {
        let stamp = parent.join(WORKERD_VERSION_STAMP);
        if !stamp.starts_with(parent) {
            bail!(
                "stamp {} escapes parent {}",
                stamp.display(),
                parent.display()
            );
        }
        stamp
    };
    if !stamp.starts_with(parent) && parent.exists() {
        // join_component_under already checked against canonicalize(parent).
    }
    match fs::read_to_string(&stamp) {
        Ok(text) => Ok(text.trim() == WORKERD_RELEASE_TAG),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).with_context(|| format!("read {}", stamp.display())),
    }
}

/// Downloads the pinned workerd artifact; fails on non-2xx or a truncated body.
fn download(url: &str) -> Result<Vec<u8>> {
    let mut response = ureq::get(url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    if !response.status().is_success() {
        bail!("GET {url} returned {}", response.status());
    }
    let mut body = Vec::new();
    response
        .body_mut()
        .as_reader()
        .read_to_end(&mut body)
        .context("read download body")?;
    Ok(body)
}

/// Rejects the download when the SHA-256 hex digest does not match the pin.
fn verify_sha256(bytes: &[u8], expected_hex: &str) -> Result<()> {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let got = hex::encode(hasher.finalize());
    if got != expected_hex {
        bail!("workerd download sha256 mismatch: got {got}, expected {expected_hex}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_asset_defined_for_ci_linux() {
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert!(host_asset().is_some());
        }
    }
}
