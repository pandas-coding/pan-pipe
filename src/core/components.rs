#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

const CORE_SKILLS: &[&str] = &[
    "pp-brainstorm",
    "pp-plan",
    "pp-implement",
    "pp-review",
    "pp-retrospect",
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ComponentType {
    Skill,
    Reviewer,
}

impl ComponentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ComponentType::Skill => "skill",
            ComponentType::Reviewer => "reviewer",
        }
    }
}

impl std::str::FromStr for ComponentType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "skill" => Ok(ComponentType::Skill),
            "reviewer" => Ok(ComponentType::Reviewer),
            _ => Err(format!("unknown component type: {}", s)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub name: String,
    pub r#type: ComponentType,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SelectedComponents {
    pub skills: Vec<String>,
    pub reviewers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupOption {
    pub value: String,
    pub label: String,
}

pub fn get_component_for_file(relative_path: &str) -> Option<(ComponentType, String)> {
    if let Some(rest) = relative_path.strip_prefix("praxis/skills/") {
        let name = rest.split('/').next()?;
        if !CORE_SKILLS.contains(&name) {
            return Some((ComponentType::Skill, name.to_string()));
        }
        return None;
    }

    if let Some(name) = relative_path
        .strip_prefix("praxis/agents/reviewers/")
        .and_then(|s| s.strip_suffix(".md"))
    {
        return Some((ComponentType::Reviewer, name.to_string()));
    }

    None
}

pub fn get_component_files(
    templates: &BTreeMap<String, String>,
    component_name: &str,
    component_type: ComponentType,
) -> BTreeMap<String, String> {
    templates
        .iter()
        .filter(|(path, _)| {
            if let Some((ty, name)) = get_component_for_file(path) {
                name == component_name && ty == component_type
            } else {
                false
            }
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

pub fn get_core_files(templates: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    templates
        .iter()
        .filter(|(path, _)| get_component_for_file(path).is_none())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

pub fn discover_optional_components(templates: &BTreeMap<String, String>) -> Vec<Component> {
    let mut seen: BTreeSet<(ComponentType, String)> = BTreeSet::new();

    for path in templates.keys() {
        if let Some((ty, name)) = get_component_for_file(path) {
            seen.insert((ty, name));
        }
    }

    let mut components: Vec<Component> = seen
        .into_iter()
        .map(|(ty, name)| {
            let description = get_component_description(templates, &name, &ty);
            Component {
                name,
                r#type: ty,
                description,
            }
        })
        .collect();

    components.sort_by(|a, b| {
        if a.r#type != b.r#type {
            a.r#type.cmp(&b.r#type)
        } else {
            a.name.cmp(&b.name)
        }
    });

    components
}

pub fn get_component_description(
    templates: &BTreeMap<String, String>,
    component_name: &str,
    component_type: &ComponentType,
) -> String {
    let primary_path = match component_type {
        ComponentType::Reviewer => format!("praxis/agents/reviewers/{}.md", component_name),
        ComponentType::Skill => format!("praxis/skills/{}/SKILL.md", component_name),
    };

    let content = match templates.get(&primary_path) {
        Some(c) => c,
        None => return component_name.to_string(),
    };

    extract_description(content).unwrap_or_else(|| component_name.to_string())
}

fn extract_description(content: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("description:") {
            let rest = rest.trim();
            let desc = rest.trim_matches('"');
            if !desc.is_empty() {
                return Some(desc.to_string());
            }
        }
    }
    None
}

pub fn get_selected_components(
    manifest_selected: Option<&crate::core::manifest::SelectedComponents>,
    templates: &BTreeMap<String, String>,
) -> crate::core::manifest::SelectedComponents {
    if let Some(selected) = manifest_selected {
        return selected.clone();
    }

    let all = discover_optional_components(templates);
    let mut result = crate::core::manifest::SelectedComponents::default();
    for comp in all {
        match comp.r#type {
            ComponentType::Skill => result.skills.push(comp.name),
            ComponentType::Reviewer => result.reviewers.push(comp.name),
        }
    }
    result
}

pub fn encode_component_value(component_type: &ComponentType, name: &str) -> String {
    format!("{}:{}", component_type.as_str(), name)
}

pub fn decode_component_value(value: &str) -> Option<(ComponentType, String)> {
    let colon_idx = value.find(':')?;
    let ty = value[..colon_idx].parse().ok()?;
    let name = value[colon_idx + 1..].to_string();
    Some((ty, name))
}

pub fn build_group_options(
    optional_components: &[Component],
) -> (BTreeMap<String, Vec<GroupOption>>, Vec<String>) {
    let mut group_options: BTreeMap<String, Vec<GroupOption>> = BTreeMap::new();
    let mut all_values = Vec::new();

    for comp in optional_components {
        let group_label = match comp.r#type {
            ComponentType::Skill => "Skills",
            ComponentType::Reviewer => "Reviewers",
        };
        let value = encode_component_value(&comp.r#type, &comp.name);
        group_options
            .entry(group_label.to_string())
            .or_default()
            .push(GroupOption {
                value: value.clone(),
                label: comp.name.clone(),
            });
        all_values.push(value);
    }

    (group_options, all_values)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_templates(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_get_component_for_file_core_skills() {
        assert!(get_component_for_file("praxis/skills/pp-brainstorm/SKILL.md").is_none());
        assert!(get_component_for_file("praxis/skills/pp-plan/SKILL.md").is_none());
        assert!(get_component_for_file("praxis/skills/pp-implement/SKILL.md").is_none());
        assert!(get_component_for_file("praxis/skills/pp-review/SKILL.md").is_none());
        assert!(get_component_for_file("praxis/skills/pp-retrospect/SKILL.md").is_none());
        // Legacy px- names are no longer in the core whitelist: upstream paths
        // are renamed to pp- at load time, and stale manifest keys are handled
        // by the update migration. A px- path is therefore classified as an
        // optional component.
        assert_eq!(
            get_component_for_file("praxis/skills/px-brainstorm/SKILL.md"),
            Some((ComponentType::Skill, "px-brainstorm".to_string()))
        );
    }

    #[test]
    fn test_get_component_for_file_optional_skills() {
        assert_eq!(
            get_component_for_file("praxis/skills/agent-browser/SKILL.md"),
            Some((ComponentType::Skill, "agent-browser".to_string()))
        );
        assert_eq!(
            get_component_for_file("praxis/skills/figma-to-code/SKILL.md"),
            Some((ComponentType::Skill, "figma-to-code".to_string()))
        );
    }

    #[test]
    fn test_get_component_for_file_nested_optional_skill() {
        assert_eq!(
            get_component_for_file("praxis/skills/agent-browser/references/commands.md"),
            Some((ComponentType::Skill, "agent-browser".to_string()))
        );
    }

    #[test]
    fn test_get_component_for_file_reviewer() {
        assert_eq!(
            get_component_for_file("praxis/agents/reviewers/security.md"),
            Some((ComponentType::Reviewer, "security".to_string()))
        );
        assert_eq!(
            get_component_for_file("praxis/agents/reviewers/code-quality.md"),
            Some((ComponentType::Reviewer, "code-quality".to_string()))
        );
    }

    #[test]
    fn test_get_component_for_file_core_root() {
        assert!(get_component_for_file("praxis/conventions.md").is_none());
        assert!(get_component_for_file("praxis/reviewer-output-format.md").is_none());
    }

    #[test]
    fn test_get_component_for_file_non_reviewer_agents() {
        assert!(get_component_for_file("praxis/agents/codebase-explorer.md").is_none());
        assert!(get_component_for_file("praxis/agents/external-researcher.md").is_none());
    }

    #[test]
    fn test_get_core_files() {
        let templates = make_templates(&[
            ("praxis/conventions.md", "conventions"),
            ("praxis/reviewer-output-format.md", "format"),
            ("praxis/agents/codebase-explorer.md", "explorer"),
            ("praxis/skills/pp-brainstorm/SKILL.md", "brainstorm"),
            ("praxis/skills/agent-browser/SKILL.md", "browser"),
            ("praxis/agents/reviewers/security.md", "security"),
        ]);
        let core = get_core_files(&templates);
        assert!(core.contains_key("praxis/conventions.md"));
        assert!(core.contains_key("praxis/reviewer-output-format.md"));
        assert!(core.contains_key("praxis/agents/codebase-explorer.md"));
        assert!(core.contains_key("praxis/skills/pp-brainstorm/SKILL.md"));
        assert!(!core.contains_key("praxis/skills/agent-browser/SKILL.md"));
        assert!(!core.contains_key("praxis/agents/reviewers/security.md"));
    }

    #[test]
    fn test_get_component_files_skill() {
        let templates = make_templates(&[
            ("praxis/skills/agent-browser/SKILL.md", "browser skill"),
            (
                "praxis/skills/agent-browser/references/commands.md",
                "commands",
            ),
            ("praxis/skills/figma-to-code/SKILL.md", "figma skill"),
            ("praxis/agents/reviewers/security.md", "security reviewer"),
            ("praxis/conventions.md", "core"),
        ]);
        let files = get_component_files(&templates, "agent-browser", ComponentType::Skill);
        assert_eq!(files.len(), 2);
        assert!(files.contains_key("praxis/skills/agent-browser/SKILL.md"));
        assert!(files.contains_key("praxis/skills/agent-browser/references/commands.md"));
    }

    #[test]
    fn test_get_component_files_reviewer() {
        let templates = make_templates(&[
            ("praxis/agents/reviewers/security.md", "security reviewer"),
            ("praxis/conventions.md", "core"),
        ]);
        let files = get_component_files(&templates, "security", ComponentType::Reviewer);
        assert_eq!(files.len(), 1);
        assert!(files.contains_key("praxis/agents/reviewers/security.md"));
    }

    #[test]
    fn test_get_component_files_unknown() {
        let templates = make_templates(&[("praxis/conventions.md", "core")]);
        assert_eq!(
            get_component_files(&templates, "nonexistent", ComponentType::Skill).len(),
            0
        );
    }

    #[test]
    fn test_get_component_files_name_conflict() {
        let templates = make_templates(&[
            ("praxis/skills/security/SKILL.md", "skill content"),
            ("praxis/agents/reviewers/security.md", "reviewer content"),
        ]);
        let skill_files = get_component_files(&templates, "security", ComponentType::Skill);
        let reviewer_files = get_component_files(&templates, "security", ComponentType::Reviewer);
        assert_eq!(skill_files.len(), 1);
        assert!(skill_files.contains_key("praxis/skills/security/SKILL.md"));
        assert_eq!(reviewer_files.len(), 1);
        assert!(reviewer_files.contains_key("praxis/agents/reviewers/security.md"));
    }

    #[test]
    fn test_discover_optional_components() {
        let templates = make_templates(&[
            ("praxis/conventions.md", "core"),
            (
                "praxis/skills/pp-brainstorm/SKILL.md",
                "---\ndescription: Brainstorm\n---",
            ),
            (
                "praxis/skills/agent-browser/SKILL.md",
                "---\ndescription: \"Browser automation\"\n---",
            ),
            (
                "praxis/skills/agent-browser/references/commands.md",
                "commands",
            ),
            (
                "praxis/skills/figma-to-code/SKILL.md",
                "---\ndescription: Figma\n---",
            ),
            (
                "praxis/agents/reviewers/security.md",
                "---\ndescription: Security review\n---",
            ),
            (
                "praxis/agents/reviewers/code-quality.md",
                "---\ndescription: Code quality\n---",
            ),
        ]);
        let components = discover_optional_components(&templates);
        let names: Vec<_> = components.iter().map(|c| c.name.as_str()).collect();
        assert!(!names.contains(&"pp-brainstorm"));
        assert!(names.contains(&"agent-browser"));
        assert!(names.contains(&"figma-to-code"));
        assert!(names.contains(&"security"));
        assert!(names.contains(&"code-quality"));
        assert_eq!(components.len(), 4);
    }

    #[test]
    fn test_discover_optional_components_order() {
        let templates = make_templates(&[
            (
                "praxis/agents/reviewers/security.md",
                "---\ndescription: Security\n---",
            ),
            (
                "praxis/skills/agent-browser/SKILL.md",
                "---\ndescription: Browser\n---",
            ),
        ]);
        let components = discover_optional_components(&templates);
        assert_eq!(components[0].r#type, ComponentType::Skill);
        assert_eq!(components[1].r#type, ComponentType::Reviewer);
    }

    #[test]
    fn test_discover_optional_components_dedup() {
        let templates = make_templates(&[
            (
                "praxis/skills/agent-browser/SKILL.md",
                "---\ndescription: Browser\n---",
            ),
            (
                "praxis/skills/agent-browser/references/commands.md",
                "commands",
            ),
            ("praxis/skills/agent-browser/references/auth.md", "auth"),
        ]);
        let components = discover_optional_components(&templates);
        assert_eq!(
            components
                .iter()
                .filter(|c| c.name == "agent-browser")
                .count(),
            1
        );
    }

    #[test]
    fn test_discover_optional_components_empty() {
        let templates = make_templates(&[
            ("praxis/conventions.md", "core"),
            ("praxis/skills/pp-brainstorm/SKILL.md", "core skill"),
        ]);
        assert!(discover_optional_components(&templates).is_empty());
    }

    #[test]
    fn test_get_component_description_skill_with_quotes() {
        let templates = make_templates(&[(
            "praxis/skills/agent-browser/SKILL.md",
            "---\nname: agent-browser\ndescription: \"Browser automation CLI for AI agents\"\n---\n\n# Content",
        )]);
        assert_eq!(
            get_component_description(&templates, "agent-browser", &ComponentType::Skill),
            "Browser automation CLI for AI agents"
        );
    }

    #[test]
    fn test_get_component_description_without_quotes() {
        let templates = make_templates(&[(
            "praxis/skills/figma-to-code/SKILL.md",
            "---\ndescription: Figma to React\n---",
        )]);
        assert_eq!(
            get_component_description(&templates, "figma-to-code", &ComponentType::Skill),
            "Figma to React"
        );
    }

    #[test]
    fn test_get_component_description_reviewer() {
        let templates = make_templates(&[(
            "praxis/agents/reviewers/security.md",
            "---\ndescription: Security review\n---",
        )]);
        assert_eq!(
            get_component_description(&templates, "security", &ComponentType::Reviewer),
            "Security review"
        );
    }

    #[test]
    fn test_get_component_description_fallback_missing_file() {
        let templates = make_templates(&[]);
        assert_eq!(
            get_component_description(&templates, "my-skill", &ComponentType::Skill),
            "my-skill"
        );
    }

    #[test]
    fn test_get_component_description_fallback_no_description() {
        let templates = make_templates(&[(
            "praxis/skills/my-skill/SKILL.md",
            "---\nname: my-skill\n---\n# Content",
        )]);
        assert_eq!(
            get_component_description(&templates, "my-skill", &ComponentType::Skill),
            "my-skill"
        );
    }

    #[test]
    fn test_get_selected_components_from_manifest() {
        let selected = crate::core::manifest::SelectedComponents {
            skills: vec!["agent-browser".to_string()],
            reviewers: vec!["security".to_string()],
        };
        let templates = make_templates(&[]);
        assert_eq!(
            get_selected_components(Some(&selected), &templates),
            selected
        );
    }

    #[test]
    fn test_get_selected_components_fallback() {
        let templates = make_templates(&[
            (
                "praxis/skills/agent-browser/SKILL.md",
                "---\ndescription: Browser\n---",
            ),
            (
                "praxis/agents/reviewers/security.md",
                "---\ndescription: Security\n---",
            ),
            ("praxis/skills/pp-brainstorm/SKILL.md", "core"),
            ("praxis/conventions.md", "core"),
        ]);
        let result = get_selected_components(None, &templates);
        assert!(result.skills.contains(&"agent-browser".to_string()));
        assert!(!result.skills.contains(&"pp-brainstorm".to_string()));
        assert!(result.reviewers.contains(&"security".to_string()));
    }

    #[test]
    fn test_encode_decode_component_value() {
        let encoded = encode_component_value(&ComponentType::Skill, "agent-browser");
        assert_eq!(encoded, "skill:agent-browser");
        let (ty, name) = decode_component_value(&encoded).unwrap();
        assert_eq!(ty, ComponentType::Skill);
        assert_eq!(name, "agent-browser");
    }

    #[test]
    fn test_encode_decode_with_colon_in_name() {
        let encoded = encode_component_value(&ComponentType::Reviewer, "foo:bar");
        assert_eq!(encoded, "reviewer:foo:bar");
        let (ty, name) = decode_component_value(&encoded).unwrap();
        assert_eq!(ty, ComponentType::Reviewer);
        assert_eq!(name, "foo:bar");
    }

    #[test]
    fn test_build_group_options() {
        let components = vec![
            Component {
                name: "agent-browser".to_string(),
                r#type: ComponentType::Skill,
                description: "Browser automation".to_string(),
            },
            Component {
                name: "figma-to-code".to_string(),
                r#type: ComponentType::Skill,
                description: "Figma to code".to_string(),
            },
            Component {
                name: "security".to_string(),
                r#type: ComponentType::Reviewer,
                description: "Security review".to_string(),
            },
        ];
        let (group_options, all_values) = build_group_options(&components);
        assert_eq!(group_options["Skills"].len(), 2);
        assert_eq!(group_options["Reviewers"].len(), 1);
        assert_eq!(
            group_options["Skills"][0],
            GroupOption {
                value: "skill:agent-browser".to_string(),
                label: "agent-browser".to_string(),
            }
        );
        assert_eq!(
            group_options["Reviewers"][0],
            GroupOption {
                value: "reviewer:security".to_string(),
                label: "security".to_string(),
            }
        );
        assert_eq!(
            all_values,
            vec![
                "skill:agent-browser",
                "skill:figma-to-code",
                "reviewer:security"
            ]
        );
    }

    #[test]
    fn test_build_group_options_empty() {
        let (group_options, all_values) = build_group_options(&[]);
        assert!(group_options.is_empty());
        assert!(all_values.is_empty());
    }
}
