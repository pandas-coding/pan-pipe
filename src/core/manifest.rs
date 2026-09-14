#![allow(dead_code)]

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;

const MANIFEST_FILE: &str = ".praxis-manifest.json";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: String,
    #[serde(default)]
    pub installed_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub enabled_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_components: Option<SelectedComponents>,
    #[serde(default)]
    pub files: HashMap<String, FileEntry>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SelectedComponents {
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub reviewers: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct FileEntry {
    pub hash: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub destinations: HashMap<String, String>,
}

pub fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub async fn hash_file(path: impl AsRef<Path>) -> Result<String> {
    let content = fs::read_to_string(path.as_ref())
        .await
        .with_context(|| format!("failed to read {:?}", path.as_ref()))?;
    Ok(hash_content(&content))
}

pub async fn read_manifest(project_root: impl AsRef<Path>) -> Result<Option<Manifest>> {
    let path = project_root.as_ref().join(MANIFEST_FILE);
    match fs::read_to_string(&path).await {
        Ok(raw) => {
            let manifest: Manifest = serde_json::from_str(&raw)
                .with_context(|| format!("invalid JSON in manifest at {:?}", path))?;
            if manifest.enabled_tools.is_empty() && !raw.contains("enabledTools") {
                // Defensive: serde default already handles this, but keep explicit
                // logic aligned with JS "defaults enabledTools to [] for old manifests"
            }
            Ok(Some(manifest))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub async fn write_manifest(project_root: impl AsRef<Path>, manifest: &Manifest) -> Result<()> {
    let path = project_root.as_ref().join(MANIFEST_FILE);
    let tmp_path = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
    let raw = serde_json::to_string_pretty(manifest)? + "\n";
    fs::write(&tmp_path, raw)
        .await
        .with_context(|| format!("failed to write temp manifest {:?}", tmp_path))?;
    fs::rename(&tmp_path, &path)
        .await
        .with_context(|| format!("failed to rename manifest to {:?}", path))?;
    Ok(())
}

pub async fn is_locally_modified(
    project_root: &Path,
    relative_path: &str,
    manifest: &Manifest,
) -> bool {
    let Some(entry) = manifest.files.get(relative_path) else {
        return false;
    };
    match hash_file(project_root.join(relative_path)).await {
        Ok(current_hash) => current_hash != entry.hash,
        Err(_) => true,
    }
}

pub async fn is_destination_modified(
    project_root: &Path,
    destination_path: &str,
    source_hash: &str,
) -> bool {
    match hash_file(project_root.join(destination_path)).await {
        Ok(current_hash) => current_hash != source_hash,
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_hash_content_is_64_char_hex() {
        let hash = hash_content("hello");
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_hash_content_deterministic() {
        assert_eq!(hash_content("hello"), hash_content("hello"));
    }

    #[test]
    fn test_hash_content_different_inputs() {
        assert_ne!(hash_content("hello"), hash_content("world"));
    }

    #[tokio::test]
    async fn test_hash_file_matches_hash_content() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.txt");
        fs::write(&path, "some content").await.unwrap();
        assert_eq!(
            hash_file(&path).await.unwrap(),
            hash_content("some content")
        );
    }

    #[tokio::test]
    async fn test_read_manifest_exists() {
        let tmp = TempDir::new().unwrap();
        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec![],
            selected_components: None,
            files: HashMap::new(),
        };
        fs::write(
            tmp.path().join(MANIFEST_FILE),
            serde_json::to_string(&manifest).unwrap(),
        )
        .await
        .unwrap();
        let result = read_manifest(tmp.path()).await.unwrap();
        assert_eq!(result, Some(manifest));
    }

    #[tokio::test]
    async fn test_read_manifest_defaults_enabled_tools() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join(MANIFEST_FILE),
            r#"{"version":"1","files":{}}"#,
        )
        .await
        .unwrap();
        let result = read_manifest(tmp.path()).await.unwrap().unwrap();
        assert!(result.enabled_tools.is_empty());
    }

    #[tokio::test]
    async fn test_read_manifest_preserves_enabled_tools() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join(MANIFEST_FILE),
            r#"{"version":"1","files":{},"enabledTools":["claude-code"]}"#,
        )
        .await
        .unwrap();
        let result = read_manifest(tmp.path()).await.unwrap().unwrap();
        assert_eq!(result.enabled_tools, vec!["claude-code"]);
    }

    #[tokio::test]
    async fn test_read_manifest_missing() {
        let tmp = TempDir::new().unwrap();
        assert!(read_manifest(tmp.path()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_read_manifest_invalid_json() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(MANIFEST_FILE), "{not valid json")
            .await
            .unwrap();
        assert!(read_manifest(tmp.path()).await.is_err());
    }

    #[tokio::test]
    async fn test_write_manifest_trailing_newline() {
        let tmp = TempDir::new().unwrap();
        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec![],
            selected_components: None,
            files: HashMap::new(),
        };
        write_manifest(tmp.path(), &manifest).await.unwrap();
        let raw = fs::read_to_string(tmp.path().join(MANIFEST_FILE))
            .await
            .unwrap();
        assert!(raw.ends_with('\n'));
        assert_eq!(serde_json::from_str::<Manifest>(&raw).unwrap(), manifest);
    }

    #[tokio::test]
    async fn test_write_manifest_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec![],
            selected_components: None,
            files: {
                let mut m = HashMap::new();
                m.insert(
                    "a.txt".to_string(),
                    FileEntry {
                        hash: "abc".to_string(),
                        destinations: HashMap::new(),
                    },
                );
                m
            },
        };
        write_manifest(tmp.path(), &manifest).await.unwrap();
        let read_back = read_manifest(tmp.path()).await.unwrap().unwrap();
        assert_eq!(read_back, manifest);
    }

    #[tokio::test]
    async fn test_is_locally_modified_missing_entry() {
        let tmp = TempDir::new().unwrap();
        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec![],
            selected_components: None,
            files: HashMap::new(),
        };
        assert!(!is_locally_modified(tmp.path(), "missing.txt", &manifest).await);
    }

    #[tokio::test]
    async fn test_is_locally_modified_matching_hash() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "original content")
            .await
            .unwrap();
        let hash = hash_content("original content");
        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec![],
            selected_components: None,
            files: {
                let mut m = HashMap::new();
                m.insert(
                    "file.txt".to_string(),
                    FileEntry {
                        hash,
                        destinations: HashMap::new(),
                    },
                );
                m
            },
        };
        assert!(!is_locally_modified(tmp.path(), "file.txt", &manifest).await);
    }

    #[tokio::test]
    async fn test_is_locally_modified_different_hash() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "modified content")
            .await
            .unwrap();
        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec![],
            selected_components: None,
            files: {
                let mut m = HashMap::new();
                m.insert(
                    "file.txt".to_string(),
                    FileEntry {
                        hash: "oldhash".to_string(),
                        destinations: HashMap::new(),
                    },
                );
                m
            },
        };
        assert!(is_locally_modified(tmp.path(), "file.txt", &manifest).await);
    }

    #[tokio::test]
    async fn test_is_locally_modified_missing_file() {
        let tmp = TempDir::new().unwrap();
        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec![],
            selected_components: None,
            files: {
                let mut m = HashMap::new();
                m.insert(
                    "gone.txt".to_string(),
                    FileEntry {
                        hash: "somehash".to_string(),
                        destinations: HashMap::new(),
                    },
                );
                m
            },
        };
        assert!(is_locally_modified(tmp.path(), "gone.txt", &manifest).await);
    }

    #[tokio::test]
    async fn test_is_destination_modified_matching() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "original content")
            .await
            .unwrap();
        let hash = hash_content("original content");
        assert!(!is_destination_modified(tmp.path(), "file.txt", &hash).await);
    }

    #[tokio::test]
    async fn test_is_destination_modified_different() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "modified content")
            .await
            .unwrap();
        assert!(is_destination_modified(tmp.path(), "file.txt", "oldhash").await);
    }

    #[tokio::test]
    async fn test_is_destination_modified_missing() {
        let tmp = TempDir::new().unwrap();
        assert!(is_destination_modified(tmp.path(), "missing.txt", "somehash").await);
    }
}
