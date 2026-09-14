#![allow(dead_code, unused_imports)]

pub mod amp_code;
pub mod claude_code;
pub mod codex;
pub mod cursor;
pub mod opencode;
pub mod pi;
pub mod shared;

use crate::core::manifest::Manifest;
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub enum McpFormat {
    #[default]
    Json,
    Toml,
}

pub struct McpFile {
    pub path: String,
    pub content: String,
    pub merge_key: Option<String>,
    pub format: McpFormat,
}

pub type McpServers = serde_json::Map<String, serde_json::Value>;

pub trait Adapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn destination_path(&self, source: &str) -> Option<PathBuf>;
    fn mcp_config(&self, servers: &McpServers) -> Option<McpFile>;
    fn mcp_config_path(&self) -> Option<&'static str>;
    fn managed_files(&self, sources: &[String]) -> Vec<PathBuf> {
        sources
            .iter()
            .filter_map(|s| self.destination_path(s))
            .collect()
    }
}

static AMP_CODE: amp_code::AmpCode = amp_code::AmpCode;
static CLAUDE_CODE: claude_code::ClaudeCode = claude_code::ClaudeCode;
static CODEX: codex::CodexCli = codex::CodexCli;
static CURSOR: cursor::Cursor = cursor::Cursor;
static OPENCODE: opencode::OpenCode = opencode::OpenCode;
static PI: pi::PiCodingAgent = pi::PiCodingAgent;

static ADAPTERS: &[&'static dyn Adapter] = &[
    &AMP_CODE as &dyn Adapter,
    &CLAUDE_CODE as &dyn Adapter,
    &CODEX as &dyn Adapter,
    &CURSOR as &dyn Adapter,
    &OPENCODE as &dyn Adapter,
    &PI as &dyn Adapter,
];

pub fn get_adapter(name: &str) -> Option<&'static dyn Adapter> {
    ADAPTERS.iter().find(|a| a.name() == name).copied()
}

pub fn list_adapters() -> Vec<(&'static str, &'static str)> {
    ADAPTERS
        .iter()
        .map(|a| (a.name(), a.display_name()))
        .collect()
}

pub use shared::transform_env_vars;

/// Reads all per-skill mcp.json files for currently selected components and
/// merges them into a single object keyed by server name.
pub async fn collect_mcp_config(project_root: &Path, manifest: &Manifest) -> Result<McpServers> {
    let selected = match &manifest.selected_components {
        Some(s) => s,
        None => return Ok(McpServers::new()),
    };

    let mut merged = McpServers::new();

    if selected.skills.is_empty() {
        return Ok(merged);
    }

    let mut search_prefixes = Vec::new();
    for tool_name in &manifest.enabled_tools {
        let Some(adapter) = get_adapter(tool_name) else {
            continue;
        };
        if let Some(test_path) = adapter.destination_path("praxis/skills/test/mcp.json") {
            let test_str = test_path.to_string_lossy();
            if let Some(idx) = test_str.find("skills/") {
                search_prefixes.push(test_str[..idx + 7].to_string());
            }
        }
    }
    search_prefixes.push("praxis/skills/".to_string());

    for skill_name in &selected.skills {
        // Guard against path traversal
        if skill_name.contains("..") || skill_name.contains('/') {
            continue;
        }

        let mut found = None;
        for prefix in &search_prefixes {
            let mcp_path = project_root.join(prefix).join(skill_name).join("mcp.json");
            let expected_prefix = project_root.join(prefix);
            if !mcp_path.starts_with(&expected_prefix) {
                continue;
            }
            match tokio::fs::read_to_string(&mcp_path).await {
                Ok(raw) => match serde_json::from_str::<Value>(&raw) {
                    Ok(Value::Object(map)) => {
                        found = Some(map);
                        break;
                    }
                    Ok(_) => {
                        eprintln!(
                            "Warning: skipping invalid mcp.json for skill \"{}\": not an object",
                            skill_name
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: skipping invalid mcp.json for skill \"{}\": {}",
                            skill_name, e
                        );
                    }
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    eprintln!(
                        "Warning: error reading mcp.json for skill \"{}\": {}",
                        skill_name, e
                    );
                }
            }
        }

        if let Some(map) = found {
            for (k, v) in map {
                merged.insert(k, v);
            }
        }
    }

    Ok(merged)
}

