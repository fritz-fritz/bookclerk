//! Provenance-qualified plugin identity.
//!
//! A bare `plugin.toml` `id` (`sqlite`, `local`, `audible`) is a **display
//! alias**, not a security principal. Durable ownership uses [`PluginKey`]:
//! canonical provenance + package coordinate + manifest id. Version and
//! content hashes are **not** part of the key; they belong on
//! [`ArtifactIdentity`].
//!
//! Platform trust is host-controlled ([`PluginProvenance::PlatformBundled`])
//! after content verification. A third-party package that declares
//! `id = "sqlite"` does not receive platform defaults.

use std::fmt;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::coordinate::{PackageCoordinate, RegistrySource};
use crate::error::{CatalogError, Result};
use bookclerk_plugin_manifest::validate_plugin_id;

/// Product name recorded on Bookclerk-shipped platform artifacts.
pub const PLATFORM_PRODUCT: &str = "bookclerk";

/// Known installer-shipped platform packages. Host-controlled; never read from
/// plugin-authored manifest fields other than matching `id` after provenance
/// is already established.
pub const PLATFORM_ARTIFACTS: &[PlatformArtifact] = &[
    PlatformArtifact {
        package_name: "bookclerk-plugin-database-sqlite",
        manifest_id: "sqlite",
        allowed_entrypoints: &["databaseAdapter"],
        allowed_bindings: &["config", "work_fs"],
    },
    PlatformArtifact {
        package_name: "bookclerk-plugin-destination-local",
        manifest_id: "local",
        allowed_entrypoints: &["storage"],
        allowed_bindings: &["config", "work_fs"],
    },
];

/// One Bookclerk-shipped platform package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformArtifact {
    /// Cargo package name staged by `install-platform`.
    pub package_name: &'static str,
    /// Manifest alias (`sqlite`, `local`).
    pub manifest_id: &'static str,
    /// Entrypoints the installer envelope permits.
    pub allowed_entrypoints: &'static [&'static str],
    /// Host bindings the installer envelope permits.
    pub allowed_bindings: &'static [&'static str],
}

impl PlatformArtifact {
    /// [`PluginKey`] for this platform package.
    #[must_use]
    pub fn plugin_key(self) -> PluginKey {
        PluginKey::platform(self.package_name, self.manifest_id)
            .expect("platform artifact ids are valid")
    }
}

/// Looks up a known platform artifact by package name + manifest id.
#[must_use]
pub fn platform_artifact(
    package_name: &str,
    manifest_id: &str,
) -> Option<&'static PlatformArtifact> {
    PLATFORM_ARTIFACTS
        .iter()
        .find(|a| a.package_name == package_name && a.manifest_id == manifest_id)
}

/// True when `key` is a known Bookclerk platform [`PluginKey`].
#[must_use]
pub fn is_platform_plugin_key(key: &PluginKey) -> bool {
    PLATFORM_ARTIFACTS.iter().any(|a| a.plugin_key() == *key)
}

/// Provenance scheme in a canonical [`PluginKey`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProvenanceScheme {
    /// crates.io / Cargo registry.
    Cargo,
    /// npm registry.
    Npm,
    /// PyPI.
    Pypi,
    /// Static HTTPS plugin index.
    Registry,
    /// Bookclerk-shipped installer artifact.
    Platform,
    /// Local filesystem install / development tree.
    Path,
}

impl ProvenanceScheme {
    /// Canonical scheme token used in [`PluginKey`] text.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Npm => "npm",
            Self::Pypi => "pypi",
            Self::Registry => "registry",
            Self::Platform => "platform",
            Self::Path => "path",
        }
    }

    /// Parses a canonical scheme token.
    fn parse(s: &str) -> Option<Self> {
        match s {
            "cargo" => Some(Self::Cargo),
            "npm" => Some(Self::Npm),
            "pypi" => Some(Self::Pypi),
            "registry" => Some(Self::Registry),
            "platform" => Some(Self::Platform),
            "path" => Some(Self::Path),
            _ => None,
        }
    }
}

impl fmt::Display for ProvenanceScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Stable identity across upgrades (not version, not artifact hash).
///
/// Canonical text (deterministic, unambiguous):
///
/// - `cargo:{origin}/{package}#{manifest_id}`
/// - `npm:{origin}/{package}#{manifest_id}`
/// - `pypi:{origin}/{package}#{manifest_id}`
/// - `registry:{index_url}#{package}#{manifest_id}`
/// - `platform:bookclerk/{package}#{manifest_id}`
/// - `path:file://{normalized_absolute_path}#{manifest_id}`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PluginKey {
    /// Canonical UTF-8 form (see type docs).
    canonical: String,
}

