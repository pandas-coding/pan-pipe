#![allow(dead_code)]

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use tar::Archive;

const TARBALL_BASE_URL: &str = "https://api.github.com/repos/DFilipeS/praxis/tarball";
const MAX_DOWNLOAD_SIZE: usize = 10 * 1024 * 1024; // 10 MB

/// Legacy skill prefix used by the upstream praxis templates.
const LEGACY_SKILL_PREFIX: &str = "px-";
/// Skill prefix used for pan-pipe installs.
const SKILL_PREFIX: &str = "pp-";

pub async fn fetch_templates(ref_: Option<&str>) -> Result<BTreeMap<String, String>> {
    let ref_ = ref_.unwrap_or("main");
    let encoded_ref = urlencoding::encode(ref_);
    let url = format!("{}/{}", TARBALL_BASE_URL, encoded_ref);

    if crate::is_verbose() {
        eprintln!("[verbose] fetching templates from {}", url);
    }

    let client = reqwest::Client::builder()
        .user_agent("pan-pipe")
        .build()
        .context("failed to build HTTP client")?;

    let res = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to fetch tarball from {}", url))?;

    let status = res.status();
    if !status.is_success() {
        let hint = if status.as_u16() == 403 {
            " You may be rate-limited."
        } else {
            ""
        };
        anyhow::bail!("GitHub API returned status {}{}", status, hint);
    }

    let content_length = res.content_length().unwrap_or(0);
    if content_length > MAX_DOWNLOAD_SIZE as u64 {
        anyhow::bail!(
            "Response too large ({} bytes). Maximum is {} bytes.",
            content_length,
            MAX_DOWNLOAD_SIZE
        );
    }

    let bytes = res.bytes().await.context("failed to read response body")?;

    if bytes.len() > MAX_DOWNLOAD_SIZE {
        anyhow::bail!(
            "Downloaded content too large ({} bytes). Maximum is {} bytes.",
            bytes.len(),
            MAX_DOWNLOAD_SIZE
        );
    }

    let templates = extract_templates(&bytes)?;
    Ok(apply_skill_prefix_rename(templates))
}

pub fn extract_templates(bytes: &[u8]) -> Result<BTreeMap<String, String>> {
    if crate::is_verbose() {
        eprintln!("[verbose] extracting {} bytes from tarball", bytes.len());
    }
    let decoder = GzDecoder::new(bytes);
    let mut archive = Archive::new(decoder);
    let mut files = BTreeMap::new();

    for entry in archive.entries().context("failed to read tar entries")? {
        let mut entry = entry.context("failed to read tar entry")?;
        let path = entry.path().context("failed to get entry path")?;

        // Strip the first path component (e.g., DFilipeS-praxis-abc123/)
        let stripped: std::path::PathBuf = path.components().skip(1).collect();
        let relative = stripped.to_string_lossy();

        if !relative.starts_with("praxis/") && relative != "praxis" {
            continue;
        }

        if !entry.header().entry_type().is_file() {
            continue;
        }

        let mut content = String::new();
        entry
            .read_to_string(&mut content)
            .context("failed to read file content from tar")?;
        files.insert(relative.into_owned(), content);
    }

    Ok(files)
}

/// Returns the legacy skill directory name (`px-<name>`) when `path` is a
/// file inside one, e.g. `praxis/skills/px-brainstorm/SKILL.md` →
/// `Some("px-brainstorm")`. A file sitting directly at `praxis/skills/px-foo`
/// (no trailing slash) is not a skill directory and yields `None`.
fn legacy_skill_dir(path: &str) -> Option<String> {
    let rest = path.strip_prefix("praxis/skills/")?;
    let seg = rest.split('/').next()?;
    if seg.starts_with(LEGACY_SKILL_PREFIX) && rest.len() > seg.len() {
        Some(seg.to_string())
    } else {
        None
    }
}

fn renamed_skill_segment(legacy_name: &str) -> String {
    format!(
        "{}{}",
        SKILL_PREFIX,
        &legacy_name[LEGACY_SKILL_PREFIX.len()..]
    )
}

