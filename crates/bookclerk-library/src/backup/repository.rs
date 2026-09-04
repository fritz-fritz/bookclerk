//! Content-addressed backup object repository.
//!
//! Layout (under `$BOOKCLERK_FILES_DIR/backups/`):
//!
//! ```text
//! manifests/<recovery-point-id>.json
//! manifests/<recovery-point-id>.sha256
//! objects/ab/cdef...   # SHA-256 hex of uncompressed canonical JSON
//! ```
//!
//! Objects are immutable. A recovery point restores from its own manifest
//! without replaying older manifests. Garbage collection deletes objects that
//! no retained manifest references.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::encode::{
    decode_canonical_object, encode_canonical_object, sha256_hex, unwrap_stored_object,
    wrap_stored_object, CanonicalObject,
};
use super::{BackupManifest, BACKUPS_DIR};
use crate::error::{LibraryError, Result};

/// On-disk backup repository rooted at `files_dir/backups`.
#[derive(Debug, Clone)]
pub struct BackupRepository {
    /// `$BOOKCLERK_FILES_DIR/backups`.
    root: PathBuf,
}

impl BackupRepository {
    /// Opens (creating) the repository under `files_dir/backups`.
    ///
    /// # Errors
    ///
    /// Returns when the backups directory cannot be created.
    pub fn open(files_dir: &Path) -> Result<Self> {
        Self::open_root(&files_dir.join(BACKUPS_DIR))
    }