impl PluginKey {
    /// Builds a key from already-canonical text. Prefer the typed constructors.
    ///
    /// # Errors
    ///
    /// Returns an error when the canonical form cannot be parsed.
    pub fn parse(raw: &str) -> Result<Self> {
        let parsed = parse_canonical(raw)?;
        Ok(Self {
            canonical: parsed.canonical,
        })
    }

    /// Platform-bundled key (`platform:bookclerk/{package}#{id}`).
    ///
    /// # Errors
    ///
    /// Returns an error when `manifest_id` fails the plugin id grammar.
    pub fn platform(package_name: &str, manifest_id: &str) -> Result<Self> {
        Self::from_parts(
            ProvenanceScheme::Platform,
            PLATFORM_PRODUCT,
            package_name,
            manifest_id,
        )
    }

    /// Registry-sourced key (Cargo / npm / PyPI / static index).
    ///
    /// Version is stripped from `coordinate` — it is not part of the key.
    ///
    /// # Errors
    ///
    /// Returns an error when the coordinate or id is invalid.
    pub fn from_coordinate(coordinate: &PackageCoordinate, manifest_id: &str) -> Result<Self> {
        match &coordinate.source {
            RegistrySource::Cargo { registry_url } => Self::from_parts(
                ProvenanceScheme::Cargo,
                &normalize_origin(registry_url)?,
                &coordinate.name,
                manifest_id,
            ),
            RegistrySource::Npm { registry_url } => Self::from_parts(
                ProvenanceScheme::Npm,
                &normalize_origin(registry_url)?,
                &coordinate.name,
                manifest_id,
            ),
            RegistrySource::Pypi { simple_url } => Self::from_parts(
                ProvenanceScheme::Pypi,
                &normalize_origin(simple_url)?,
                &coordinate.name,
                manifest_id,
            ),
            RegistrySource::Static { index_url } => Self::from_parts(
                ProvenanceScheme::Registry,
                &normalize_index_url(index_url)?,
                &coordinate.name,
                manifest_id,
            ),
            RegistrySource::LocalArchive => {
                // Local archives still need a filesystem identity after extract.
                // Callers should use [`Self::from_install_path`] on the dest root.
                Err(CatalogError::message(
                    "local archive coordinates are not PluginKeys; use the installed path",
                ))
            }
        }
    }

    /// Filesystem / development-tree key for an install directory.
    ///
    /// # Errors
    ///
    /// Returns an error when the path cannot be made into a `file://` URL or
    /// the id is invalid.
    pub fn from_install_path(root: &Path, manifest_id: &str) -> Result<Self> {
        let file_url = path_to_file_url(root)?;
        Self::from_parts(ProvenanceScheme::Path, &file_url, "", manifest_id)
    }

    /// Formats one canonical key from already-normalized parts.
    fn from_parts(
        scheme: ProvenanceScheme,
        source: &str,
        package: &str,
        manifest_id: &str,
    ) -> Result<Self> {
        validate_plugin_id(manifest_id).map_err(|e| CatalogError::message(e.to_string()))?;
        if source.is_empty() && scheme != ProvenanceScheme::Path {
            return Err(CatalogError::message("plugin key source must not be empty"));
        }
        if source.contains('#') || package.contains('#') {
            return Err(CatalogError::message(
                "plugin key source and package must not contain '#'",
            ));
        }
        if package.contains('\n') || source.contains('\n') {
            return Err(CatalogError::message(
                "plugin key source and package must be single-line",
            ));
        }
        let canonical = match scheme {
            ProvenanceScheme::Registry => {
                if package.is_empty() {
                    return Err(CatalogError::message(
                        "registry plugin key needs a package name",
                    ));
                }
                format!("registry:{source}#{package}#{manifest_id}")
            }
            ProvenanceScheme::Path => {
                format!("path:{source}#{manifest_id}")
            }
            ProvenanceScheme::Platform => {
                if package.is_empty() {
                    return Err(CatalogError::message(
                        "platform plugin key needs a package name",
                    ));
                }
                format!("platform:{source}/{package}#{manifest_id}")
            }
            ProvenanceScheme::Cargo | ProvenanceScheme::Npm | ProvenanceScheme::Pypi => {
                if package.is_empty() {
                    return Err(CatalogError::message(
                        "registry plugin key needs a package name",
                    ));
                }
                format!("{}:{source}/{package}#{manifest_id}", scheme.as_str())
            }
        };
        Ok(Self { canonical })
    }