/// Rewrites upstream `praxis/skills/px-<name>/...` template paths to
/// `pp-<name>` and replaces `px-<name>` references inside every template
/// content (frontmatter `name:` fields and cross-skill references) so the
/// installed files are self-consistent under the `pp-` prefix. Returns the
/// input unchanged when the templates contain no legacy `px-` skill dirs.
pub fn apply_skill_prefix_rename(templates: BTreeMap<String, String>) -> BTreeMap<String, String> {
    let legacy_names: BTreeSet<String> = templates
        .keys()
        .filter_map(|k| legacy_skill_dir(k))
        .collect();
    if legacy_names.is_empty() {
        return templates;
    }

    if crate::is_verbose() {
        eprintln!(
            "[verbose] renamed {} legacy px-* skill path(s) to pp-*",
            legacy_names.len()
        );
    }

    let mut out = BTreeMap::new();
    for (path, content) in templates {
        let new_path = match legacy_skill_dir(&path) {
            Some(legacy) => {
                let rest = path.strip_prefix("praxis/skills/").unwrap_or_default();
                let after = &rest[legacy.len()..];
                format!("praxis/skills/{}{}", renamed_skill_segment(&legacy), after)
            }
            None => path,
        };

        let mut new_content = content;
        for legacy in &legacy_names {
            if new_content.contains(legacy.as_str()) {
                new_content = new_content.replace(legacy.as_str(), &renamed_skill_segment(legacy));
            }
        }
        out.insert(new_path, new_content);
    }
    out
}

/// Maps a legacy skill source path (`praxis/skills/px-<name>/<rest>`) to its
/// `pp-` counterpart. Returns `None` for any other path shape.
pub fn renamed_skill_source(source: &str) -> Option<String> {
    let legacy = legacy_skill_dir(source)?;
    let rest = source.strip_prefix("praxis/skills/")?;
    Some(format!(
        "praxis/skills/{}{}",
        renamed_skill_segment(&legacy),
        &rest[legacy.len()..]
    ))
}

/// Pairs every manifest file key with its renamed `pp-` counterpart, but only
/// when that counterpart exists in the current templates. The result is
/// sorted by the old key so callers get deterministic output.
pub fn detect_skill_renames<'a>(
    manifest_keys: impl IntoIterator<Item = &'a str>,
    templates: &BTreeMap<String, String>,
) -> Vec<(String, String)> {
    let mut renames: Vec<(String, String)> = manifest_keys
        .into_iter()
        .filter_map(|old| {
            let new = renamed_skill_source(old)?;
            templates.contains_key(&new).then(|| (old.to_string(), new))
        })
        .collect();
    renames.sort();
    renames
}

#[cfg(test)]
mod tests {
    use super::*;

    use tar::Builder;

