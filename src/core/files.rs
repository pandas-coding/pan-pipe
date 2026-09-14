#![allow(dead_code)]

use crate::core::manifest::hash_content;
use crate::core::prompt::{PromptOption, PromptResult, Prompter};
use anyhow::Result;
use similar::TextDiff;
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;

/// Returns true if resolved_path is safely within resolved_root.
pub fn is_safe_path(resolved_root: &Path, resolved_path: &Path) -> bool {
    resolved_path.starts_with(resolved_root)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallStatus {
    Written { hash: String },
    Matched { hash: String },
    Skipped { hash: String },
    Cancelled,
}

pub async fn install_file(
    full_path: &Path,
    relative_path: &str,
    content: &str,
    prompter: &mut dyn Prompter,
) -> Result<InstallStatus> {
    if full_path.exists() {
        let existing_content = fs::read_to_string(full_path).await?;

        if existing_content == content {
            return Ok(InstallStatus::Matched {
                hash: hash_content(content),
            });
        }

        let options = vec![
            PromptOption {
                value: "overwrite".to_string(),
                label: "Overwrite with Praxis version".to_string(),
            },
            PromptOption {
                value: "skip".to_string(),
                label: "Skip this file".to_string(),
            },
            PromptOption {
                value: "diff".to_string(),
                label: "Show diff, then decide".to_string(),
            },
        ];

        let mut action = prompter
            .select(
                &format!(
                    "{} already exists and differs. What would you like to do?",
                    relative_path
                ),
                &options,
            )
            .await?;

        if prompter.is_cancelled(&action) {
            return Ok(InstallStatus::Cancelled);
        }

        let mut action_str = match &action {
            PromptResult::Value(v) => v.as_str(),
            _ => return Ok(InstallStatus::Cancelled),
        };

        if action_str == "diff" {
            let diff = TextDiff::from_lines(existing_content.as_str(), content);
            let patch = diff
                .unified_diff()
                .header("your version", "praxis")
                .to_string();
            prompter.log_info(&patch);

            let options2 = vec![
                PromptOption {
                    value: "overwrite".to_string(),
                    label: "Overwrite with Praxis version".to_string(),
                },
                PromptOption {
                    value: "skip".to_string(),
                    label: "Skip this file".to_string(),
                },
            ];
            action = prompter
                .select(&format!("Overwrite {}?", relative_path), &options2)
                .await?;

            if prompter.is_cancelled(&action) {
                return Ok(InstallStatus::Cancelled);
            }

            action_str = match &action {
                PromptResult::Value(v) => v.as_str(),
                _ => return Ok(InstallStatus::Cancelled),
            };
        }

        if action_str == "skip" {
            return Ok(InstallStatus::Skipped {
                hash: hash_content(&existing_content),
            });
        }

        // overwrite
    }

    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    fs::write(full_path, content).await?;
    Ok(InstallStatus::Written {
        hash: hash_content(content),
    })
}

pub trait DestinationResolver: Send + Sync {
    fn destination_path(&self, source: &str) -> Option<std::path::PathBuf>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallResult {
    pub hash: String,
    pub destinations: HashMap<String, String>,
}

pub async fn install_to_destinations(
    project_root: &Path,
    resolved_root: &Path,
    source_file: &str,
    content: &str,
    enabled_tools: &[String],
    adapters: &HashMap<String, Box<dyn DestinationResolver>>,
) -> Result<InstallResult> {
    let hash = hash_content(content);
    let mut destinations = HashMap::new();

    for tool_name in enabled_tools {
        let Some(adapter) = adapters.get(tool_name) else {
            continue;
        };
        let Some(dest_path) = adapter.destination_path(source_file) else {
            continue;
        };
        let full_path = project_root.join(&dest_path);
        if !is_safe_path(resolved_root, &full_path) {
            continue;
        }
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&full_path, content).await?;
        destinations.insert(tool_name.clone(), dest_path.to_string_lossy().to_string());
    }

    Ok(InstallResult { hash, destinations })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::prompt::ScriptPrompter;
    use tempfile::TempDir;

    struct MockAdapter {
        mapping: HashMap<String, String>,
    }

    impl DestinationResolver for MockAdapter {
        fn destination_path(&self, source: &str) -> Option<std::path::PathBuf> {
            self.mapping.get(source).map(|s| s.into())
        }
    }

    #[tokio::test]
    async fn test_install_file_writes_new() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("new.md");
        let mut prompter = ScriptPrompter::new(vec![]);
        let result = install_file(&path, "new.md", "# Content", &mut prompter)
            .await
            .unwrap();
        assert!(matches!(result, InstallStatus::Written { .. }));
        assert_eq!(fs::read_to_string(&path).await.unwrap(), "# Content");
    }

    #[tokio::test]
    async fn test_install_file_matched() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("existing.md");
        fs::write(&path, "# Content").await.unwrap();
        let mut prompter = ScriptPrompter::new(vec![]);
        let result = install_file(&path, "existing.md", "# Content", &mut prompter)
            .await
            .unwrap();
        assert!(matches!(result, InstallStatus::Matched { .. }));
    }

    #[tokio::test]
    async fn test_install_file_overwrite() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("conflict.md");
        fs::write(&path, "old content").await.unwrap();
        let mut prompter = ScriptPrompter::new(vec!["overwrite".to_string()]);
        let result = install_file(&path, "conflict.md", "new content", &mut prompter)
            .await
            .unwrap();
        assert!(matches!(result, InstallStatus::Written { .. }));
        assert_eq!(fs::read_to_string(&path).await.unwrap(), "new content");
    }

    #[tokio::test]
    async fn test_install_file_skip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("conflict.md");
        fs::write(&path, "old content").await.unwrap();
        let mut prompter = ScriptPrompter::new(vec!["skip".to_string()]);
        let result = install_file(&path, "conflict.md", "new content", &mut prompter)
            .await
            .unwrap();
        assert!(matches!(result, InstallStatus::Skipped { .. }));
        assert_eq!(fs::read_to_string(&path).await.unwrap(), "old content");
    }

    #[tokio::test]
    async fn test_install_file_diff_then_overwrite() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("conflict.md");
        fs::write(&path, "old content").await.unwrap();
        let mut prompter = ScriptPrompter::new(vec!["diff".to_string(), "overwrite".to_string()]);
        let result = install_file(&path, "conflict.md", "new content", &mut prompter)
            .await
            .unwrap();
        assert!(matches!(result, InstallStatus::Written { .. }));
        assert_eq!(fs::read_to_string(&path).await.unwrap(), "new content");
        assert!(prompter.logs.iter().any(|l| l.contains("---")));
    }

    #[tokio::test]
    async fn test_install_file_diff_then_skip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("conflict.md");
        fs::write(&path, "old content").await.unwrap();
        let mut prompter = ScriptPrompter::new(vec!["diff".to_string(), "skip".to_string()]);
        let result = install_file(&path, "conflict.md", "new content", &mut prompter)
            .await
            .unwrap();
        assert!(matches!(result, InstallStatus::Skipped { .. }));
        assert_eq!(fs::read_to_string(&path).await.unwrap(), "old content");
    }

    #[tokio::test]
    async fn test_install_file_cancelled_first() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("conflict.md");
        fs::write(&path, "old content").await.unwrap();
        let mut prompter = ScriptPrompter::new(vec![]);
        let result = install_file(&path, "conflict.md", "new content", &mut prompter)
            .await
            .unwrap();
        assert_eq!(result, InstallStatus::Cancelled);
    }

    #[tokio::test]
    async fn test_install_file_cancelled_after_diff() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("conflict.md");
        fs::write(&path, "old content").await.unwrap();
        let mut prompter = ScriptPrompter::new(vec!["diff".to_string()]);
        let result = install_file(&path, "conflict.md", "new content", &mut prompter)
            .await
            .unwrap();
        assert_eq!(result, InstallStatus::Cancelled);
    }

    #[tokio::test]
    async fn test_install_to_destinations_skips_unknown_tool() {
        let tmp = TempDir::new().unwrap();
        let adapters: HashMap<String, Box<dyn DestinationResolver>> = HashMap::new();
        let result = install_to_destinations(
            tmp.path(),
            tmp.path(),
            "praxis/test.md",
            "content",
            &["unknown-tool".to_string()],
            &adapters,
        )
        .await
        .unwrap();
        assert_eq!(result.hash, hash_content("content"));
        assert!(result.destinations.is_empty());
    }

    #[tokio::test]
    async fn test_install_to_destinations_skips_no_mapping() {
        let tmp = TempDir::new().unwrap();
        let mut adapters: HashMap<String, Box<dyn DestinationResolver>> = HashMap::new();
        adapters.insert(
            "amp-code".to_string(),
            Box::new(MockAdapter {
                mapping: HashMap::new(),
            }),
        );
        let result = install_to_destinations(
            tmp.path(),
            tmp.path(),
            "not-praxis/test.md",
            "content",
            &["amp-code".to_string()],
            &adapters,
        )
        .await
        .unwrap();
        assert!(result.destinations.is_empty());
    }

    #[tokio::test]
    async fn test_install_to_destinations_skips_unsafe_path() {
        let tmp = TempDir::new().unwrap();
        let subdir = tmp.path().join("subdir");
        fs::create_dir(&subdir).await.unwrap();
        let mut adapters: HashMap<String, Box<dyn DestinationResolver>> = HashMap::new();
        adapters.insert(
            "amp-code".to_string(),
            Box::new(MockAdapter {
                mapping: {
                    let mut m = HashMap::new();
                    m.insert(
                        "praxis/conventions.md".to_string(),
                        "../outside.md".to_string(),
                    );
                    m
                },
            }),
        );
        let result = install_to_destinations(
            tmp.path(),
            &subdir.canonicalize().unwrap_or_else(|_| subdir.clone()),
            "praxis/conventions.md",
            "# Core",
            &["amp-code".to_string()],
            &adapters,
        )
        .await
        .unwrap();
        assert!(result.destinations.is_empty());
    }

    #[tokio::test]
    async fn test_install_to_destinations_valid() {
        let tmp = TempDir::new().unwrap();
        let mut adapters: HashMap<String, Box<dyn DestinationResolver>> = HashMap::new();
        adapters.insert(
            "amp-code".to_string(),
            Box::new(MockAdapter {
                mapping: {
                    let mut m = HashMap::new();
                    m.insert(
                        "praxis/conventions.md".to_string(),
                        ".agents/conventions.md".to_string(),
                    );
                    m
                },
            }),
        );
        let result = install_to_destinations(
            tmp.path(),
            tmp.path(),
            "praxis/conventions.md",
            "# Core",
            &["amp-code".to_string()],
            &adapters,
        )
        .await
        .unwrap();
        assert_eq!(
            result.destinations.get("amp-code"),
            Some(&".agents/conventions.md".to_string())
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join(".agents/conventions.md"))
                .await
                .unwrap(),
            "# Core"
        );
    }
}