    /// Canonical textual form.
    #[must_use]
    pub fn canonical(&self) -> &str {
        &self.canonical
    }

    /// SHA-256 (hex) of the canonical UTF-8 bytes.
    #[must_use]
    pub fn digest_sha256(&self) -> String {
        hex::encode(Sha256::digest(self.canonical.as_bytes()))
    }

    /// Filesystem-safe directory leaf (`pk-` + first 16 hex chars of the digest).
    #[must_use]
    pub fn fs_id(&self) -> String {
        format!("pk-{}", &self.digest_sha256()[..16])
    }

    /// Manifest plugin id (display alias).
    #[must_use]
    pub fn manifest_id(&self) -> &str {
        self.canonical
            .rsplit_once('#')
            .map(|(_, id)| id)
            .unwrap_or(self.canonical.as_str())
    }

    /// Provenance scheme.
    #[must_use]
    pub fn scheme(&self) -> ProvenanceScheme {
        parse_canonical(&self.canonical)
            .map(|p| p.scheme)
            .unwrap_or(ProvenanceScheme::Path)
    }

    /// Package coordinate / name when present (empty for `path:` keys).
    #[must_use]
    pub fn package(&self) -> String {
        parsed_package(&self.canonical).unwrap_or_default()
    }
}

impl fmt::Display for PluginKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical)
    }
}

/// Parsed canonical key used while validating [`PluginKey::parse`].
struct ParsedKey {
    /// Rebuilt canonical text.
    canonical: String,
    /// Scheme token.
    scheme: ProvenanceScheme,
}

/// Parses and rebuilds a canonical key so equivalent spellings collapse.
fn parse_canonical(raw: &str) -> Result<ParsedKey> {
    let raw = raw.trim();
    if raw.is_empty() || raw.contains('\n') {
        return Err(CatalogError::message(
            "plugin key must be a single non-empty line",
        ));
    }
    let (scheme_str, rest) = raw
        .split_once(':')
        .ok_or_else(|| CatalogError::message(format!("plugin key `{raw}` missing scheme")))?;
    let scheme = ProvenanceScheme::parse(scheme_str).ok_or_else(|| {
        CatalogError::message(format!("unknown plugin key scheme `{scheme_str}`"))
    })?;
    let rebuilt = match scheme {
        ProvenanceScheme::Path => {
            let (source, id) = rest
                .split_once('#')
                .ok_or_else(|| CatalogError::message("path plugin key must be path:file://…#id"))?;
            PluginKey::from_parts(scheme, source, "", id)?
        }
        ProvenanceScheme::Platform => {
            let (source_pkg, id) = rest.split_once('#').ok_or_else(|| {
                CatalogError::message("platform plugin key must be platform:bookclerk/pkg#id")
            })?;
            let (product, package) = source_pkg.split_once('/').ok_or_else(|| {
                CatalogError::message("platform plugin key must include product/package")
            })?;
            if product != PLATFORM_PRODUCT {
                return Err(CatalogError::message(format!(
                    "unsupported platform product `{product}`"
                )));
            }
            PluginKey::from_parts(scheme, product, package, id)?
        }
        ProvenanceScheme::Registry => {
            let (index_and_pkg, id) = rsplit_hash(rest)?;
            let (index, package) = index_and_pkg.split_once('#').ok_or_else(|| {
                CatalogError::message(
                    "registry plugin key must be registry:<index_url>#<package>#<id>",
                )
            })?;
            PluginKey::from_parts(scheme, index, package, id)?
        }
        ProvenanceScheme::Cargo | ProvenanceScheme::Npm | ProvenanceScheme::Pypi => {
            let (origin_pkg, id) = rsplit_hash(rest)?;
            let (origin, package) = split_origin_package(origin_pkg)?;
            PluginKey::from_parts(scheme, &origin, &package, id)?
        }
    };
    Ok(ParsedKey {
        canonical: rebuilt.canonical,
        scheme,
    })
}

/// Package coordinate extracted from a canonical key (empty for `path:`).
fn parsed_package(canonical: &str) -> Option<String> {
    let (scheme, rest) = canonical.split_once(':')?;
    match scheme {
        "path" => Some(String::new()),
        "platform" => {
            let (source_pkg, _) = rest.rsplit_once('#')?;
            source_pkg.split_once('/').map(|(_, p)| p.to_string())
        }
        "registry" => {
            let (index_and_pkg, _) = rest.rsplit_once('#')?;
            index_and_pkg.split_once('#').map(|(_, p)| p.to_string())
        }
        _ => {
            let (origin_pkg, _) = rest.rsplit_once('#')?;
            let url = url::Url::parse(origin_pkg).ok()?;
            let path = url.path().trim_start_matches('/');
            if path.is_empty() {
                None
            } else {
                Some(path.to_string())
            }
        }
    }
}

