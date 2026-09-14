use crate::adapters::{collect_mcp_config, get_adapter, list_adapters, write_mcp_config_file};
use crate::core::files::is_safe_path;
use crate::core::manifest::{Manifest, read_manifest, write_manifest};
use crate::core::templates::fetch_templates;
use anyhow::{Result, anyhow};
use owo_colors::OwoColorize;

pub async fn add(names: Vec<String>, ref_: Option<String>) -> Result<()> {
    let project_root = std::env::current_dir()?;
    let resolved_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.clone());

    println!("{}", "Pan-Pipe — Tool Add".bold());

    let manifest = read_manifest(&project_root).await?.ok_or_else(|| {
        anyhow!("Pan-Pipe is not initialized in this project. Run \"pan-pipe init\" first.")
    })?;

    let all_adapters = list_adapters();
    let enabled_tools: std::collections::HashSet<String> =
        manifest.enabled_tools.iter().cloned().collect();

    let selected_names = if names.is_empty() {
        let options: Vec<String> = all_adapters
            .iter()
            .map(|(name, display)| format!("{} ({})", display, name))
            .collect();
        let ans = inquire::MultiSelect::new("Select tools to configure:", options.clone())
            .with_default(
                &enabled_tools
                    .iter()
                    .filter_map(|n| {
                        options
                            .iter()
                            .position(|o| o.ends_with(&format!("({})", n)))
                    })
                    .collect::<Vec<_>>(),
            )
            .prompt()?;
        ans.iter()
            .map(|s| {
                s.split('(')
                    .nth(1)
                    .unwrap()
                    .trim_end_matches(')')
                    .to_string()
            })
            .collect()
    } else {
        let valid: std::collections::HashSet<_> = all_adapters.iter().map(|(n, _)| *n).collect();
        for name in &names {
            if !valid.contains(name.as_str()) {
                anyhow::bail!(
                    "Unknown tool \"{}\". Available: {}",
                    name,
                    valid.into_iter().collect::<Vec<_>>().join(", ")
                );
            }
        }
        names
    };

    if selected_names.is_empty() {
        println!("No tools selected.");
        return Ok(());
    }

    let templates = fetch_templates(ref_.as_deref()).await?;
    println!("Fetched {} template files", templates.len());

    let mut updated_manifest_files = manifest.files.clone();
    let mut files_installed = 0;

    let new_tools: Vec<_> = selected_names
        .iter()
        .filter(|n| !manifest.enabled_tools.contains(n))
        .cloned()
        .collect();

    for (source_path, entry) in &manifest.files {
        let Some(content) = templates.get(source_path) else {
            continue;
        };
        let tools_to_install = if new_tools.is_empty() {
            &selected_names[..]
        } else {
            &new_tools[..]
        };
        let existing_destinations = entry.destinations.clone();
        let mut new_destinations = existing_destinations.clone();

        for tool_name in tools_to_install {
            let Some(adapter) = get_adapter(tool_name) else {
                continue;
            };
            let Some(dest_path) = adapter.destination_path(source_path) else {
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
            files_installed += 1;
        }

        updated_manifest_files.insert(
            source_path.clone(),
            crate::core::manifest::FileEntry {
                hash: entry.hash.clone(),
                destinations: new_destinations,
            },
        );
    }

    let mut new_enabled = enabled_tools;
    for name in &selected_names {
        new_enabled.insert(name.clone());
    }

    let updated_manifest = Manifest {
        version: manifest.version,
        installed_at: manifest.installed_at,
        updated_at: chrono::Utc::now().to_rfc3339(),
        enabled_tools: new_enabled.into_iter().collect(),
        selected_components: manifest.selected_components,
        files: updated_manifest_files,
    };
    write_manifest(&project_root, &updated_manifest).await?;

    // Write MCP configs
    let mcp_config = collect_mcp_config(&project_root, &updated_manifest).await?;
    let mut mcp_written = 0;
    for name in &selected_names {
        let Some(adapter) = get_adapter(name) else {
            continue;
        };
        if let Some(entry) = adapter.mcp_config(&mcp_config) {
            let full_path = project_root.join(&entry.path);
            if is_safe_path(&resolved_root, &full_path) {
                write_mcp_config_file(&full_path, &entry).await?;
                println!("{} {}", "written".green(), entry.path);
                mcp_written += 1;
            }
        }
    }

    println!(
        "Done! {} file(s) written for {}.",
        (files_installed + mcp_written).to_string().green(),
        selected_names.join(", ")
    );
    Ok(())
}

