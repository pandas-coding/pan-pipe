---
title: Distribute pan-pipe via mise using GitHub Releases
date: 2025-09-17
status: refined
tags: [distribution, mise, github-releases, cross-platform, github-actions]
---

# Distribute pan-pipe via mise using GitHub Releases

## Problem

Users need a convenient way to install and update pan-pipe across multiple personal computers. Currently, installation requires either:
- Compiling from source with `cargo install` (slow, requires Rust toolchain)
- Manually downloading binaries from GitHub Releases (tedious, no update mechanism)

The goal is to leverage mise (which users already use for managing Node.js and Rust) to provide a seamless installation and update experience.

## Core Idea

Distribute pan-pipe through mise's GitHub backend by publishing pre-compiled binaries to GitHub Releases. Users can then install and update pan-pipe globally using simple mise commands (`mise use -g` and `mise upgrade`), without needing project-level configuration files or local compilation.

## Key Insights

- **No compilation on install**: Users explicitly cannot compile Rust code during installation, so pre-compiled binaries are required
- **Cross-platform support needed**: Must provide binaries for multiple platforms (Linux, macOS, Windows in various architectures)
- **Global installation**: Users want a single global version, not per-project versions (no `.mise.toml` needed)
- **Fast updates**: After a new release is published, users should be able to update quickly via `mise upgrade`
- **mise as distribution mechanism**: mise is purely an installation/update tool here, not a development dependency manager
- **GitHub Releases as source**: Chosen over npm or crates.io to avoid extra layers and keep the distribution direct

## Decisions Made

- **Platforms**: Support `x86_64-unknown-linux-gnu` (covers WSL Ubuntu and Fedora) and `x86_64-pc-windows-msvc` (Windows)
- **Release automation**: Use GitHub Actions (free for open source, one-time setup)
- **Version strategy**: Semantic versioning with git tags (e.g., `v0.1.0`, `v0.2.0`)
- **Binary naming**: Follow Rust community convention: `pan-pipe-v{version}-{target}.{ext}` where ext is `tar.gz` for Linux and `zip` for Windows
- **Distribution method**: GitHub Releases + mise GitHub backend
- **User workflow**: `mise use -g github:pan-pipe/pan-pipe@latest` for install, `mise upgrade pan-pipe` for updates

## Implementation Requirements

- Create `.github/workflows/release.yml` for automated builds
- Trigger on git tags matching `v*`
- Build for both targets in parallel
- Package binaries appropriately (tar.gz for Linux, zip for Windows)
- Upload to GitHub Releases automatically
- Update README with mise installation instructions

## Technical Details Confirmed

**mise GitHub backend:**
- Native support in mise via `github:owner/repo` syntax
- Automatically detects versions from GitHub Releases API
- `mise ls-remote github:owner/repo` lists all available versions
- `mise upgrade` checks for and installs latest version

**Binary naming convention (verified with fd, ripgrep):**
- Linux/macOS: `{project}-v{version}-{target-triple}.tar.gz`
- Windows: `{project}-v{version}-{target-triple}.zip`
- Example: `pan-pipe-v0.1.0-x86_64-unknown-linux-gnu.tar.gz`

**GitHub Actions:**
- Free for public repositories (unlimited minutes)
- Triggers on git tag push
- Can build on multiple OS runners in parallel
- Automatic release creation and asset upload

## Alternative Directions Considered

**Direction B: npm + mise npm backend** (rejected)
- Would require maintaining npm package with postinstall script
- Adds unnecessary intermediate layer
- More complex than direct GitHub Releases

**Direction C: crates.io + cargo backend with cargo-binstall** (rejected)
- Requires users to have cargo-binstall installed
- Unclear mise support for cargo-binstall
- Still need to publish binaries to GitHub Releases anyway

**Manual release process** (rejected)
- Too time-consuming for each release (15-30 minutes)
- Error-prone
- Not sustainable for regular updates

## Related Documents

- (none yet)
