use crate::adapters::{get_adapter, regenerate_tool_configs};
use crate::core::components::{
    build_group_options, decode_component_value, discover_optional_components, get_component_files,
};
use crate::core::files::{install_file, is_safe_path};
use crate::core::manifest::{
    FileEntry, Manifest, is_destination_modified, is_locally_modified, read_manifest,
    write_manifest,
};
use crate::core::templates::fetch_templates;
use anyhow::{Result, anyhow};
use owo_colors::OwoColorize;
use std::collections::HashMap;

pub async fn run() -> Result<()> {
    let project_root = std::env::current_dir()?;
    let resolved_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.clone());

    println!("{}", "Pan-Pipe — Components".bold());

    let manifest = read_manifest(&project_root).await?.ok_or_else(|| {
        anyhow::anyhow!("Pan-Pipe is not initialized in this project. Run \"pan-pipe init\" first.")
    })?;

    let templates = fetch_templates(None).await?;
    println!("Fetched {} template files", templates.len());

    let optional_components = discover_optional_components(&templates);
    if optional_components.is_empty() {
        println!("No optional components available.");
        return Ok(());
    }

    let current_values: std::collections::HashSet<String> = match &manifest.selected_components {
        Some(sel) => {
            let mut vals = std::collections::HashSet::new();
            for n in &sel.skills {
                vals.insert(format!("skill:{}", n));
            }
            for n in &sel.reviewers {
                vals.insert(format!("reviewer:{}", n));
            }
            vals
        }
        None => {
            let mut vals = std::collections::HashSet::new();
            for comp in &optional_components {
                vals.insert(crate::core::components::encode_component_value(
                    &comp.r#type,
                    &comp.name,
                ));
            }
            vals
        }
    };

    let (group_options, all_values) = build_group_options(&optional_components);
    let initial_indices: Vec<usize> = all_values
        .iter()
        .enumerate()
        .filter(|(_, v)| current_values.contains(*v))
        .map(|(i, _)| i)
        .collect();

    let mut options_flat: Vec<String> = Vec::new();
    for label in group_options.keys() {
        for opt in &group_options[label] {
            options_flat.push(format!("{}: {}", label, opt.label));
        }
    }

    let selected = inquire::MultiSelect::new(
        "Select optional components to install:",
        options_flat.clone(),
    )
    .with_default(&initial_indices)
    .prompt()
    .map_err(|e| anyhow!("component selection failed: {}", e))?;

    let mut new_selection = crate::core::manifest::SelectedComponents {
        skills: vec![],
        reviewers: vec![],
    };
    let mut selected_encoded = std::collections::HashSet::new();
    for s in &selected {
        let parts: Vec<_> = s.splitn(2, ": ").collect();
        if parts.len() == 2 {
            let ty = if parts[0] == "Skills" {
                "skill"
            } else {
                "reviewer"
            };
            let name = parts[1];
            selected_encoded.insert(format!("{}:{}", ty, name));
            if ty == "skill" {
                new_selection.skills.push(name.to_string());
            } else {
                new_selection.reviewers.push(name.to_string());
            }
        }
    }

    let additions: Vec<_> = all_values
        .iter()
        .filter(|v| selected_encoded.contains(*v) && !current_values.contains(*v))
        .cloned()
        .collect();
    let removals: Vec<_> = all_values
        .iter()
        .filter(|v| !selected_encoded.contains(*v) && current_values.contains(*v))
        .cloned()
        .collect();

    if additions.is_empty() && removals.is_empty() {
        println!("No changes to component selection.");
        println!("Done.");
        return Ok(());
    }

    let mut updated_manifest_files = manifest.files.clone();
    let mut files_added = 0usize;
    let mut files_removed = 0usize;
    let enabled_tools = manifest.enabled_tools.clone();

    // Handle additions
    for value in &additions {
        let Some((ty, name)) = decode_component_value(value) else {
            continue;
        };
        let component_files = get_component_files(&templates, &name, ty);
        for (relative_path, content) in component_files {
            if !enabled_tools.is_empty() {
                let mut destinations = HashMap::new();
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
                }
                updated_manifest_files.insert(
                    relative_path.clone(),
                    FileEntry {
                        hash: crate::core::manifest::hash_content(&content),
                        destinations,
                    },
                );
                files_added += 1;
                println!("{} {}", "added".green(), relative_path);
            } else {
                let full_path = project_root.join(&relative_path);
                if !is_safe_path(&resolved_root, &full_path) {
                    continue;
                }
                if let Some(parent) = full_path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                let mut prompter = crate::core::prompt::InquirePrompter;
                match install_file(&full_path, &relative_path, &content, &mut prompter).await {
                    Ok(crate::core::files::InstallStatus::Written { hash })
                    | Ok(crate::core::files::InstallStatus::Matched { hash }) => {
                        updated_manifest_files.insert(
                            relative_path.clone(),
                            FileEntry {
                                hash,
                                destinations: HashMap::new(),
                            },
                        );
                        files_added += 1;
                        println!("{} {}", "added".green(), relative_path);
                    }
                    Ok(crate::core::files::InstallStatus::Skipped { .. }) => {}
                    Ok(crate::core::files::InstallStatus::Cancelled) => {
                        println!("Cancelled.");
                        return Ok(());
                    }
                    Err(e) => eprintln!("Error: {}", e),
                }
            }
        }
    }

    // Handle removals
    let mut removed_dirs = std::collections::HashSet::new();
    for value in &removals {
        let Some((ty, name)) = decode_component_value(value) else {
            continue;
        };
        let component_files = get_component_files(&templates, &name, ty);
        for (relative_path, _) in component_files {
            let Some(entry) = manifest.files.get(&relative_path) else {
                continue;
            };

            if !enabled_tools.is_empty() && !entry.destinations.is_empty() {
                let mut any_removed = false;
                for dest_path in entry.destinations.values() {
                    let full_dest = project_root.join(dest_path);
                    if !is_safe_path(&resolved_root, &full_dest) {
                        continue;
                    }
                    if !full_dest.exists() {
                        any_removed = true;
                        continue;
                    }
                    let modified =
                        is_destination_modified(&project_root, dest_path, &entry.hash).await;
                    if modified {
                        let should_remove = inquire::Confirm::new(&format!(
                            "{} has local modifications. Remove it anyway?",
                            dest_path
                        ))
                        .with_default(false)
                        .prompt()
                        .unwrap_or(false);
                        if !should_remove {
                            println!("{} {}", "kept".dimmed(), dest_path);
                            continue;
                        }
                    }
                    let _ = tokio::fs::remove_file(&full_dest).await;
                    any_removed = true;
                    println!("{} {}", "removed".red(), dest_path);
                    let mut dir = full_dest.parent().map(|p| p.to_path_buf());
                    while let Some(d) = dir {
                        if !d.starts_with(&resolved_root) || d == resolved_root {
                            break;
                        }
                        removed_dirs.insert(d.clone());
                        dir = d.parent().map(|p| p.to_path_buf());
                    }
                }
                if any_removed {
                    updated_manifest_files.remove(&relative_path);
                    files_removed += 1;
                }
            } else {
                let full_path = project_root.join(&relative_path);
                if !is_safe_path(&resolved_root, &full_path) {
                    continue;
                }
                if !full_path.exists() {
                    updated_manifest_files.remove(&relative_path);
                    continue;
                }
                let locally_modified =
                    is_locally_modified(&project_root, &relative_path, &manifest).await;
                if locally_modified {
                    let should_remove = inquire::Confirm::new(&format!(
                        "{} has local modifications. Remove it anyway?",
                        relative_path
                    ))
                    .with_default(false)
                    .prompt()
                    .unwrap_or(false);
                    if !should_remove {
                        println!("{} {}", "kept".dimmed(), relative_path);
                        continue;
                    }
                }
                let _ = tokio::fs::remove_file(&full_path).await;
                updated_manifest_files.remove(&relative_path);
                files_removed += 1;
                println!("{} {}", "removed".red(), relative_path);
                let mut dir = full_path.parent().map(|p| p.to_path_buf());
                while let Some(d) = dir {
                    if !d.starts_with(&resolved_root) || d == resolved_root {
                        break;
                    }
                    removed_dirs.insert(d.clone());
                    dir = d.parent().map(|p| p.to_path_buf());
                }
            }
        }
    }

    // Remove empty directories (deepest first)
    let mut dirs: Vec<_> = removed_dirs.into_iter().collect();
    dirs.sort_by_key(|b| std::cmp::Reverse(b.as_os_str().len()));
    for dir in dirs {
        let _ = tokio::fs::remove_dir(&dir).await;
    }

    let updated_manifest = Manifest {
        version: manifest.version,
        installed_at: manifest.installed_at,
        updated_at: chrono::Utc::now().to_rfc3339(),
        enabled_tools: manifest.enabled_tools,
        selected_components: Some(new_selection),
        files: updated_manifest_files,
    };
    write_manifest(&project_root, &updated_manifest).await?;

    match regenerate_tool_configs(&project_root, &updated_manifest).await {
        Ok(regenerated) if !regenerated.is_empty() => {
            println!("Updated MCP config for {}", regenerated.join(", "));
        }
        Ok(_) => {}
        Err(e) => {
            println!(
                "{} Could not regenerate tool configs: {}",
                "Warning:".yellow(),
                e
            );
        }
    }

    let mut parts = Vec::new();
    if files_added > 0 {
        parts.push(format!("{} file(s) added", files_added.to_string().green()));
    }
    if files_removed > 0 {
        parts.push(format!(
            "{} file(s) removed",
            files_removed.to_string().red()
        ));
    }
    println!(
        "{}",
        if parts.is_empty() {
            "Done!".to_string()
        } else {
            format!("Done! {}.", parts.join(", "))
        }
    );
    Ok(())
}
