# pan-pipe

AI-assisted development workflow installer for multiple coding agents.

A Rust rewrite of [praxis](https://github.com/DFilipeS/praxis) with extended support for **pi-coding-agent**, **Codex CLI**, and more.

## Supported Agents

| Agent | Skills | Agents/Reviewers | MCP Config |
|---|---|---|---|
| claude-code | `.claude/skills/` | `.claude/agents/` | `.mcp.json` |
| cursor | `.cursor/skills/` | `.cursor/` | `.cursor/mcp.json` |
| opencode | `.opencode/skills/` | `.opencode/agents/` | `opencode.json` |
| amp-code | `.agents/skills/` | `.agents/agents/` | native per-skill `mcp.json` |
| **pi-coding-agent** | `.pi/skills/` | `.pi/prompts/` | — |
| **codex** | `.codex/skills/` | `.codex/agents/` | `.codex/config.toml` |

## Installation

### Using mise (recommended)

[mise](https://mise.jdx.dev/) is a polyglot tool version manager. If you're already using mise, this is the easiest way to install and keep pan-pipe up to date.

```bash
# Install pan-pipe globally
mise use -g github:pan-pipe/pan-pipe@latest

# Update to the latest version
mise upgrade pan-pipe
```

**Supported platforms:**
- Linux x86_64 (including WSL Ubuntu, Fedora)
- Windows x86_64

### From crates.io (when published)

```bash
cargo install pan-pipe
```

### From source

```bash
git clone https://github.com/pan-pipe/pan-pipe
cd pan-pipe
cargo install --path .
```

### Pre-built binaries

Download from the [Releases](https://github.com/pan-pipe/pan-pipe/releases) page.

## Usage

### Initialize

```bash
pan-pipe init
```

Select the tools you want to install for and optional components.

### Update

```bash
pan-pipe update
```

Fetch the latest templates and apply changes.

### Status

```bash
pan-pipe status
```

Show the status of installed files (unchanged / modified / missing).

### Manage components

```bash
pan-pipe components
```

Add or remove optional skills and reviewers.

### Manage tools

```bash
# Add a tool
pan-pipe tool add cursor

# Remove a tool
pan-pipe tool remove cursor

# List tools
pan-pipe tool list
```

### Use a specific Git ref

```bash
pan-pipe init --ref v1.2.3
pan-pipe update --ref main
```

## Development

```bash
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt
```

## Migrating from JS praxis

pan-pipe uses the same `.praxis-manifest.json` format. Simply run:

```bash
pan-pipe update
```

in a project that was initialized with the JS version.

## License

MIT
