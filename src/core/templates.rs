#![allow(dead_code)]

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use std::collections::BTreeMap;
use std::io::Read;
use tar::Archive;

const TARBALL_BASE_URL: &str = "https://api.github.com/repos/DFilipeS/praxis/tarball";
const MAX_DOWNLOAD_SIZE: usize = 10 * 1024 * 1024; // 10 MB

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

    extract_templates(&bytes)
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
}
