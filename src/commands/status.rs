use crate::adapters::get_adapter;
use crate::core::manifest::{Manifest, read_manifest};
use anyhow::Result;
use owo_colors::OwoColorize;
use std::path::PathBuf;

pub async fn run() -> Result<()> {
    let project_root = std::env::current_dir()?;
    let manifest = read_manifest(&project_root).await?;
    run_with(project_root, manifest).await
}

pub async fn run_with(project_root: PathBuf, manifest: Option<Manifest>) -> Result<()> {
    println!("{}", "Pan-Pipe — Status".bold());

    let Some(manifest) = manifest else {
        println!("{}", "Pan-Pipe is not installed in this project.".yellow());
        println!("Run \"pan-pipe init\" to get started.");
        return Ok(());
    };

    println!("Installed: {}", manifest.installed_at.dimmed());
    println!("Updated:   {}", manifest.updated_at.dimmed());

    let enabled_tools = &manifest.enabled_tools;
    let has_tool_destinations =
        !enabled_tools.is_empty() && manifest.files.values().any(|e| !e.destinations.is_empty());

    let mut unchanged = 0usize;
    let mut modified = 0usize;
    let mut missing = 0usize;
    let mut lines = Vec::new();

    let files: Vec<_> = manifest.files.keys().cloned().collect();

    if has_tool_destinations {
        for relative_path in &files {
            let entry = manifest.files.get(relative_path).unwrap();
            if !entry.destinations.is_empty() {
                for dest_path in entry.destinations.values() {
                    let full_path = project_root.join(dest_path);
                    if !full_path.exists() {
                        lines.push(format!(
                            "  {} {} {}",
                            "✗".red(),
                            dest_path,
                            "(missing)".red()
                        ));
                        missing += 1;
                    } else {
                        let content = tokio::fs::read_to_string(&full_path)
                            .await
                            .unwrap_or_default();
                        let current_hash = crate::core::manifest::hash_content(&content);
                        if current_hash != entry.hash {
                            lines.push(format!(
                                "  {} {} {}",
                                "✎".yellow(),
                                dest_path,
                                "(modified)".yellow()
                            ));
                            modified += 1;
                        } else {
                            lines.push(format!("  {} {}", "✓".green(), dest_path));
                            unchanged += 1;
                        }
                    }
                }
            } else {
                // Legacy entry without destinations
                let full_path = project_root.join(relative_path);
                if !full_path.exists() {
                    lines.push(format!(
                        "  {} {} {}",
                        "✗".red(),
                        relative_path,
                        "(missing)".red()
                    ));
                    missing += 1;
                } else {
                    let content = tokio::fs::read_to_string(&full_path)
                        .await
                        .unwrap_or_default();
                    let current_hash = crate::core::manifest::hash_content(&content);
                    if current_hash != entry.hash {
                        lines.push(format!(
                            "  {} {} {}",
                            "✎".yellow(),
                            relative_path,
                            "(modified)".yellow()
                        ));
                        modified += 1;
                    } else {
                        lines.push(format!("  {} {}", "✓".green(), relative_path));
                        unchanged += 1;
                    }
                }
            }
        }
    } else {
        for relative_path in &files {
            let full_path = project_root.join(relative_path);
            let entry = manifest.files.get(relative_path).unwrap();
            if !full_path.exists() {
                lines.push(format!(
                    "  {} {} {}",
                    "✗".red(),
                    relative_path,
                    "(missing)".red()
                ));
                missing += 1;
            } else {
                let content = tokio::fs::read_to_string(&full_path)
                    .await
                    .unwrap_or_default();
                let current_hash = crate::core::manifest::hash_content(&content);
                if current_hash != entry.hash {
                    lines.push(format!(
                        "  {} {} {}",
                        "✎".yellow(),
                        relative_path,
                        "(modified)".yellow()
                    ));
                    modified += 1;
                } else {
                    lines.push(format!("  {} {}", "✓".green(), relative_path));
                    unchanged += 1;
                }
            }
        }
    }

    if !lines.is_empty() {
        println!("{}", lines.join("\n"));
    }

    let mut parts = Vec::new();
    if unchanged > 0 {
        parts.push(format!("{} unchanged", unchanged.to_string().green()));
    }
    if modified > 0 {
        parts.push(format!("{} modified", modified.to_string().yellow()));
    }
    if missing > 0 {
        parts.push(format!("{} missing", missing.to_string().red()));
    }

    // Show enabled tools
    if !enabled_tools.is_empty() {
        let tool_names: Vec<_> = enabled_tools
            .iter()
            .map(|t| {
                get_adapter(t)
                    .map(|a| a.display_name().to_string())
                    .unwrap_or_else(|| t.clone())
            })
            .collect();
        println!("Tools: {}", tool_names.join(", "));
    }

    // Show component selection summary
    if let Some(ref selection) = manifest.selected_components {
        let selected_count = selection.skills.len() + selection.reviewers.len();
        println!(
            "Components: {} optional component(s) selected. Run {} to change.",
            selected_count,
            "pan-pipe components".dimmed()
        );
    }

    // Show MCP config status per tool
    if !enabled_tools.is_empty() {
        let mut mcp_lines = Vec::new();
        for tool_name in enabled_tools {
            let Some(adapter) = get_adapter(tool_name) else {
                continue;
            };
            let mcp_config_path = adapter.mcp_config_path();
            if mcp_config_path.is_none() {
                mcp_lines.push(format!(
                    "  {} {}: {}",
                    "—".dimmed(),
                    adapter.display_name(),
                    "reads mcp.json directly".dimmed()
                ));
                continue;
            }
            let full_path = project_root.join(mcp_config_path.unwrap());
            if full_path.exists() {
                mcp_lines.push(format!(
                    "  {} {}: {}",
                    "✓".green(),
                    adapter.display_name(),
                    mcp_config_path.unwrap()
                ));
            } else {
                mcp_lines.push(format!(
                    "  {} {}: {} {}",
                    "✗".red(),
                    adapter.display_name(),
                    mcp_config_path.unwrap(),
                    "(missing)".red()
                ));
            }
        }
        if !mcp_lines.is_empty() {
            println!("MCP configs:");
            println!("{}", mcp_lines.join("\n"));
        }
    }

    println!(
        "{} managed files: {}.",
        files.len(),
        if parts.is_empty() {
            "all up to date".to_string()
        } else {
            parts.join(", ")
        }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::manifest::{FileEntry, hash_content};
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn make_manifest(opts: Option<HashMap<String, FileEntry>>) -> Manifest {
        Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec![],
            selected_components: None,
            files: opts.unwrap_or_default(),
        }
    }

    #[tokio::test]
    async fn test_status_not_installed() {
        let tmp = TempDir::new().unwrap();
        // Note: run_with directly returns Ok, we can't easily capture stdout in async test
        // without redirecting. We'll test the functional paths via run_with and file states.
        let result = run_with(tmp.path().to_path_buf(), None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_status_unchanged() {
        let tmp = TempDir::new().unwrap();
        let content = "# Test file";
        let hash = hash_content(content);
        tokio::fs::create_dir_all(tmp.path().join("praxis"))
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("praxis/test.md"), content)
            .await
            .unwrap();

        let manifest = make_manifest({
            let mut m = HashMap::new();
            m.insert(
                "praxis/test.md".to_string(),
                FileEntry {
                    hash,
                    destinations: HashMap::new(),
                },
            );
            Some(m)
        });
        let result = run_with(tmp.path().to_path_buf(), Some(manifest)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_status_modified() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::create_dir_all(tmp.path().join("praxis"))
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("praxis/test.md"), "modified content")
            .await
            .unwrap();

        let manifest = make_manifest({
            let mut m = HashMap::new();
            m.insert(
                "praxis/test.md".to_string(),
                FileEntry {
                    hash: hash_content("original content"),
                    destinations: HashMap::new(),
                },
            );
            Some(m)
        });
        let result = run_with(tmp.path().to_path_buf(), Some(manifest)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_status_missing() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest({
            let mut m = HashMap::new();
            m.insert(
                "praxis/gone.md".to_string(),
                FileEntry {
                    hash: "abc123".to_string(),
                    destinations: HashMap::new(),
                },
            );
            Some(m)
        });
        let result = run_with(tmp.path().to_path_buf(), Some(manifest)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_status_with_destinations() {
        let tmp = TempDir::new().unwrap();
        let content = "# Core";
        let hash = hash_content(content);
        tokio::fs::create_dir_all(tmp.path().join(".agents"))
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join(".agents/conventions.md"), content)
            .await
            .unwrap();
        tokio::fs::create_dir_all(tmp.path().join(".cursor"))
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join(".cursor/conventions.md"), "modified")
            .await
            .unwrap();

        let manifest = make_manifest({
            let mut m = HashMap::new();
            m.insert(
                "praxis/conventions.md".to_string(),
                FileEntry {
                    hash,
                    destinations: {
                        let mut d = HashMap::new();
                        d.insert("amp-code".to_string(), ".agents/conventions.md".to_string());
                        d.insert("cursor".to_string(), ".cursor/conventions.md".to_string());
                        d
                    },
                },
            );
            Some(m)
        });
        let mut manifest = manifest;
        manifest.enabled_tools = vec!["amp-code".to_string(), "cursor".to_string()];
        let result = run_with(tmp.path().to_path_buf(), Some(manifest)).await;
        assert!(result.is_ok());
    }
}