/// Splits `rest` on the last `#` into `(prefix, manifest_id)`.
fn rsplit_hash(s: &str) -> Result<(&str, &str)> {
    s.rsplit_once('#')
        .ok_or_else(|| CatalogError::message("plugin key must end with #<manifest_id>"))
}

/// Splits `https://origin/package` into `(origin, package)`.
fn split_origin_package(s: &str) -> Result<(String, String)> {
    let url = url::Url::parse(s).map_err(|e| CatalogError::message(e.to_string()))?;
    if url.scheme() != "https" && url.scheme() != "http" {
        return Err(CatalogError::message(format!(
            "plugin key origin `{s}` must be http(s)"
        )));
    }
    let origin = normalize_origin(s)?;
    let package = url.path().trim_start_matches('/').to_string();
    if package.is_empty() {
        return Err(CatalogError::message(
            "plugin key origin must include /{package}",
        ));
    }
    Ok((origin, package))
}

/// Identity of one exact installed build: [`PluginKey`] + version + hashes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactIdentity {
    /// Logical plugin (stable across upgrades).
    pub plugin_key: PluginKey,
    /// Package / manifest version string.
    pub version: String,
    /// SHA-256 of the installed `plugin.toml` bytes.
    pub manifest_sha256: String,
    /// Deterministic Merkle-style root over the immutable installed payload.
    pub payload_root_sha256: String,
    /// SHA-256 of the downloaded archive, when the install came from a registry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_sha256: Option<String>,
}

/// How Bookclerk established (or lost) trust in an installed tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginProvenance {
    /// Host-staged Bookclerk platform artifact; content hashes match the receipt.
    PlatformBundled,
    /// Installed from a registry/archive; receipt hashes still match.
    VerifiedInstalled,
    /// Local/dev tree with no verified receipt (or path-only identity).
    LocalDevelopment,
    /// Receipt exists but the immutable payload no longer matches.
    Modified,
}

impl PluginProvenance {
    /// True when this provenance may receive installer platform default grants.
    #[must_use]
    pub fn grants_platform_defaults(self) -> bool {
        matches!(self, Self::PlatformBundled)
    }
}

impl fmt::Display for PluginProvenance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::PlatformBundled => "platform_bundled",
            Self::VerifiedInstalled => "verified_installed",
            Self::LocalDevelopment => "local_development",
            Self::Modified => "modified",
        })
    }
}

/// Evaluated identity for one discovered install directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginInstallIdentity {
    /// Stable logical identity.
    pub plugin_key: PluginKey,
    /// Exact installed build.
    pub artifact: ArtifactIdentity,
    /// Host-evaluated provenance (never plugin-authored).
    pub provenance: PluginProvenance,
}

impl PluginInstallIdentity {
    /// Display alias (`plugin.toml` id).
    #[must_use]
    pub fn alias(&self) -> &str {
        self.plugin_key.manifest_id()
    }
}

/// Normalizes a registry origin (scheme + host, no trailing slash, lowercase host).
fn normalize_origin(raw: &str) -> Result<String> {
    let url = url::Url::parse(raw).or_else(|_| url::Url::parse(&format!("https://{raw}")))?;
    if url.scheme() != "https" && url.scheme() != "http" {
        return Err(CatalogError::message(format!(
            "registry origin `{raw}` must be http(s)"
        )));
    }
    let host = url
        .host_str()
        .ok_or_else(|| CatalogError::message(format!("registry origin `{raw}` missing host")))?
        .to_ascii_lowercase();
    let port = url.port();
    let origin = match port {
        None => format!("{}://{host}", url.scheme()),
        Some(p) => format!("{}://{host}:{p}", url.scheme()),
    };
    Ok(origin)
}

/// Normalizes a static index URL (origin + path, no fragment, no trailing slash).
fn normalize_index_url(raw: &str) -> Result<String> {
    let mut url = url::Url::parse(raw)?;
    url.set_fragment(None);
    url.set_query(None);
    let mut s = url.as_str().to_string();
    if s.ends_with('/') && url.path() != "/" {
        s.pop();
    }
    Ok(s)
}