    fn make_tarball(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let gz = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::default());
            let mut tar = Builder::new(gz);
            for (path, content) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_path(path).unwrap();
                header.set_size(content.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                tar.append(&header, content.as_bytes()).unwrap();
            }
            tar.into_inner().unwrap().finish().unwrap();
        }
        buf
    }

    #[test]
    fn test_extract_templates_filters_praxis() {
        let tarball = make_tarball(&[
            ("DFilipeS-praxis-abc123/praxis/test.md", "# Test"),
            ("DFilipeS-praxis-abc123/README.md", "# Readme"),
        ]);
        let files = extract_templates(&tarball).unwrap();
        assert!(files.contains_key("praxis/test.md"));
        assert_eq!(files.get("praxis/test.md").unwrap(), "# Test");
        assert!(!files.contains_key("README.md"));
    }

    #[test]
    fn test_extract_templates_nested() {
        let tarball = make_tarball(&[
            ("DFilipeS-praxis-abc123/praxis/skill.md", "# Skill"),
            ("DFilipeS-praxis-abc123/praxis/sub/nested.md", "# Nested"),
            ("DFilipeS-praxis-abc123/README.md", "# Readme"),
        ]);
        let files = extract_templates(&tarball).unwrap();
        for key in files.keys() {
            assert!(key.starts_with("praxis/"));
        }
        assert!(files.contains_key("praxis/skill.md"));
        assert!(files.contains_key("praxis/sub/nested.md"));
    }

    #[test]
    fn test_extract_templates_empty() {
        let tarball = make_tarball(&[]);
        let files = extract_templates(&tarball).unwrap();
        assert!(files.is_empty());
    }

    fn make_templates(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_apply_skill_prefix_rename_paths_and_content() {
        let templates = make_templates(&[
            (
                "praxis/skills/px-brainstorm/SKILL.md",
                "---\nname: px-brainstorm\ndescription: \"Mentions px-plan\"\n---\n",
            ),
            (
                "praxis/skills/px-plan/SKILL.md",
                "---\nname: px-plan\n---\nAfter px-brainstorm.\n",
            ),
            (
                "praxis/skills/px-implement/SKILL.md",
                "---\nname: px-implement\n---\nAlways hand off to px-review when done.\n",
            ),
            (
                "praxis/skills/px-review/SKILL.md",
                "---\nname: px-review\n---\n",
            ),
            (
                "praxis/skills/agent-browser/SKILL.md",
                "---\nname: agent-browser\n---\nNo legacy references here.\n",
            ),
            (
                "praxis/agents/codebase-explorer.md",
                "Use the px-brainstorm skill before planning.\n",
            ),
            (
                "praxis/conventions.md",
                "Workflow: px-plan → px-implement.\n",
            ),
        ]);

        let renamed = apply_skill_prefix_rename(templates);

        // Paths renamed only for px- skill dirs.
        assert!(renamed.contains_key("praxis/skills/pp-brainstorm/SKILL.md"));
        assert!(renamed.contains_key("praxis/skills/pp-implement/SKILL.md"));
        assert!(renamed.contains_key("praxis/skills/agent-browser/SKILL.md"));
        assert!(!renamed.contains_key("praxis/skills/px-brainstorm/SKILL.md"));

        // Content rewritten in skill files and in other files referencing them.
        assert!(renamed["praxis/skills/pp-brainstorm/SKILL.md"].contains("name: pp-brainstorm"));
        assert!(renamed["praxis/skills/pp-brainstorm/SKILL.md"].contains("pp-plan"));
        assert!(renamed["praxis/skills/pp-implement/SKILL.md"].contains("pp-review"));
        assert!(renamed["praxis/skills/agent-browser/SKILL.md"].contains("agent-browser"));
        assert!(renamed["praxis/agents/codebase-explorer.md"].contains("pp-brainstorm"));
        assert!(renamed["praxis/conventions.md"].contains("pp-plan → pp-implement"));
    }

    #[test]
    fn test_apply_skill_prefix_rename_noop_without_px_skills() {
        let templates = make_templates(&[
            ("praxis/conventions.md", "conventions"),
            (
                "praxis/skills/agent-browser/SKILL.md",
                "---\nname: agent-browser\n---",
            ),
        ]);
        let renamed = apply_skill_prefix_rename(templates.clone());
        assert_eq!(renamed, templates);
    }

    #[test]
    fn test_apply_skill_prefix_rename_leaves_bare_px_file_untouched() {
        let templates = make_templates(&[("praxis/skills/px-foo", "not a skill directory")]);
        let renamed = apply_skill_prefix_rename(templates);
        assert!(renamed.contains_key("praxis/skills/px-foo"));
        assert_eq!(renamed["praxis/skills/px-foo"], "not a skill directory");
    }

    #[test]
    fn test_renamed_skill_source() {
        assert_eq!(
            renamed_skill_source("praxis/skills/px-brainstorm/SKILL.md").as_deref(),
            Some("praxis/skills/pp-brainstorm/SKILL.md")
        );
        assert_eq!(
            renamed_skill_source("praxis/skills/px-plan/reference/template.md").as_deref(),
            Some("praxis/skills/pp-plan/reference/template.md")
        );
        // Non-skill / non-legacy / bare-dir shapes.
        assert_eq!(renamed_skill_source("praxis/conventions.md"), None);
        assert_eq!(
            renamed_skill_source("praxis/skills/agent-browser/SKILL.md"),
            None
        );
        assert_eq!(renamed_skill_source("praxis/skills/px-foo"), None);
        assert_eq!(renamed_skill_source("praxis/skills/pp-plan/SKILL.md"), None);
    }

    #[test]
    fn test_detect_skill_renames() {
        let templates = make_templates(&[
            ("praxis/skills/pp-brainstorm/SKILL.md", "content"),
            ("praxis/skills/pp-plan/SKILL.md", "content"),
            ("praxis/conventions.md", "content"),
        ]);
        let manifest_keys = [
            "praxis/conventions.md",
            "praxis/skills/px-plan/SKILL.md",
            "praxis/skills/px-review/SKILL.md", // pp- counterpart missing
        ];
        let renames = detect_skill_renames(manifest_keys, &templates);
        assert_eq!(
            renames,
            vec![(
                "praxis/skills/px-plan/SKILL.md".to_string(),
                "praxis/skills/pp-plan/SKILL.md".to_string()
            )]
        );
    }

    #[test]
    fn test_detect_skill_renames_sorted_and_empty() {
        let templates = make_templates(&[
            ("praxis/skills/pp-brainstorm/SKILL.md", "content"),
            ("praxis/skills/pp-plan/SKILL.md", "content"),
            ("praxis/skills/pp-review/SKILL.md", "content"),
        ]);
        let manifest_keys = [
            "praxis/skills/px-review/SKILL.md",
            "praxis/skills/px-plan/SKILL.md",
            "praxis/skills/px-brainstorm/SKILL.md",
        ];
        let renames = detect_skill_renames(manifest_keys, &templates);
        let olds: Vec<&str> = renames.iter().map(|(o, _)| o.as_str()).collect();
        assert_eq!(
            olds,
            vec![
                "praxis/skills/px-brainstorm/SKILL.md",
                "praxis/skills/px-plan/SKILL.md",
                "praxis/skills/px-review/SKILL.md"
            ]
        );
        assert!(detect_skill_renames([], &templates).is_empty());
    }
}
