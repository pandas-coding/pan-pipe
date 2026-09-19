use super::{Adapter, McpFile, McpFormat, McpServers, transform_env_vars};
use serde_json::Value;
use std::path::PathBuf;

pub struct CodexCli;

impl Adapter for CodexCli {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn display_name(&self) -> &'static str {
        "Codex CLI"
    }

    fn destination_path(&self, source: &str) -> Option<PathBuf> {
        source
            .strip_prefix("praxis/")
            .map(|r| format!(".codex/{}", r).into())
    }

    fn mcp_config(&self, servers: &McpServers) -> Option<McpFile> {
        let mut lines = vec!["[mcp_servers]".to_string()];
        for (name, entry) in servers {
            lines.push(format!("[mcp_servers.\"{}\"]", name));
            if let Value::Object(map) = entry {
                if let Some(Value::String(cmd)) = map.get("command") {
                    lines.push(format!("command = \"{}\"", cmd));
                }
                if let Some(Value::Array(args)) = map.get("args") {
                    let args_str: Vec<String> = args
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| format!("\"{}\"", s)))
                        .collect();
                    if !args_str.is_empty() {
                        lines.push(format!("args = [{}]", args_str.join(", ")));
                    }
                }
                if let Some(Value::Object(env)) = map.get("env") {
                    let mut env_lines = vec!["[mcp_servers.\"{}\".env]".to_string()];
                    for (k, v) in env {
                        if let Some(s) = v.as_str() {
                            env_lines.push(format!("{} = \"{}\"", k, s));
                        }
                    }
                    if env_lines.len() > 1 {
                        // Replace placeholder with actual name
                        for line in &mut env_lines {
                            *line = line.replace("{}", name);
                        }
                        lines.extend(env_lines);
                    }
                }
            }
            lines.push(String::new());
        }

        let content = lines.join("\n");
        Some(McpFile {
            path: ".codex/config.toml".to_string(),
            content,
            merge_key: Some("mcp_servers".to_string()),
            format: McpFormat::Toml,
        })
    }

    fn mcp_config_path(&self) -> Option<&'static str> {
        Some(".codex/config.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_destination_path() {
        let a = CodexCli;
        assert_eq!(
            a.destination_path("praxis/conventions.md"),
            Some(PathBuf::from(".codex/conventions.md"))
        );
        assert_eq!(
            a.destination_path("praxis/skills/pp-brainstorm/SKILL.md"),
            Some(PathBuf::from(".codex/skills/pp-brainstorm/SKILL.md"))
        );
        assert_eq!(
            a.destination_path("praxis/agents/reviewers/security.md"),
            Some(PathBuf::from(".codex/agents/reviewers/security.md"))
        );
        assert_eq!(a.destination_path("README.md"), None);
    }

    #[test]
    fn test_generate_mcp_config() {
        let a = CodexCli;
        let mut servers = McpServers::new();
        servers.insert(
            "figma".to_string(),
            serde_json::json!({
                "command": "npx",
                "args": ["-y", "figma-developer-mcp"],
                "env": { "FIGMA_API_KEY": "${FIGMA_API_KEY}" }
            }),
        );
        let result = a.mcp_config(&servers).unwrap();
        assert_eq!(result.path, ".codex/config.toml");
        assert!(result.content.contains("[mcp_servers.\"figma\"]"));
        assert!(result.content.contains("command = \"npx\""));
        assert!(
            result
                .content
                .contains("args = [\"-y\", \"figma-developer-mcp\"]")
        );
        assert!(result.content.contains("[mcp_servers.\"figma\".env]"));
        assert!(
            result
                .content
                .contains("FIGMA_API_KEY = \"${FIGMA_API_KEY}\"")
        );
    }

    #[test]
    fn test_generate_mcp_config_no_args_no_env() {
        let a = CodexCli;
        let mut servers = McpServers::new();
        servers.insert("test".to_string(), serde_json::json!({ "command": "cmd" }));
        let result = a.mcp_config(&servers).unwrap();
        assert!(result.content.contains("[mcp_servers.\"test\"]"));
        assert!(result.content.contains("command = \"cmd\""));
        assert!(!result.content.contains("args"));
        assert!(!result.content.contains("env"));
    }

    #[test]
    fn test_mcp_config_merge_key() {
        let a = CodexCli;
        let result = a.mcp_config(&McpServers::new()).unwrap();
        assert_eq!(result.merge_key, Some("mcp_servers".to_string()));
    }

    #[test]
    fn test_managed_files() {
        let a = CodexCli;
        let sources = vec![
            "praxis/conventions.md".to_string(),
            "praxis/skills/figma-to-code/SKILL.md".to_string(),
            "README.md".to_string(),
        ];
        let managed = a.managed_files(&sources);
        assert_eq!(
            managed,
            vec![
                PathBuf::from(".codex/conventions.md"),
                PathBuf::from(".codex/skills/figma-to-code/SKILL.md"),
            ]
        );
    }

    #[test]
    fn test_tool_name() {
        assert_eq!(CodexCli.name(), "codex");
    }
}