    /// Opens `root` as a backups repository (`manifests/` + `objects/`).
    ///
    /// # Errors
    ///
    /// Returns when the directories cannot be created.
    pub fn open_root(root: &Path) -> Result<Self> {
        fs::create_dir_all(root.join("manifests"))?;
        fs::create_dir_all(root.join("objects"))?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Repository root (`…/backups`).
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Writes `object` if missing. Returns the uncompressed SHA-256 hex.
    ///
    /// The write is into a sibling temp file then renamed so readers never see
    /// a partial object. An existing object is reused only after its bytes
    /// verify; a truncated or corrupt file is replaced atomically.
    ///
    /// # Errors
    ///
    /// Returns when encoding, compression, or the filesystem write fails.
    pub fn put_object(&self, object: &CanonicalObject) -> Result<String> {
        let uncompressed = encode_canonical_object(object)?;
        let digest = sha256_hex(&uncompressed);
        let path = self.object_path(&digest)?;
        if path.is_file() {
            match self.get_object(&digest) {
                Ok(_) => return Ok(digest),
                Err(_) => {
                    // Corrupt or truncated: replace below.
                }
            }
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let stored = wrap_stored_object(&uncompressed)?;
        let tmp = path.with_extension("tmp");
        {
            let mut f = File::create(&tmp)?;
            f.write_all(&stored)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &path)?;
        Ok(digest)
    }

    /// Loads and verifies one object by uncompressed digest.
    ///
    /// # Errors
    ///
    /// Returns when the object is missing, corrupt, or the digest mismatches.
    pub fn get_object(&self, digest: &str) -> Result<CanonicalObject> {
        let path = self.object_path(digest)?;
        if !path.is_file() {
            return Err(LibraryError::Schema(format!(
                "backup object `{digest}` is missing"
            )));
        }
        let stored = fs::read(&path)?;
        let uncompressed = unwrap_stored_object(&stored)?;
        let actual = sha256_hex(&uncompressed);
        if actual != digest {
            return Err(LibraryError::Schema(format!(
                "backup object `{digest}` content does not match its address (`{actual}`)"
            )));
        }
        decode_canonical_object(&uncompressed)
    }

    /// True when an object file exists (does not imply the bytes are valid).
    #[must_use]
    pub fn object_exists(&self, digest: &str) -> bool {
        self.object_path(digest)
            .map(|p| p.is_file())
            .unwrap_or(false)
    }

    /// Publishes `manifest` only after all referenced objects exist **and**
    /// verify.
    ///
    /// Writes JSON then a sibling `.sha256` of the exact file bytes. Incomplete
    /// staging uses `manifests/.staging-<id>.json` and is ignored by list/GC.
    ///
    /// # Errors
    ///
    /// Returns when a referenced object is missing or corrupt, or the
    /// filesystem write fails.
    pub fn publish_manifest(&self, manifest: &BackupManifest) -> Result<PathBuf> {
        for digest in manifest.referenced_objects() {
            self.get_object(&digest).map_err(|err| {
                LibraryError::Schema(format!("cannot publish backup `{}`: {err}", manifest.id))
            })?;
        }
        let json = serde_json::to_vec_pretty(manifest)
            .map_err(|err| LibraryError::Other(anyhow::anyhow!("backup manifest json: {err}")))?;
        let dir = self.root.join("manifests");
        fs::create_dir_all(&dir)?;
        let staging = dir.join(format!(".staging-{}.json", manifest.id));
        let final_path = dir.join(format!("{}.json", manifest.id));
        let hash_path = dir.join(format!("{}.sha256", manifest.id));
        fs::write(&staging, &json)?;
        let digest = sha256_hex(&json);
        fs::write(&hash_path, format!("{digest}\n"))?;
        fs::rename(&staging, &final_path)?;
        Ok(final_path)
    }

    /// Reads and integrity-checks a published manifest.
    ///
    /// # Errors
    ///
    /// Returns when the file is missing, JSON is malformed, the sidecar hash
    /// mismatches, or the embedded id does not match the filename.
    pub fn read_manifest(&self, id: &str) -> Result<BackupManifest> {
        if !manifest_id_ok(id) {
            return Err(LibraryError::Schema(format!(
                "backup recovery-point id `{id}` is not a safe path"
            )));
        }
        let path = self.root.join("manifests").join(format!("{id}.json"));
        let hash_path = self.root.join("manifests").join(format!("{id}.sha256"));
        if !path.is_file() {
            return Err(LibraryError::Schema(format!(
                "backup recovery point `{id}` is missing"
            )));
        }
        let json = fs::read(&path)?;
        let expected = fs::read_to_string(&hash_path)
            .map_err(|_| {
                LibraryError::Schema(format!(
                    "backup recovery point `{id}` is missing its integrity sidecar"
                ))
            })?
            .trim()
            .to_ascii_lowercase();
        let actual = sha256_hex(&json);
        if actual != expected {
            return Err(LibraryError::Schema(format!(
                "backup recovery point `{id}` manifest digest mismatch"
            )));
        }
        let manifest: BackupManifest = serde_json::from_slice(&json).map_err(|err| {
            LibraryError::Schema(format!("backup recovery point `{id}` json: {err}"))
        })?;
        if manifest.id != id {
            return Err(LibraryError::Schema(format!(
                "backup manifest id `{}` does not match file `{id}`",
                manifest.id
            )));
        }
        Ok(manifest)
    }

    /// Lists published recovery-point manifests oldest-first. Staging files
    /// and unreadable manifests are skipped.
    #[must_use]
    pub fn list_manifests(&self) -> Vec<BackupManifest> {
        let dir = self.root.join("manifests");
        let Ok(entries) = fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for ent in entries.flatten() {
            let name = ent.file_name();
            let name = name.to_string_lossy();
            if !name.ends_with(".json") || name.starts_with('.') {
                continue;
            }
            let id = name.trim_end_matches(".json");
            if let Ok(manifest) = self.read_manifest(id) {
                out.push(manifest);
            }
        }
        out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        out
    }

    /// Lists every published recovery-point manifest, failing if any cannot
    /// be verified. Staging files (dot-prefixed) are ignored.
    ///
    /// Use this for prune/GC. [`Self::list_manifests`] stays best-effort for
    /// non-destructive listing.
    ///
    /// # Errors
    ///
    /// Returns when the manifests directory cannot be read, a published
    /// `*.json` fails [`Self::read_manifest`], or an unexpected non-sidecar
    /// file is present.
    pub fn list_manifests_strict(&self) -> Result<Vec<BackupManifest>> {
        let dir = self.root.join("manifests");
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let entries = fs::read_dir(&dir)?;
        let mut out = Vec::new();
        for ent in entries {
            let ent = ent?;
            let name = ent.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') {
                continue;
            }
            if name.ends_with(".sha256") {
                continue;
            }
            if !name.ends_with(".json") {
                return Err(LibraryError::Schema(format!(
                    "backup manifests directory contains unexpected file `{name}`"
                )));
            }
            let id = name.trim_end_matches(".json");
            out.push(self.read_manifest(id)?);
        }
        out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(out)
    }

    /// Deletes one published manifest (not its objects). Returns whether a
    /// file was removed.
    ///
    /// # Errors
    ///
    /// Returns when the id is unsafe or unlink fails for a reason other than
    /// not found.
    pub fn delete_manifest(&self, id: &str) -> Result<bool> {
        if !manifest_id_ok(id) {
            return Err(LibraryError::Schema(format!(
                "backup recovery-point id `{id}` is not a safe path"
            )));
        }
        let json = self.root.join("manifests").join(format!("{id}.json"));
        let hash = self.root.join("manifests").join(format!("{id}.sha256"));
        let mut removed = false;
        for path in [json, hash] {
            match fs::remove_file(&path) {
                Ok(()) => removed = true,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err.into()),
            }
        }
        Ok(removed)
    }