pub async fn remove(names: Vec<String>) -> Result<()> {
    let project_root = std::env::current_dir()?;
    let resolved_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.clone());

    println!("{}", "Pan-Pipe — Tool Remove".bold());

    let manifest = read_manifest(&project_root).await?.ok_or_else(|| {
        anyhow!("Pan-Pipe is not initialized in this project. Run \"pan-pipe init\" first.")
    })?;

    if names.is_empty() {
        anyhow::bail!("Please specify one or more tool names to remove.");
    }

    let valid: std::collections::HashSet<_> = list_adapters().iter().map(|(n, _)| *n).collect();
    for name in &names {
        if !valid.contains(name.as_str()) {
            anyhow::bail!(
                "Unknown tool \"{}\". Available: {}",
                name,
                valid.into_iter().collect::<Vec<_>>().join(", ")
            );
        }
    }

    let mut updated_manifest_files = manifest.files.clone();
    let mut total_removed = 0usize;
    let mut total_skipped = 0usize;
    let mut removed_dirs = std::collections::HashSet::new();

    for name in &names {
        let Some(_adapter) = get_adapter(name) else {
            continue;
        };
        for (source_path, entry) in &manifest.files {
            let Some(dest_path) = entry.destinations.get(name) else {
                continue;
            };
            let full_dest = project_root.join(dest_path);
            if !is_safe_path(&resolved_root, &full_dest) {
                continue;
            }

            if !full_dest.exists() {
                let mut new_dests = entry.destinations.clone();
                new_dests.remove(name);
                updated_manifest_files.insert(
                    source_path.clone(),
                    crate::core::manifest::FileEntry {
                        hash: entry.hash.clone(),
                        destinations: new_dests,
                    },
                );
                continue;
            }

            let modified = crate::core::manifest::is_destination_modified(
                &project_root,
                dest_path,
                &entry.hash,
            )
            .await;

            if modified {
                println!(
                    "{} {} {}",
                    "skipped".dimmed(),
                    dest_path,
                    "(locally modified)".yellow()
                );
                total_skipped += 1;
                continue;
            }

            tokio::fs::remove_file(&full_dest).await?;
            total_removed += 1;
            println!("{} {}", "removed".red(), dest_path);

            let mut dir = full_dest.parent().map(|p| p.to_path_buf());
            while let Some(d) = dir {
                if !d.starts_with(&resolved_root) || d == resolved_root {
                    break;
                }
                removed_dirs.insert(d.clone());
                dir = d.parent().map(|p| p.to_path_buf());
            }

            let mut new_dests = entry.destinations.clone();
            new_dests.remove(name);
            updated_manifest_files.insert(
                source_path.clone(),
                crate::core::manifest::FileEntry {
                    hash: entry.hash.clone(),
                    destinations: new_dests,
                },
            );
        }
    }

    // Remove empty directories (deepest first)
    let mut dirs: Vec<_> = removed_dirs.into_iter().collect();
    dirs.sort_by_key(|b| std::cmp::Reverse(b.as_os_str().len()));
    for dir in dirs {
        let _ = tokio::fs::remove_dir(&dir).await;
    }

    // Remove MCP configs
    let mcp_config = collect_mcp_config(&project_root, &manifest).await?;
    for name in &names {
        let Some(adapter) = get_adapter(name) else {
            continue;
        };
        if let Some(entry) = adapter.mcp_config(&mcp_config) {
            let full_path = project_root.join(&entry.path);
            if !is_safe_path(&resolved_root, &full_path) {
                continue;
            }
            if !full_path.exists() {
                continue;
            }
            if let Some(ref merge_key) = entry.merge_key {
                let existing_raw = tokio::fs::read_to_string(&full_path)
                    .await
                    .unwrap_or_default();
                if let Ok(mut existing) = serde_json::from_str::<serde_json::Value>(&existing_raw) {
                    if let Some(obj) = existing.as_object_mut() {
                        obj.remove(merge_key);
                        if obj.is_empty() {
                            let _ = tokio::fs::remove_file(&full_path).await;
                        } else {
                            let raw = serde_json::to_string_pretty(&existing)? + "\n";
                            tokio::fs::write(&full_path, raw).await?;
                        }
                        println!("{} {}", "removed".red(), entry.path);
                        continue;
                    }
                }
            }
            let _ = tokio::fs::remove_file(&full_path).await;
            println!("{} {}", "removed".red(), entry.path);
            if let Some(parent) = full_path.parent() {
                let _ = tokio::fs::remove_dir(parent).await;
            }
        }
    }

    let mut enabled_tools: std::collections::HashSet<_> =
        manifest.enabled_tools.iter().cloned().collect();
    for name in &names {
        enabled_tools.remove(name);
    }

    let updated_manifest = Manifest {
        version: manifest.version,
        installed_at: manifest.installed_at,
        updated_at: chrono::Utc::now().to_rfc3339(),
        enabled_tools: enabled_tools.into_iter().collect(),
        selected_components: manifest.selected_components,
        files: updated_manifest_files,
    };
    write_manifest(&project_root, &updated_manifest).await?;

    let mut parts = Vec::new();
    if total_removed > 0 {
        parts.push(format!("{} removed", total_removed.to_string().red()));
    }
    if total_skipped > 0 {
        parts.push(format!("{} skipped", total_skipped.to_string().yellow()));
    }
    println!(
        "Done! {} for {}.",
        if parts.is_empty() {
            "0 file(s) removed".to_string()
        } else {
            parts.join(", ")
        },
        names.join(", ")
    );
    Ok(())
}

pub async fn list() -> Result<()> {
    let project_root = std::env::current_dir()?;

    println!("{}", "Pan-Pipe — Tool List".bold());

    let manifest = read_manifest(&project_root).await?.ok_or_else(|| {
        anyhow!("Pan-Pipe is not initialized in this project. Run \"pan-pipe init\" first.")
    })?;

    let enabled_tools: std::collections::HashSet<_> =
        manifest.enabled_tools.iter().cloned().collect();
    let all_adapters = list_adapters();

    for (name, display) in &all_adapters {
        let status = if enabled_tools.contains(*name) {
            "enabled".green().to_string()
        } else {
            "disabled".dimmed().to_string()
        };
        println!("{} ({}) — {}", display, name, status);
    }

    println!("{} tool adapter(s) available.", all_adapters.len());
    Ok(())
}
