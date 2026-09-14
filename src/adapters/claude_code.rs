use super::{Adapter, McpFile, McpServers};
use std::path::PathBuf;

pub struct ClaudeCode;

impl Adapter for ClaudeCode {
    fn name(&self) -> &'static str {
        "claude-code"
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
    }

    fn destination_path(&self, source: &str) -> Option<PathBuf> {
        source
            .strip_prefix("praxis/")
            .map(|r| format!(".claude/{}", r).into())
    }

    fn mcp_config(&self, servers: &McpServers) -> Option<McpFile> {
        let content = serde_json::to_string_pretty(&serde_json::json!({ "mcpServers": servers }))
            .unwrap()
            + "\n";
        Some(McpFile {
            path: ".mcp.json".to_string(),
            content,
            merge_key: None,
            format: super::McpFormat::Json,
        })
    }

    fn mcp_config_path(&self) -> Option<&'static str> {
        Some(".mcp.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_destination_path() {
        let a = ClaudeCode;
        assert_eq!(
            a.destination_path("praxis/conventions.md"),
            Some(PathBuf::from(".claude/conventions.md"))
        );
        assert_eq!(
            a.destination_path("praxis/skills/px-brainstorm/SKILL.md"),
            Some(PathBuf::from(".claude/skills/px-brainstorm/SKILL.md"))
        );
        assert_eq!(
            a.destination_path("praxis/agents/reviewers/security.md"),
            Some(PathBuf::from(".claude/agents/reviewers/security.md"))
        );
        assert_eq!(a.destination_path("README.md"), None);
    }

    #[test]
    fn test_generate_mcp_config() {
        let a = ClaudeCode;
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
        assert_eq!(result.path, ".mcp.json");
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(
            parsed["mcpServers"]["figma"]["env"]["FIGMA_API_KEY"],
            "${FIGMA_API_KEY}"
        );
    }

    #[test]
    fn test_managed_files() {
        let a = ClaudeCode;
        let sources = vec![
            "praxis/conventions.md".to_string(),
            "praxis/skills/figma-to-code/SKILL.md".to_string(),
            "README.md".to_string(),
        ];
        let managed = a.managed_files(&sources);
        assert_eq!(
            managed,
            vec![
                PathBuf::from(".claude/conventions.md"),
                PathBuf::from(".claude/skills/figma-to-code/SKILL.md"),
            ]
        );
    }

    #[test]
    fn test_tool_name() {
        assert_eq!(ClaudeCode.name(), "claude-code");
    }
}