/// Writes an MCP config file to disk.
/// Handles merge-key logic for files like opencode.json.
pub async fn write_mcp_config_file(full_path: &Path, entry: &McpFile) -> Result<()> {
    if let Some(parent) = full_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    if let Some(ref merge_key) = entry.merge_key {
        if full_path.exists() {
            match entry.format {
                McpFormat::Json => {
                    let existing_raw = tokio::fs::read_to_string(full_path)
                        .await
                        .unwrap_or_default();
                    let mut existing: Value = serde_json::from_str(&existing_raw)
                        .unwrap_or_else(|_| Value::Object(serde_json::Map::new()));
                    let new_content: Value = serde_json::from_str(&entry.content)
                        .with_context(|| "invalid JSON in generated MCP content")?;
                    if let Some(existing_obj) = existing.as_object_mut() {
                        if let Some(new_val) = new_content.get(merge_key) {
                            existing_obj.insert(merge_key.clone(), new_val.clone());
                            let raw = serde_json::to_string_pretty(&existing)? + "\n";
                            tokio::fs::write(full_path, raw).await?;
                            return Ok(());
                        }
                    }
                }
                McpFormat::Toml => {
                    let existing_raw = tokio::fs::read_to_string(full_path)
                        .await
                        .unwrap_or_default();
                    let mut existing: toml::Value = existing_raw
                        .parse()
                        .unwrap_or_else(|_| toml::Value::Table(toml::Table::new()));
                    let new_content: toml::Value = entry
                        .content
                        .parse()
                        .with_context(|| "invalid TOML in generated MCP content")?;
                    if let toml::Value::Table(existing_tbl) = &mut existing {
                        if let toml::Value::Table(new_tbl) = &new_content {
                            if let Some(new_val) = new_tbl.get(merge_key) {
                                existing_tbl.insert(merge_key.clone(), new_val.clone());
                                let raw = toml::to_string_pretty(&existing)? + "\n";
                                tokio::fs::write(full_path, raw).await?;
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }
    }

    tokio::fs::write(full_path, &entry.content).await?;
    Ok(())
}

/// Regenerates MCP config files for all enabled tools.
/// Returns the list of tool names that were regenerated.
pub async fn regenerate_tool_configs(
    project_root: &Path,
    manifest: &Manifest,
) -> Result<Vec<String>> {
    let enabled_tools = &manifest.enabled_tools;
    if enabled_tools.is_empty() {
        return Ok(Vec::new());
    }

    let mcp_config = collect_mcp_config(project_root, manifest).await?;
    let mut regenerated = Vec::new();

    for tool_name in enabled_tools {
        let Some(adapter) = get_adapter(tool_name) else {
            continue;
        };
        if let Some(entry) = adapter.mcp_config(&mcp_config) {
            let full_path = project_root.join(&entry.path);
            write_mcp_config_file(&full_path, &entry).await?;
        }
        regenerated.push(tool_name.clone());
    }

    Ok(regenerated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn test_get_adapter_known() {
        assert!(get_adapter("amp-code").is_some());
        assert!(get_adapter("claude-code").is_some());
        assert!(get_adapter("cursor").is_some());
        assert!(get_adapter("opencode").is_some());
    }

    #[test]
    fn test_get_adapter_unknown() {
        assert!(get_adapter("unknown").is_none());
    }

    #[test]
    fn test_list_adapters() {
        let adapters = list_adapters();
        assert_eq!(adapters.len(), 6);
        let names: Vec<_> = adapters.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"amp-code"));
        assert!(names.contains(&"claude-code"));
        assert!(names.contains(&"codex"));
        assert!(names.contains(&"cursor"));
        assert!(names.contains(&"opencode"));
        assert!(names.contains(&"pi-coding-agent"));
        for (_, display) in adapters {
            assert!(!display.is_empty());
        }
    }

    #[tokio::test]
    async fn test_collect_mcp_config_empty_skills() {
        let tmp = TempDir::new().unwrap();
        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec![],
            selected_components: Some(crate::core::manifest::SelectedComponents {
                skills: vec![],
                reviewers: vec![],
            }),
            files: HashMap::new(),
        };
        let result = collect_mcp_config(tmp.path(), &manifest).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_collect_mcp_config_no_selected_components() {
        let tmp = TempDir::new().unwrap();
        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec![],
            selected_components: None,
            files: HashMap::new(),
        };
        let result = collect_mcp_config(tmp.path(), &manifest).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_collect_mcp_config_reads_and_merges() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::create_dir_all(tmp.path().join("praxis/skills/figma-to-code"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(tmp.path().join("praxis/skills/mobile-mcp"))
            .await
            .unwrap();

        tokio::fs::write(
            tmp.path().join("praxis/skills/figma-to-code/mcp.json"),
            r#"{"figma":{"command":"npx","args":["-y","figma-developer-mcp","--stdio"],"env":{"FIGMA_API_KEY":"${FIGMA_API_KEY}"}}}"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            tmp.path().join("praxis/skills/mobile-mcp/mcp.json"),
            r#"{"mobile-mcp":{"command":"npx","args":["-y","@mobilenext/mobile-mcp@latest"]}}"#,
        )
        .await
        .unwrap();

        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec![],
            selected_components: Some(crate::core::manifest::SelectedComponents {
                skills: vec!["figma-to-code".to_string(), "mobile-mcp".to_string()],
                reviewers: vec![],
            }),
            files: HashMap::new(),
        };
        let result = collect_mcp_config(tmp.path(), &manifest).await.unwrap();
        assert!(result.contains_key("figma"));
        assert!(result.contains_key("mobile-mcp"));
    }

    #[tokio::test]
    async fn test_collect_mcp_config_skips_malformed() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::create_dir_all(tmp.path().join("praxis/skills/bad-skill"))
            .await
            .unwrap();
        tokio::fs::write(
            tmp.path().join("praxis/skills/bad-skill/mcp.json"),
            "not valid json {",
        )
        .await
        .unwrap();

        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec![],
            selected_components: Some(crate::core::manifest::SelectedComponents {
                skills: vec!["bad-skill".to_string()],
                reviewers: vec![],
            }),
            files: HashMap::new(),
        };
        let result = collect_mcp_config(tmp.path(), &manifest).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_collect_mcp_config_skips_path_traversal() {
        let tmp = TempDir::new().unwrap();
        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec![],
            selected_components: Some(crate::core::manifest::SelectedComponents {
                skills: vec!["../../etc".to_string()],
                reviewers: vec![],
            }),
            files: HashMap::new(),
        };
        let result = collect_mcp_config(tmp.path(), &manifest).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_collect_mcp_config_reads_from_tool_dest() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::create_dir_all(tmp.path().join(".agents/skills/figma-to-code"))
            .await
            .unwrap();
        tokio::fs::write(
            tmp.path().join(".agents/skills/figma-to-code/mcp.json"),
            r#"{"figma":{"command":"npx","args":["-y","figma-mcp"],"env":{}}}"#,
        )
        .await
        .unwrap();

        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec!["amp-code".to_string()],
            selected_components: Some(crate::core::manifest::SelectedComponents {
                skills: vec!["figma-to-code".to_string()],
                reviewers: vec![],
            }),
            files: HashMap::new(),
        };
        let result = collect_mcp_config(tmp.path(), &manifest).await.unwrap();
        assert!(result.contains_key("figma"));
        assert_eq!(result["figma"]["command"], "npx");
    }

    #[tokio::test]
    async fn test_regenerate_tool_configs_empty() {
        let tmp = TempDir::new().unwrap();
        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec![],
            selected_components: Some(crate::core::manifest::SelectedComponents {
                skills: vec![],
                reviewers: vec![],
            }),
            files: HashMap::new(),
        };
        let result = regenerate_tool_configs(tmp.path(), &manifest)
            .await
            .unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_regenerate_tool_configs_writes_cursor() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::create_dir_all(tmp.path().join("praxis/skills/figma-to-code"))
            .await
            .unwrap();
        tokio::fs::write(
            tmp.path().join("praxis/skills/figma-to-code/mcp.json"),
            r#"{"figma":{"command":"npx","args":["-y","figma"],"env":{"K":"${V}"}}}"#,
        )
        .await
        .unwrap();

        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec!["cursor".to_string()],
            selected_components: Some(crate::core::manifest::SelectedComponents {
                skills: vec!["figma-to-code".to_string()],
                reviewers: vec![],
            }),
            files: HashMap::new(),
        };
        regenerate_tool_configs(tmp.path(), &manifest)
            .await
            .unwrap();
        let content = tokio::fs::read_to_string(tmp.path().join(".cursor/mcp.json"))
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["mcpServers"]["figma"]["env"]["K"], "${env:V}");
    }

    #[tokio::test]
    async fn test_regenerate_tool_configs_merge_opencode() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(
            tmp.path().join("opencode.json"),
            r#"{"provider":{"default":"anthropic"}}"#,
        )
        .await
        .unwrap();

        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec!["opencode".to_string()],
            selected_components: Some(crate::core::manifest::SelectedComponents {
                skills: vec![],
                reviewers: vec![],
            }),
            files: HashMap::new(),
        };
        regenerate_tool_configs(tmp.path(), &manifest)
            .await
            .unwrap();
        let content = tokio::fs::read_to_string(tmp.path().join("opencode.json"))
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["provider"]["default"], "anthropic");
        assert!(parsed.get("mcp").is_some());
    }

    #[tokio::test]
    async fn test_regenerate_tool_configs_overwrite_invalid_opencode() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("opencode.json"), "not json {")
            .await
            .unwrap();

        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec!["opencode".to_string()],
            selected_components: Some(crate::core::manifest::SelectedComponents {
                skills: vec![],
                reviewers: vec![],
            }),
            files: HashMap::new(),
        };
        regenerate_tool_configs(tmp.path(), &manifest)
            .await
            .unwrap();
        let content = tokio::fs::read_to_string(tmp.path().join("opencode.json"))
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&content).unwrap();
        assert!(parsed.get("mcp").is_some());
    }

    #[tokio::test]
    async fn test_regenerate_tool_configs_skips_unknown() {
        let tmp = TempDir::new().unwrap();
        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec!["nonexistent".to_string()],
            selected_components: Some(crate::core::manifest::SelectedComponents {
                skills: vec![],
                reviewers: vec![],
            }),
            files: HashMap::new(),
        };
        let result = regenerate_tool_configs(tmp.path(), &manifest)
            .await
            .unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_regenerate_tool_configs_returns_list() {
        let tmp = TempDir::new().unwrap();
        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec!["cursor".to_string(), "opencode".to_string()],
            selected_components: Some(crate::core::manifest::SelectedComponents {
                skills: vec![],
                reviewers: vec![],
            }),
            files: HashMap::new(),
        };
        let result = regenerate_tool_configs(tmp.path(), &manifest)
            .await
            .unwrap();
        assert_eq!(result, vec!["cursor", "opencode"]);
    }

    #[tokio::test]
    async fn test_regenerate_tool_configs_includes_amp_code() {
        let tmp = TempDir::new().unwrap();
        let manifest = Manifest {
            version: "1.0.0".to_string(),
            installed_at: "2025-01-01T00:00:00.000Z".to_string(),
            updated_at: "2025-01-01T00:00:00.000Z".to_string(),
            enabled_tools: vec!["amp-code".to_string()],
            selected_components: Some(crate::core::manifest::SelectedComponents {
                skills: vec![],
                reviewers: vec![],
            }),
            files: HashMap::new(),
        };
        let result = regenerate_tool_configs(tmp.path(), &manifest)
            .await
            .unwrap();
        assert_eq!(result, vec!["amp-code"]);
    }
}
