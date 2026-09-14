use super::{Adapter, McpFile, McpServers};
use std::path::PathBuf;

pub struct AmpCode;

impl Adapter for AmpCode {
    fn name(&self) -> &'static str {
        "amp-code"
    }

    fn display_name(&self) -> &'static str {
        "Amp Code"
    }

    fn destination_path(&self, source: &str) -> Option<PathBuf> {
        source
            .strip_prefix("praxis/")
            .map(|r| format!(".agents/{}", r).into())
    }

    fn mcp_config(&self, _servers: &McpServers) -> Option<McpFile> {
        None
    }

    fn mcp_config_path(&self) -> Option<&'static str> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_destination_path() {
        let a = AmpCode;
        assert_eq!(
            a.destination_path("praxis/conventions.md"),
            Some(PathBuf::from(".agents/conventions.md"))
        );
        assert_eq!(
            a.destination_path("praxis/skills/px-brainstorm/SKILL.md"),
            Some(PathBuf::from(".agents/skills/px-brainstorm/SKILL.md"))
        );
        assert_eq!(
            a.destination_path("praxis/agents/reviewers/security.md"),
            Some(PathBuf::from(".agents/agents/reviewers/security.md"))
        );
        assert_eq!(a.destination_path("README.md"), None);
    }

    #[test]
    fn test_generate_mcp_config_returns_none() {
        assert!(AmpCode.mcp_config(&McpServers::new()).is_none());
    }

    #[test]
    fn test_mcp_config_path_returns_none() {
        assert_eq!(AmpCode.mcp_config_path(), None);
    }

    #[test]
    fn test_managed_files() {
        let a = AmpCode;
        let sources = vec![
            "praxis/conventions.md".to_string(),
            "README.md".to_string(),
            "praxis/skills/px-brainstorm/SKILL.md".to_string(),
        ];
        let managed = a.managed_files(&sources);
        assert_eq!(
            managed,
            vec![
                PathBuf::from(".agents/conventions.md"),
                PathBuf::from(".agents/skills/px-brainstorm/SKILL.md"),
            ]
        );
    }

    #[test]
    fn test_tool_name() {
        assert_eq!(AmpCode.name(), "amp-code");
    }
}
