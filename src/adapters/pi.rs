use super::{Adapter, McpFile, McpServers};
use std::path::PathBuf;

pub struct PiCodingAgent;

impl Adapter for PiCodingAgent {
    fn name(&self) -> &'static str {
        "pi-coding-agent"
    }

    fn display_name(&self) -> &'static str {
        "pi-coding-agent"
    }

    fn destination_path(&self, source: &str) -> Option<PathBuf> {
        if let Some(rest) = source.strip_prefix("praxis/skills/") {
            return Some(format!(".pi/skills/{}", rest).into());
        }
        if let Some(name) = source
            .strip_prefix("praxis/agents/reviewers/")
            .and_then(|s| s.strip_suffix(".md"))
        {
            return Some(format!(".pi/prompts/reviewer-{}.md", name).into());
        }
        if let Some(name) = source
            .strip_prefix("praxis/agents/")
            .and_then(|s| s.strip_suffix(".md"))
        {
            return Some(format!(".pi/prompts/{}.md", name).into());
        }
        // Shared files: skip for pi (no native conventions file support)
        None
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
    fn test_destination_path_skills() {
        let a = PiCodingAgent;
        assert_eq!(
            a.destination_path("praxis/skills/pp-brainstorm/SKILL.md"),
            Some(PathBuf::from(".pi/skills/pp-brainstorm/SKILL.md"))
        );
        assert_eq!(
            a.destination_path("praxis/skills/agent-browser/references/commands.md"),
            Some(PathBuf::from(
                ".pi/skills/agent-browser/references/commands.md"
            ))
        );
    }

    #[test]
    fn test_destination_path_reviewers() {
        let a = PiCodingAgent;
        assert_eq!(
            a.destination_path("praxis/agents/reviewers/security.md"),
            Some(PathBuf::from(".pi/prompts/reviewer-security.md"))
        );
        assert_eq!(
            a.destination_path("praxis/agents/reviewers/code-quality.md"),
            Some(PathBuf::from(".pi/prompts/reviewer-code-quality.md"))
        );
    }

    #[test]
    fn test_destination_path_other_agents() {
        let a = PiCodingAgent;
        assert_eq!(
            a.destination_path("praxis/agents/codebase-explorer.md"),
            Some(PathBuf::from(".pi/prompts/codebase-explorer.md"))
        );
    }

    #[test]
    fn test_destination_path_shared_files_returns_none() {
        let a = PiCodingAgent;
        assert_eq!(a.destination_path("praxis/conventions.md"), None);
        assert_eq!(a.destination_path("praxis/reviewer-output-format.md"), None);
    }

    #[test]
    fn test_generate_mcp_config_returns_none() {
        assert!(PiCodingAgent.mcp_config(&McpServers::new()).is_none());
    }

    #[test]
    fn test_mcp_config_path_returns_none() {
        assert_eq!(PiCodingAgent.mcp_config_path(), None);
    }

    #[test]
    fn test_managed_files() {
        let a = PiCodingAgent;
        let sources = vec![
            "praxis/skills/pp-brainstorm/SKILL.md".to_string(),
            "praxis/agents/reviewers/security.md".to_string(),
            "praxis/conventions.md".to_string(),
        ];
        let managed = a.managed_files(&sources);
        assert_eq!(
            managed,
            vec![
                PathBuf::from(".pi/skills/pp-brainstorm/SKILL.md"),
                PathBuf::from(".pi/prompts/reviewer-security.md"),
            ]
        );
    }

    #[test]
    fn test_tool_name() {
        assert_eq!(PiCodingAgent.name(), "pi-coding-agent");
    }
}
