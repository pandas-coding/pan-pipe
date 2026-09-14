use super::{Adapter, McpFile, McpServers, transform_env_vars};
use std::path::PathBuf;

pub struct Cursor;

impl Adapter for Cursor {
    fn name(&self) -> &'static str {
        "cursor"
    }

    fn display_name(&self) -> &'static str {
        "Cursor"
    }

    fn destination_path(&self, source: &str) -> Option<PathBuf> {
        source
            .strip_prefix("praxis/")
            .map(|r| format!(".cursor/{}", r).into())
    }

    fn mcp_config(&self, servers: &McpServers) -> Option<McpFile> {
        let mut transformed = serde_json::Value::Object(servers.clone());
        transform_env_vars(&mut transformed, &|name| format!("${{env:{}}}", name));
        let content =
            serde_json::to_string_pretty(&serde_json::json!({ "mcpServers": transformed }))
                .unwrap()
                + "\n";
        Some(McpFile {
            path: ".cursor/mcp.json".to_string(),
            content,
            merge_key: None,
            format: super::McpFormat::Json,
        })
    }

    fn mcp_config_path(&self) -> Option<&'static str> {
        Some(".cursor/mcp.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_destination_path() {
        let a = Cursor;
        assert_eq!(
            a.destination_path("praxis/conventions.md"),
            Some(PathBuf::from(".cursor/conventions.md"))
        );
        assert_eq!(
            a.destination_path("praxis/skills/figma-to-code/SKILL.md"),
            Some(PathBuf::from(".cursor/skills/figma-to-code/SKILL.md"))
        );
        assert_eq!(a.destination_path("README.md"), None);
    }

    #[test]
    fn test_generate_mcp_config_transforms_env() {
        let a = Cursor;
        let mut servers = McpServers::new();
        servers.insert(
            "test".to_string(),
            serde_json::json!({
                "command": "cmd",
                "args": [],
                "env": { "KEY": "${MY_VAR}", "OTHER": "${ANOTHER}" }
            }),
        );
        let result = a.mcp_config(&servers).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["mcpServers"]["test"]["env"]["KEY"], "${env:MY_VAR}");
        assert_eq!(
            parsed["mcpServers"]["test"]["env"]["OTHER"],
            "${env:ANOTHER}"
        );
    }

    #[test]
    fn test_managed_files() {
        let a = Cursor;
        let sources = vec![
            "praxis/conventions.md".to_string(),
            "praxis/agents/reviewers/security.md".to_string(),
            "README.md".to_string(),
        ];
        let managed = a.managed_files(&sources);
        assert_eq!(
            managed,
            vec![
                PathBuf::from(".cursor/conventions.md"),
                PathBuf::from(".cursor/agents/reviewers/security.md"),
            ]
        );
    }

    #[test]
    fn test_tool_name() {
        assert_eq!(Cursor.name(), "cursor");
    }
}