/// `file://` URL for an install directory (forward slashes, no trailing slash).
fn path_to_file_url(root: &Path) -> Result<String> {
    let abs = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| CatalogError::message(e.to_string()))?
            .join(root)
    };
    for comp in abs.components() {
        if matches!(comp, Component::ParentDir) {
            return Err(CatalogError::message(format!(
                "refusing plugin path with `..`: {}",
                root.display()
            )));
        }
    }
    let mut url = url::Url::from_file_path(&abs).map_err(|()| {
        CatalogError::message(format!(
            "cannot convert plugin path {} to file URL",
            abs.display()
        ))
    })?;
    // Drop trailing slash on the path except for filesystem roots.
    if url.path().ends_with('/') && url.path() != "/" {
        let mut p = url.path().trim_end_matches('/').to_string();
        if p.is_empty() {
            p = "/".into();
        }
        url.set_path(&p);
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_keys_are_stable_and_distinct_from_path() {
        let sqlite = PluginKey::platform("bookclerk-plugin-database-sqlite", "sqlite").unwrap();
        assert_eq!(
            sqlite.canonical(),
            "platform:bookclerk/bookclerk-plugin-database-sqlite#sqlite"
        );
        assert_eq!(sqlite.manifest_id(), "sqlite");
        assert!(is_platform_plugin_key(&sqlite));

        let fake = PluginKey::from_install_path(Path::new("/tmp/evil-sqlite"), "sqlite").unwrap();
        assert_eq!(fake.manifest_id(), "sqlite");
        assert_ne!(sqlite, fake);
        assert!(!is_platform_plugin_key(&fake));
    }

    #[test]
    fn cargo_and_npm_match_documented_forms() {
        let cargo = PackageCoordinate::parse("cargo:bookclerk-plugin-source-foo@1.2.3").unwrap();
        let key = PluginKey::from_coordinate(&cargo, "foo").unwrap();
        assert_eq!(
            key.canonical(),
            "cargo:https://crates.io/bookclerk-plugin-source-foo#foo"
        );
        assert_eq!(key.package(), "bookclerk-plugin-source-foo");

        let npm = PackageCoordinate::parse("npm:@author/foo@9.0.0").unwrap();
        let key = PluginKey::from_coordinate(&npm, "foo").unwrap();
        assert_eq!(
            key.canonical(),
            "npm:https://registry.npmjs.org/@author/foo#foo"
        );

        let pypi = PackageCoordinate::parse("pypi:foo==0.1.0").unwrap();
        let key = PluginKey::from_coordinate(&pypi, "foo").unwrap();
        assert_eq!(key.canonical(), "pypi:https://pypi.org/foo#foo");

        let reg = PackageCoordinate::parse(
            "registry:https://plugins.example/index.json#author/foo@1.0.0",
        )
        .unwrap();
        let key = PluginKey::from_coordinate(&reg, "foo").unwrap();
        assert_eq!(
            key.canonical(),
            "registry:https://plugins.example/index.json#author/foo#foo"
        );
    }

    #[test]
    fn version_is_not_part_of_plugin_key() {
        let a = PackageCoordinate::parse("cargo:bookclerk-plugin-source-foo@1.0.0").unwrap();
        let b = PackageCoordinate::parse("cargo:bookclerk-plugin-source-foo@9.9.9").unwrap();
        assert_eq!(
            PluginKey::from_coordinate(&a, "foo").unwrap(),
            PluginKey::from_coordinate(&b, "foo").unwrap()
        );
    }

    #[test]
    fn parse_round_trips() {
        let key = PluginKey::platform("bookclerk-plugin-destination-local", "local").unwrap();
        assert_eq!(PluginKey::parse(key.canonical()).unwrap(), key);
        assert_eq!(key.fs_id().len(), 19); // pk- + 16 hex
        assert!(key.fs_id().starts_with("pk-"));
    }

    #[test]
    fn same_manifest_id_two_registries_are_distinct() {
        let cargo = PackageCoordinate::parse("cargo:sqlite-impersonator@1.0.0").unwrap();
        let npm = PackageCoordinate::parse("npm:sqlite-impersonator@1.0.0").unwrap();
        let a = PluginKey::from_coordinate(&cargo, "sqlite").unwrap();
        let b = PluginKey::from_coordinate(&npm, "sqlite").unwrap();
        assert_ne!(a, b);
        assert_eq!(a.manifest_id(), "sqlite");
        assert_eq!(b.manifest_id(), "sqlite");
    }
}
