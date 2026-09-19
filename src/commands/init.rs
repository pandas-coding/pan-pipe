use crate::adapters::{get_adapter, list_adapters, regenerate_tool_configs};
use crate::core::components::{
    build_group_options, discover_optional_components, get_component_files, get_core_files,
};
use crate::core::files::is_safe_path;
use crate::core::manifest::{FileEntry, Manifest, hash_content, read_manifest, write_manifest};
use crate::core::templates::fetch_templates;
use anyhow::{Result, anyhow};
use owo_colors::OwoColorize;
use std::collections::HashMap;

pub async fn run(
    ref_: Option<String>,
    tools: Vec<String>,
    all_components: bool,
    no_components: bool,
) -> Result<()> {
    let project_root = std::env::current_dir()?;
    let resolved_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.clone());

    println!("{}", "Pan-Pipe — Initialize".bold());

    if read_manifest(&project_root).await?.is_some() {
        println!(
            "{}",
            "Pan-Pipe is already initialized in this project. Running update instead.".yellow()
        );
        return crate::commands::update::run(ref_).await;
    }

    let templates = fetch_templates(ref_.as_deref()).await?;
    println!("Fetched {} template files", templates.len());

    // Tool selection
    let enabled_tools = if !tools.is_empty() {
        let all_adapters = list_adapters();
        let valid_names: Vec<&str> = all_adapters.iter().map(|(name, _)| *name).collect();
        for name in &tools {
            if !valid_names.contains(&name.as_str()) {
                anyhow::bail!(
                    "Unknown tool \"{}\". Available: {}",
                    name,
                    valid_names.join(", ")
                );
            }
        }
        tools
    } else {
        let all_adapters = list_adapters();
        let adapter_options: Vec<String> = all_adapters
            .iter()
            .map(|(name, display)| format!("{} ({})", display, name))
            .collect();

        let selected_tools = if !adapter_options.is_empty() {
            inquire::MultiSelect::new("Select tools to install for:", adapter_options.clone())
                .prompt()
                .map_err(|e| {
                    anyhow!(
                        "Tool selection failed: {}. For non-interactive use, pass one or more --tool flags (e.g. --tool pi-coding-agent)",
                        e
                    )
                })?
        } else {
            vec![]
        };

        selected_tools
            .iter()
            .map(|s| {
                s.split('(')
                    .nth(1)
                    .unwrap()
                    .trim_end_matches(')')
                    .to_string()
            })
            .collect()
    };

    if enabled_tools.is_empty() {
        anyhow::bail!(
            "At least one tool must be selected. Skills and agents are installed into each coding agent's config directory (e.g. `.pi/skills/`). Press <space> to toggle, <enter> to confirm, or re-run non-interactively with `--tool <name>`."
        );
    }

    // Optional component selection
    let optional_components = discover_optional_components(&templates);
    let mut selected_components = crate::core::manifest::SelectedComponents {
        skills: vec![],
        reviewers: vec![],
    };

    if !optional_components.is_empty() {
        let (group_options, _all_values) = build_group_options(&optional_components);
        let group_labels: Vec<String> = group_options.keys().cloned().collect();
        let mut options_flat: Vec<String> = Vec::new();
        for label in &group_labels {
            for opt in &group_options[label] {
                options_flat.push(format!("{}: {}", label, opt.label));
            }
        }

        let selected = if all_components {
            options_flat.clone()
        } else if no_components {
            vec![]
        } else {
            inquire::MultiSelect::new(
                "Select optional components to install:",
                options_flat.clone(),
            )
            .with_default(&(0..options_flat.len()).collect::<Vec<_>>())
            .prompt()
            .map_err(|e| {
                anyhow!(
                    "Component selection failed: {}. For non-interactive use, pass --all-components or --no-components",
                    e
                )
            })?
        };

        for s in selected {
            let parts: Vec<_> = s.splitn(2, ": ").collect();
            if parts.len() == 2 {
                let ty = if parts[0] == "Skills" {
                    "skill"
                } else {
                    "reviewer"
                };
                if ty == "skill" {
                    selected_components.skills.push(parts[1].to_string());
                } else {
                    selected_components.reviewers.push(parts[1].to_string());
                }
            }
        }
    }

    // Build files to install
    let mut files_to_install = get_core_files(&templates);
    for skill in &selected_components.skills {
        for (k, v) in get_component_files(
            &templates,
            skill,
            crate::core::components::ComponentType::Skill,
        ) {
            files_to_install.insert(k, v);
        }
    }
    for reviewer in &selected_components.reviewers {
        for (k, v) in get_component_files(
            &templates,
            reviewer,
            crate::core::components::ComponentType::Reviewer,
        ) {
            files_to_install.insert(k, v);
        }
    }

    let mut manifest_files: HashMap<String, FileEntry> = HashMap::new();
    let mut installed = 0usize;
    let skipped = 0usize;

    for (relative_path, content) in files_to_install {
        if !relative_path.starts_with("praxis/") {
            continue;
        }
        let mut destinations = HashMap::new();
        let mut wrote = false;

        for tool_name in &enabled_tools {
            let Some(adapter) = get_adapter(tool_name) else {
                continue;
            };
            let Some(dest_path) = adapter.destination_path(&relative_path) else {
                continue;
            };
            let full_path = project_root.join(&dest_path);
            if !is_safe_path(&resolved_root, &full_path) {
                continue;
            }
            if let Some(parent) = full_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&full_path, &content).await?;
            destinations.insert(tool_name.clone(), dest_path.to_string_lossy().to_string());
            wrote = true;
        }

        if wrote {
            installed += 1;
        }
        manifest_files.insert(
            relative_path.clone(),
            FileEntry {
                hash: hash_content(&content),
                destinations,
            },
        );
    }

    // Create .ai-workflow directories
    for dir in [
        ".ai-workflow/ideas",
        ".ai-workflow/plans",
        ".ai-workflow/learnings",
    ] {
        tokio::fs::create_dir_all(project_root.join(dir)).await?;
    }
    let tags_path = project_root.join(".ai-workflow/tags");
    if !tags_path.exists() {
        tokio::fs::write(&tags_path, "").await?;
    }

    let now = chrono::Utc::now().to_rfc3339();
    let manifest = Manifest {
        version: "1.0.0".to_string(),
        installed_at: now.clone(),
        updated_at: now,
        enabled_tools,
        selected_components: Some(selected_components),
        files: manifest_files,
    };
    write_manifest(&project_root, &manifest).await?;

    if !manifest.enabled_tools.is_empty() {
        match regenerate_tool_configs(&project_root, &manifest).await {
            Ok(regenerated) if !regenerated.is_empty() => {
                println!("Generated MCP config for {}", regenerated.join(", "));
            }
            Ok(_) => {}
            Err(e) => {
                println!(
                    "{} Could not generate tool configs: {}",
                    "Warning:".yellow(),
                    e
                );
            }
        }
    }

    let mut parts = vec![format!("{} files installed", installed.to_string().green())];
    if skipped > 0 {
        parts.push(format!("{} files skipped", skipped.to_string().yellow()));
    }
    println!("Pan-Pipe initialized! {}", parts.join(", "));
    Ok(())
}
