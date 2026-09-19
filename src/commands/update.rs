use crate::adapters::{get_adapter, regenerate_tool_configs};
use crate::core::components::{get_component_for_file, get_selected_components};
use crate::core::files::is_safe_path;
use crate::core::manifest::{
    FileEntry, Manifest, hash_content, is_destination_modified, is_locally_modified, read_manifest,
    write_manifest,
};
use crate::core::prompt::{InquirePrompter, PromptOption, Prompter};
use crate::core::templates::fetch_templates;
use anyhow::Result;
use owo_colors::OwoColorize;
use similar::TextDiff;
use std::collections::HashMap;

/// Removes now-empty parent directories of `full_path`, walking up towards
/// `resolved_root` but never removing `resolved_root` itself.
async fn remove_empty_parents(full_path: &std::path::Path, resolved_root: &std::path::Path) {
    let mut dir = full_path.parent().map(|p| p.to_path_buf());
    while let Some(d) = dir {
        if d == resolved_root || !d.starts_with(resolved_root) {
            break;
        }
        if tokio::fs::remove_dir(&d).await.is_err() {
            break; // not empty (or already gone) — stop walking up
        }
        dir = d.parent().map(|p| p.to_path_buf());
    }
}

pub async fn run(ref_: Option<String>) -> Result<()> {
    let project_root = std::env::current_dir()?;
    let resolved_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.clone());

    println!("{}", "Pan-Pipe — Update".bold());

    let manifest = read_manifest(&project_root).await?.ok_or_else(|| {
        anyhow::anyhow!("Pan-Pipe is not initialized in this project. Run \"pan-pipe init\" first.")
    })?;

    let templates = fetch_templates(ref_.as_deref()).await?;
    println!("Fetched {} template files", templates.len());

    let current_selection =
        get_selected_components(manifest.selected_components.as_ref(), &templates);
    let selected_skill_names: std::collections::HashSet<_> =
        current_selection.skills.iter().cloned().collect();
    let selected_reviewer_names: std::collections::HashSet<_> =
        current_selection.reviewers.iter().cloned().collect();

    let renames = crate::core::templates::detect_skill_renames(
        manifest.files.keys().map(|s| s.as_str()),
        &templates,
    );
    let rename_targets: std::collections::HashSet<&str> =
        renames.iter().map(|(_, new)| new.as_str()).collect();
    let rename_sources: std::collections::HashSet<&str> =
        renames.iter().map(|(old, _)| old.as_str()).collect();

    let mut new_files = Vec::new();
    let mut removed_files = Vec::new();
    let mut changed_files = Vec::new();
    let mut new_unselected_components = std::collections::HashSet::new();

    for (relative_path, content) in &templates {
        let new_hash = hash_content(content);
        match manifest.files.get(relative_path) {
            None => {
                if rename_targets.contains(relative_path.as_str()) {
                    continue;
                }
                if let Some((ty, name)) = get_component_for_file(relative_path) {
                    let is_selected = match ty {
                        crate::core::components::ComponentType::Skill => {
                            selected_skill_names.contains(&name)
                        }
                        crate::core::components::ComponentType::Reviewer => {
                            selected_reviewer_names.contains(&name)
                        }
                    };
                    if !is_selected {
                        new_unselected_components.insert(name);
                        continue;
                    }
                }
                new_files.push((relative_path.clone(), content.clone(), new_hash));
            }
            Some(entry) if entry.hash != new_hash => {
                if let Some((ty, name)) = get_component_for_file(relative_path) {
                    let is_selected = match ty {
                        crate::core::components::ComponentType::Skill => {
                            selected_skill_names.contains(&name)
                        }
                        crate::core::components::ComponentType::Reviewer => {
                            selected_reviewer_names.contains(&name)
                        }
                    };
                    if !is_selected {
                        continue;
                    }
                }
                changed_files.push((relative_path.clone(), content.clone(), new_hash));
            }
            _ => {}
        }
    }

    for relative_path in manifest.files.keys() {
        if rename_sources.contains(relative_path.as_str()) {
            continue;
        }
        if !templates.contains_key(relative_path) {
            removed_files.push(relative_path.clone());
        }
    }

    if new_files.is_empty()
        && changed_files.is_empty()
        && removed_files.is_empty()
        && renames.is_empty()
    {
        if !new_unselected_components.is_empty() {
            println!(
                "{} new optional component(s) available. Run `pan-pipe components` to review.",
                new_unselected_components.len()
            );
        }
        println!("Everything is up to date!");
        return Ok(());
    }

    if !new_files.is_empty() {
        println!("{} new file(s) to add", new_files.len().to_string().green());
    }
    if !changed_files.is_empty() {
        println!(
            "{} file(s) changed",
            changed_files.len().to_string().yellow()
        );
    }
    if !removed_files.is_empty() {
        println!("{} file(s) removed", removed_files.len().to_string().red());
    }
    if !renames.is_empty() {
        println!("{} file(s) renamed", renames.len().to_string().cyan());
    }

    let mut updated_manifest_files = manifest.files.clone();
    let mut added = 0usize;
    let mut updated = 0usize;
    let mut removed = 0usize;
    let mut renamed = 0usize;
    let mut skipped = 0usize;
    let mut manifest_dirty = false;
    let enabled_tools = manifest.enabled_tools.clone();

    // Handle new files
    for (relative_path, content, hash) in new_files {
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
            updated_manifest_files.insert(relative_path.clone(), FileEntry { hash, destinations });
            added += 1;
            println!("{} {}", "added".green(), relative_path);
        } else {
            let full_path = project_root.join(&relative_path);
            if !is_safe_path(&resolved_root, &full_path) {
                continue;
            }
            if let Some(parent) = full_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&full_path, &content).await?;
            updated_manifest_files.insert(
                relative_path.clone(),
                FileEntry {
                    hash,
                    destinations: HashMap::new(),
                },
            );
            added += 1;
            println!("{} {}", "added".green(), relative_path);
        }
    }

    // Handle renamed files (px-* → pp-*)
    for (old_key, new_key) in &renames {
        let Some(content) = templates.get(new_key) else {
            continue;
        };
        let new_hash = hash_content(content);
        let entry = manifest.files.get(old_key).cloned().unwrap_or_default();

        if !enabled_tools.is_empty() {
            // Remove old destination files (all tools recorded in the manifest entry).
            for old_dest in entry.destinations.values() {
                let full_dest = project_root.join(old_dest);
                if !is_safe_path(&resolved_root, &full_dest) || !full_dest.exists() {
                    continue;
                }
                if is_destination_modified(&project_root, old_dest, &entry.hash).await {
                    println!(
                        "{} {} {}",
                        "kept".dimmed(),
                        old_dest,
                        "(locally modified)".yellow()
                    );
                    continue;
                }
                let _ = tokio::fs::remove_file(&full_dest).await;
                remove_empty_parents(&full_dest, &resolved_root).await;
            }
            // Install the renamed file for every enabled tool.
            let mut new_destinations = HashMap::new();
            for tool_name in &enabled_tools {
                let Some(adapter) = get_adapter(tool_name) else {
                    continue;
                };
                let Some(dest_path) = adapter.destination_path(new_key) else {
                    continue;
                };
                let full_path = project_root.join(&dest_path);
                if !is_safe_path(&resolved_root, &full_path) {
                    continue;
                }
                if let Some(parent) = full_path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::write(&full_path, content).await?;
                new_destinations.insert(tool_name.clone(), dest_path.to_string_lossy().to_string());
            }
            updated_manifest_files.remove(old_key);
            updated_manifest_files.insert(
                new_key.clone(),
                FileEntry {
                    hash: new_hash,
                    destinations: new_destinations,
                },
            );
            renamed += 1;
            println!("{} {} → {}", "renamed".cyan(), old_key, new_key);
        } else {
            // Legacy manifest without tool destinations: move files under praxis/.
            let old_full = project_root.join(old_key);
            let old_existed_and_modified = old_full.exists()
                && is_safe_path(&resolved_root, &old_full)
                && is_locally_modified(&project_root, old_key, &manifest).await;
            if old_existed_and_modified {
                println!(
                    "{} {} {}",
                    "kept".dimmed(),
                    old_key,
                    "(locally modified)".yellow()
                );
            } else if old_full.exists() && is_safe_path(&resolved_root, &old_full) {
                let _ = tokio::fs::remove_file(&old_full).await;
                remove_empty_parents(&old_full, &resolved_root).await;
            }
            let new_full = project_root.join(new_key);
            if is_safe_path(&resolved_root, &new_full) {
                if let Some(parent) = new_full.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::write(&new_full, content).await?;
                updated_manifest_files.remove(old_key);
                updated_manifest_files.insert(
                    new_key.clone(),
                    FileEntry {
                        hash: new_hash,
                        destinations: HashMap::new(),
                    },
                );
                renamed += 1;
                println!("{} {} → {}", "renamed".cyan(), old_key, new_key);
            }
        }
    }

    // Handle changed files
    let mut prompter = InquirePrompter;
    for (relative_path, content, new_hash) in changed_files {
        if !enabled_tools.is_empty() {
            let entry = manifest
                .files
                .get(&relative_path)
                .cloned()
                .unwrap_or_default();
            let old_destinations = entry.destinations.clone();
            let mut new_destinations = HashMap::new();
            let mut any_updated = false;

            for tool_name in &enabled_tools {
                let Some(adapter) = get_adapter(tool_name) else {
                    continue;
                };
                let Some(dest_path) = adapter.destination_path(&relative_path) else {
                    continue;
                };
                let dest_path_str = dest_path.to_string_lossy().to_string();
                let full_dest = project_root.join(&dest_path);
                if !is_safe_path(&resolved_root, &full_dest) {
                    continue;
                }

                let dest_exists = full_dest.exists();
                let modified = if dest_exists {
                    is_destination_modified(&project_root, &dest_path_str, &entry.hash).await
                } else {
                    false
                };

                if !dest_exists || !modified {
                    if let Some(parent) = full_dest.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }
                    tokio::fs::write(&full_dest, &content).await?;
                    new_destinations.insert(tool_name.clone(), dest_path_str);
                    any_updated = true;
                } else {
                    let local_content = tokio::fs::read_to_string(&full_dest)
                        .await
                        .unwrap_or_default();
                    let options = vec![
                        PromptOption {
                            value: "overwrite".to_string(),
                            label: "Overwrite with new Pan-Pipe version".to_string(),
                        },
                        PromptOption {
                            value: "skip".to_string(),
                            label: "Keep your version".to_string(),
                        },
                        PromptOption {
                            value: "diff".to_string(),
                            label: "Show diff, then decide".to_string(),
                        },
                    ];
                    let action = prompter.select(&format!("{} has local changes and a new Pan-Pipe version. What would you like to do?", dest_path_str), &options).await?;
                    let mut action_str = match &action {
                        crate::core::prompt::PromptResult::Value(v) => v.clone(),
                        _ => {
                            println!("Cancelled.");
                            return Ok(());
                        }
                    };

                    if action_str == "diff" {
                        let diff = TextDiff::from_lines(&local_content, &content);
                        let patch = diff
                            .unified_diff()
                            .header("your version", "new pan-pipe version")
                            .to_string();
                        prompter.log_info(&patch);
                        let options2 = vec![
                            PromptOption {
                                value: "overwrite".to_string(),
                                label: "Overwrite with new Pan-Pipe version".to_string(),
                            },
                            PromptOption {
                                value: "skip".to_string(),
                                label: "Keep your version".to_string(),
                            },
                        ];
                        let action2 = prompter
                            .select(&format!("Overwrite {}?", dest_path_str), &options2)
                            .await?;
                        action_str = match &action2 {
                            crate::core::prompt::PromptResult::Value(v) => v.clone(),
                            _ => {
                                println!("Cancelled.");
                                return Ok(());
                            }
                        };
                    }

                    if action_str == "overwrite" {
                        tokio::fs::write(&full_dest, &content).await?;
                        new_destinations.insert(tool_name.clone(), dest_path_str);
                        any_updated = true;
                    } else if let Some(old) = old_destinations.get(tool_name) {
                        new_destinations.insert(tool_name.clone(), old.clone());
                        skipped += 1;
                    }
                }
            }

            let effective_hash = if any_updated { new_hash } else { entry.hash };
            updated_manifest_files.insert(
                relative_path.clone(),
                FileEntry {
                    hash: effective_hash,
                    destinations: new_destinations,
                },
            );
            if any_updated {
                updated += 1;
                println!("{} {}", "updated".yellow(), relative_path);
            }
        } else {
            let full_path = project_root.join(&relative_path);
            if !is_safe_path(&resolved_root, &full_path) {
                continue;
            }
            let locally_modified = if full_path.exists() {
                is_locally_modified(&project_root, &relative_path, &manifest).await
            } else {
                false
            };

            if !locally_modified {
                tokio::fs::write(&full_path, &content).await?;
                updated_manifest_files.insert(
                    relative_path.clone(),
                    FileEntry {
                        hash: new_hash,
                        destinations: HashMap::new(),
                    },
                );
                updated += 1;
                println!("{} {}", "updated".yellow(), relative_path);
            } else {
                let local_content = tokio::fs::read_to_string(&full_path)
                    .await
                    .unwrap_or_default();
                let options = vec![
                    PromptOption {
                        value: "overwrite".to_string(),
                        label: "Overwrite with new Pan-Pipe version".to_string(),
                    },
                    PromptOption {
                        value: "skip".to_string(),
                        label: "Keep your version".to_string(),
                    },
                    PromptOption {
                        value: "diff".to_string(),
                        label: "Show diff, then decide".to_string(),
                    },
                ];
                let action = prompter.select(&format!("{} has local changes and a new Pan-Pipe version. What would you like to do?", relative_path), &options).await?;
                let mut action_str = match &action {
                    crate::core::prompt::PromptResult::Value(v) => v.clone(),
                    _ => {
                        println!("Cancelled.");
                        return Ok(());
                    }
                };

                if action_str == "diff" {
                    let diff = TextDiff::from_lines(&local_content, &content);
                    let patch = diff
                        .unified_diff()
                        .header("your version", "new pan-pipe version")
                        .to_string();
                    prompter.log_info(&patch);
                    let options2 = vec![
                        PromptOption {
                            value: "overwrite".to_string(),
                            label: "Overwrite with new Pan-Pipe version".to_string(),
                        },
                        PromptOption {
                            value: "skip".to_string(),
                            label: "Keep your version".to_string(),
                        },
                    ];
                    let action2 = prompter
                        .select(&format!("Overwrite {}?", relative_path), &options2)
                        .await?;
                    action_str = match &action2 {
                        crate::core::prompt::PromptResult::Value(v) => v.clone(),
                        _ => {
                            println!("Cancelled.");
                            return Ok(());
                        }
                    };
                }

                if action_str == "overwrite" {
                    tokio::fs::write(&full_path, &content).await?;
                    updated_manifest_files.insert(
                        relative_path.clone(),
                        FileEntry {
                            hash: new_hash,
                            destinations: HashMap::new(),
                        },
                    );
                    updated += 1;
                    println!("{} {}", "updated".yellow(), relative_path);
                } else {
                    skipped += 1;
                    println!("{} {}", "skipped".dimmed(), relative_path);
                }
            }
        }
    }

    // Handle removed files
    for relative_path in removed_files {
        if let Some((ty, name)) = get_component_for_file(&relative_path) {
            let is_selected = match ty {
                crate::core::components::ComponentType::Skill => {
                    selected_skill_names.contains(&name)
                }
                crate::core::components::ComponentType::Reviewer => {
                    selected_reviewer_names.contains(&name)
                }
            };
            if !is_selected {
                updated_manifest_files.remove(&relative_path);
                manifest_dirty = true;
                continue;
            }
        }

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
                let modified = is_destination_modified(&project_root, dest_path, &entry.hash).await;
                let warning = if modified {
                    format!(" {}", "(locally modified)".yellow())
                } else {
                    String::new()
                };
                let should_remove = inquire::Confirm::new(&format!(
                    "{} was removed from Pan-Pipe.{} Delete it?",
                    dest_path, warning
                ))
                .with_default(true)
                .prompt()
                .unwrap_or(false);
                if should_remove {
                    let _ = tokio::fs::remove_file(&full_dest).await;
                    any_removed = true;
                    println!("{} {}", "removed".red(), dest_path);
                } else {
                    skipped += 1;
                    println!("{} {}", "skipped".dimmed(), dest_path);
                }
            }
            if any_removed {
                updated_manifest_files.remove(&relative_path);
                removed += 1;
            }
        } else {
            let full_path = project_root.join(&relative_path);
            if !is_safe_path(&resolved_root, &full_path) {
                continue;
            }
            if !full_path.exists() {
                updated_manifest_files.remove(&relative_path);
                removed += 1;
                continue;
            }
            let locally_modified =
                is_locally_modified(&project_root, &relative_path, &manifest).await;
            let warning = if locally_modified {
                format!(" {}", "(locally modified)".yellow())
            } else {
                String::new()
            };
            let should_remove = inquire::Confirm::new(&format!(
                "{} was removed from Pan-Pipe.{} Delete it?",
                relative_path, warning
            ))
            .with_default(true)
            .prompt()
            .unwrap_or(false);
            if should_remove {
                let _ = tokio::fs::remove_file(&full_path).await;
                updated_manifest_files.remove(&relative_path);
                removed += 1;
                println!("{} {}", "removed".red(), relative_path);
            } else {
                skipped += 1;
                println!("{} {}", "skipped".dimmed(), relative_path);
            }
        }
    }

    let needs_write = added > 0
        || updated > 0
        || removed > 0
        || renamed > 0
        || manifest_dirty
        || manifest.selected_components.is_none();
    let updated_manifest = Manifest {
        version: manifest.version,
        installed_at: manifest.installed_at,
        updated_at: chrono::Utc::now().to_rfc3339(),
        enabled_tools: manifest.enabled_tools,
        selected_components: Some(crate::core::manifest::SelectedComponents {
            skills: current_selection.skills,
            reviewers: current_selection.reviewers,
        }),
        files: updated_manifest_files,
    };

    if needs_write {
        write_manifest(&project_root, &updated_manifest).await?;
    }

    if needs_write {
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
    }

    let mut parts = Vec::new();
    if added > 0 {
        parts.push(format!("{} added", added.to_string().green()));
    }
    if updated > 0 {
        parts.push(format!("{} updated", updated.to_string().yellow()));
    }
    if removed > 0 {
        parts.push(format!("{} removed", removed.to_string().red()));
    }
    if renamed > 0 {
        parts.push(format!("{} renamed", renamed.to_string().cyan()));
    }
    if skipped > 0 {
        parts.push(format!("{} skipped", skipped.to_string().dimmed()));
    }

    if !new_unselected_components.is_empty() {
        println!(
            "{} new optional component(s) available. Run `pan-pipe components` to review.",
            new_unselected_components.len()
        );
    }

    println!("Update complete! {}", parts.join(", "));
    Ok(())
}