    /// Deletes objects not referenced by any retained published manifest.
    ///
    /// Reachability is computed from a **strict** enumeration of published
    /// manifests. An unreadable retained manifest fails closed instead of
    /// treating its objects as garbage.
    ///
    /// # Errors
    ///
    /// Returns when a published manifest cannot be verified, a directory
    /// cannot be read, or an unreferenced object cannot be unlinked.
    pub fn gc_unreferenced_objects(&self) -> Result<usize> {
        let mut live = BTreeSet::new();
        for manifest in self.list_manifests_strict()? {
            live.extend(manifest.referenced_objects());
        }
        let objects = self.root.join("objects");
        let mut deleted = 0usize;
        let Ok(prefixes) = fs::read_dir(&objects) else {
            return Ok(0);
        };
        for prefix in prefixes.flatten() {
            let path = prefix.path();
            if !path.is_dir() {
                continue;
            }
            let Ok(files) = fs::read_dir(&path) else {
                continue;
            };
            for file in files.flatten() {
                let file_path = file.path();
                if file_path.extension().and_then(|e| e.to_str()) == Some("tmp") {
                    let _ = fs::remove_file(&file_path);
                    continue;
                }
                let Some(name) = file_path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let Some(dir) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let digest = format!("{dir}{name}");
                if live.contains(&digest) {
                    continue;
                }
                fs::remove_file(&file_path)?;
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    /// Content-addressed path `objects/<aa>/<rest>` for a SHA-256 hex digest.
    fn object_path(&self, digest: &str) -> Result<PathBuf> {
        if !object_digest_ok(digest) {
            return Err(LibraryError::Schema(format!(
                "backup object digest `{digest}` is not a SHA-256 hex id"
            )));
        }
        Ok(self
            .root
            .join("objects")
            .join(&digest[..2])
            .join(&digest[2..]))
    }
}

/// True when `digest` is a 64-character SHA-256 hex id.
fn object_digest_ok(digest: &str) -> bool {
    digest.len() == 64 && digest.chars().all(|c| c.is_ascii_hexdigit())
}

/// True when `id` is a safe recovery-point file name (no path separators).
fn manifest_id_ok(id: &str) -> bool {
    !id.is_empty()
        && !id.contains('/')
        && !id.contains('\\')
        && id != "."
        && id != ".."
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}
