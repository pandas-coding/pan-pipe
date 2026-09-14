use super::{Adapter, McpFile, McpServers, transform_env_vars};
use serde_json::Value;
use std::path::PathBuf;

pub struct OpenCode;

impl Adapter for OpenCode {
    fn name(&self) -> &'static str {
        "opencode"
    }

    fn display_name(&self) -> &'static str {
        "OpenCode"
    }

    fn destination_path(&self, source: &str) -> Option<PathBuf> {
        source
            .strip_prefix("praxis/")
            .map(|r| format!(".opencode/{}", r).into())
    }

    fn mcp_config(&self, servers: &McpServers) -> Option<McpFile> {
        let mut out = serde_json::Map::new();
        for (name, entry) in servers {
            let obj = entry.clone();
            let command = if let Some(cmd) = obj.get("command").and_then(|v| v.as_str()) {
                let mut cmd_vec = vec![cmd.to_string()];
                if let Some(args) = obj.get("args").and_then(|v| v.as_array()) {
                    for arg in args {
                        if let Some(s) = arg.as_str() {
                            cmd_vec.push(s.to_string());
                        }
                    }
                }
                Value::Array(cmd_vec.into_iter().map(Value::String).collect())
            } else {
                Value::Array(vec![])
            };

            let mut env = obj
                .get("env")
                .cloned()
                .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
            transform_env_vars(&mut env, &|name| format!("{{env:{}}}", name));

            let mut server = serde_json::Map::new();
            server.insert("type".to_string(), "local".into());
            server.insert("command".to_string(), command);
            server.insert("environment".to_string(), env);
            out.insert(name.clone(), Value::Object(server));
        }

        let content =
            serde_json::to_string_pretty(&serde_json::json!({ "mcp": out })).unwrap() + "\n";
        Some(McpFile {
            path: "opencode.json".to_string(),
            content,
            merge_key: Some("mcp".to_string()),
            format: super::McpFormat::Json,
        })
    }

    fn mcp_config_path(&self) -> Option<&'static str> {
        Some("opencode.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_destination_path() {
        let a = OpenCode;
        assert_eq!(
            a.destination_path("praxis/conventions.md"),
            Some(PathBuf::from(".opencode/conventions.md"))
        );
        assert_eq!(
            a.destination_path("praxis/skills/mobile-mcp/SKILL.md"),
            Some(PathBuf::from(".opencode/skills/mobile-mcp/SKILL.md"))
        );
        assert_eq!(
            a.destination_path("praxis/agents/reviewers/security.md"),
            Some(PathBuf::from(".opencode/agents/reviewers/security.md"))
        );
        assert_eq!(a.destination_path("README.md"), None);
    }

    #[test]
    fn test_generate_mcp_config_structure() {
        let a = OpenCode;
        let mut servers = McpServers::new();
        servers.insert(
            "figma".to_string(),
            serde_json::json!({ "command": "npx", "args": ["-y", "figma-mcp"], "env": {} }),
        );
        let result = a.mcp_config(&servers).unwrap();
        assert_eq!(result.path, "opencode.json");
        assert_eq!(result.merge_key, Some("mcp".to_string()));
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert!(parsed.get("mcp").is_some());
        assert_eq!(
            parsed["mcp"]["figma"]["command"],
            serde_json::json!(["npx", "-y", "figma-mcp"])
        );
        assert_eq!(parsed["mcp"]["figma"]["type"], "local");
    }

    #[test]
    fn test_generate_mcp_config_renames_env() {
        let a = OpenCode;
        let mut servers = McpServers::new();
        servers.insert(
            "test".to_string(),
            serde_json::json!({ "command": "cmd", "args": [], "env": { "KEY": "val" } }),
        );
        let result = a.mcp_config(&servers).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert!(parsed["mcp"]["test"].get("environment").is_some());
        assert!(parsed["mcp"]["test"].get("env").is_none());
        assert_eq!(parsed["mcp"]["test"]["environment"]["KEY"], "val");
    }

    #[test]
    fn test_generate_mcp_config_transforms_env_vars() {
        let a = OpenCode;
        let mut servers = McpServers::new();
        servers.insert(
            "test".to_string(),
            serde_json::json!({ "command": "cmd", "args": [], "env": { "KEY": "${MY_VAR}" } }),
        );
        let result = a.mcp_config(&servers).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["mcp"]["test"]["environment"]["KEY"], "{env:MY_VAR}");
    }

    #[test]
    fn test_generate_mcp_config_no_args() {
        let a = OpenCode;
        let mut servers = McpServers::new();
        servers.insert(
            "test".to_string(),
            serde_json::json!({ "command": "cmd", "env": {} }),
        );
        let result = a.mcp_config(&servers).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["mcp"]["test"]["command"], serde_json::json!(["cmd"]));
    }

    #[test]
    fn test_generate_mcp_config_no_env() {
        let a = OpenCode;
        let mut servers = McpServers::new();
        servers.insert(
            "test".to_string(),
            serde_json::json!({ "command": "cmd", "args": ["-y"] }),
        );
        let result = a.mcp_config(&servers).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["mcp"]["test"]["environment"], serde_json::json!({}));
        assert_eq!(
            parsed["mcp"]["test"]["command"],
            serde_json::json!(["cmd", "-y"])
        );
    }

    #[test]
    fn test_managed_files() {
        let a = OpenCode;
        let sources = vec![
            "praxis/conventions.md".to_string(),
            "praxis/agents/reviewers/security.md".to_string(),
            "README.md".to_string(),
        ];
        let managed = a.managed_files(&sources);
        assert_eq!(
            managed,
            vec![
                PathBuf::from(".opencode/conventions.md"),
                PathBuf::from(".opencode/agents/reviewers/security.md"),
            ]
        );
    }

    #[test]
    fn test_tool_name() {
        assert_eq!(OpenCode.name(), "opencode");
    }
}
