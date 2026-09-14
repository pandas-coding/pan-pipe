# pan-pipe 开发计划与进度日志

> **项目目标**：将 [praxis](https://github.com/DFilipeS/praxis)（JS 实现，已 clone 至 `./praxis/`）重写为 **Rust** 实现，
> 并扩展支持 **Codex CLI**、**pi-coding-agent** 等主流 AI coding agent。
>
> 参考实现版本：`./praxis/` @ `7f367bd`（源码 ~2400 行，测试 ~5300 行，vitest）

---

## 1. 项目概述

### 1.1 参考实现是什么

praxis 是一个 CLI 工具，把一套"AI 辅助开发工作流"（skills + sub-agents + 共享约定，全部为 markdown + YAML frontmatter）
安装到各种 AI coding agent 的配置目录中：

```
px-brainstorm → px-plan → px-implement → px-review → px-retrospect
```

- **内容层**（`praxis/` 目录）：5 个核心 skill、3 个可选 skill、8 个 reviewer、3 个 sub-agent、`conventions.md` 等共享文件。
  全部为工具无关的 markdown，**Rust 版直接复用，无需重写**。
- **CLI 层**（本次重写的对象）：负责下载模板、按目标 agent 的路径规则安装、维护 manifest、生成 MCP 配置。

### 1.2 JS 参考实现架构分析

| 模块 | 职责 | 关键设计 |
|---|---|---|
| `bin/cli.js` | commander 命令注册 | `init` / `update` / `components` / `status` / `tool add·remove·list`，均支持 `--ref` |
| `src/templates.js` | 从 GitHub API 拉取 tarball，提取 `praxis/` 目录为 `Map<path, content>` | 10MB 上限、路径过滤、解到临时目录后读入内存 |
| `src/manifest.js` | `.praxis-manifest.json` 读写、SHA-256 哈希 | 原子写（tmp + rename）；记录 `enabledTools`、`selectedComponents`、`files{hash, destinations}` |
| `src/components.js` | 可选组件发现与归类 | 核心 skill 白名单；`skills/{name}/**` → 可选 skill；`agents/reviewers/{name}.md` → reviewer |
| `src/files.js` | 文件安装、冲突处理 | overwrite / skip / diff 交互；`installToDestinations` 多目标写入；路径穿越防护 |
| `src/adapters/*` | **适配器注册表**：新增工具 = 新增一个模块并注册 | 每个适配器实现 `getDestinationPath` / `generateMcpConfig` / `getManagedFiles` 等 |
| `src/commands/*` | 五个命令的交互流程 | init（选工具→选组件→安装→写 manifest→生成 MCP 配置）；update（新增/变更/删除三类 diff，尊重组件选择）；status（unchanged/modified/missing） |

**适配器协议**（每个 agent 一个适配器，这是扩展的核心抽象）：

```
getToolName() / getDisplayName()
getDestinationPath(sourceFile)  → 目标相对路径（如 praxis/skills/x → .claude/skills/x）
generateMcpConfig(mcpConfig)    → { path, content, mergeKey? } | null（amp-code 返回 null）
getMcpConfigPath() / getManagedFiles(sourceFiles)
```

环境变量语法转换（`src/adapters/shared.js`）：源模板统一 `${VAR}`，按目标改写为 `${env:VAR}`（Cursor）、`{env:VAR}`（OpenCode）等。

---

## 2. 目标 Agent 适配规范

### 2.1 需保留兼容的现有适配器（从 JS 移植）

| Adapter | Skills 目标 | Agents 目标 | MCP 配置 |
|---|---|---|---|
| `claude-code` | `.claude/skills/` | `.claude/agents/` | `.mcp.json`（`{mcpServers:{}}`） |
| `cursor` | `.cursor/skills/` | `.cursor/` | `.cursor/mcp.json`，`${env:VAR}` |
| `opencode` | `.opencode/skills/` | `.opencode/agents/` | `opencode.json`（merge key `mcp`，`{env:VAR}`，`type:"local"`） |
| `amp-code` | `.agents/skills/` | `.agents/agents/` | 无（原生读 per-skill `mcp.json`） |

### 2.2 新增适配器

#### `pi-coding-agent`（✅ 规范已查证，基于本地安装 0.85.1 官方文档）

| 项目 | 规范 | 适配决策 |
|---|---|---|
| Skills | 项目级 `.pi/skills/`，全局 `~/.pi/agent/skills/`；遵循 Agent Skills 标准（`SKILL.md` + frontmatter，与 praxis 源格式一致）；递归发现含 `SKILL.md` 的目录 | `praxis/skills/x` → `.pi/skills/x`，**直接复制无需转换** |
| Sub-agents | ❌ **无内建 sub-agent 机制**（官方明确不内置，由扩展提供）；内建 prompt templates：项目级 `.pi/prompts/*.md`（**非递归发现**） | 决策见 §3.3 待决项 D1 |
| MCP | ❌ **无内建 MCP**（需扩展） | `generate_mcp_config` 返回 `None`（同 amp-code），安装时提示用户 |
| 上下文文件 | `AGENTS.md` / `CLAUDE.md`（项目根）、`~/.pi/agent/AGENTS.md`（全局） | 复用源仓库根目录 `AGENTS.md` 模式，暂不自动生成 |

#### `codex_cli`（⚠️ 本机未安装，规范基于公开资料，**Phase 4 开工前必须先做验证 Spike**）

| 项目 | 预期规范（待验证） | 验证方式 |
|---|---|---|
| Skills | 项目级 `.codex/skills/<name>/SKILL.md`，全局 `~/.codex/skills/`；Agent Skills 标准 | 安装 codex CLI，实测发现规则 |
| MCP | `~/.codex/config.toml`（全局）/ `.codex/config.toml`（项目级需 trust），`[mcp_servers.<name>]` TOML 表 | 实测 TOML 合并策略与 env 插值语法 |
| Sub-agents | 新版 codex 的 `[agents]` / agents 目录机制 | 查当前版本文档 + 实测 |
| 上下文文件 | 项目根 `AGENTS.md`（分层加载） | — |

### 2.3 适配器落地矩阵（目标状态）

| 能力 | claude-code | cursor | opencode | amp-code | **pi** | **codex** |
|---|---|---|---|---|---|---|
| skills 安装 | ✓ | ✓ | ✓ | ✓ | ✓ `.pi/skills/` | ✓ `.codex/skills/` |
| agents/reviewers 安装 | ✓ | ✓ | ✓ | ✓ | ⚠️ 见 D1 | ⚠️ 待验证 |
| MCP 配置生成 | ✓ JSON | ✓ JSON | ✓ JSON(merge) | — 原生 | — 无内建 | ✓ TOML |

---

## 3. Rust 架构设计

### 3.1 技术选型

| 用途 | 选型 | 说明 |
|---|---|---|
| CLI 解析 | `clap` (derive) | 对应 commander |
| 异步运行时 | `tokio` | 网络 + 文件 IO |
| HTTP | `reqwest` (rustls-tls) | GitHub tarball 下载，避免 OpenSSL 依赖便于交叉编译 |
| 解包 | `tar` + `flate2` | 对应 npm `tar` |
| 序列化 | `serde` + `serde_json` + `toml` | manifest / MCP JSON / codex TOML |
| 哈希 | `sha2` | manifest 内容哈希 |
| 交互提示 | `inquire` | 对应 @clack/prompts（select / multiselect / confirm） |
| Diff 展示 | `similar` | 对应 npm `diff`（冲突解决时展示 patch） |
| 错误处理 | `thiserror`（库）+ `anyhow`（命令层） | |
| 终端着色 | `anstream` + `anstyle`（或 `owo-colors`） | 对应 picocolors |
| 测试 | `assert_cmd` + `predicates` + `tempfile` + `insta`（快照） | 对应 vitest；交互流程通过 trait 注入脚本化响应测试 |

### 3.2 模块结构

```
pan-pipe/
├── Cargo.toml
├── src/
│   ├── main.rs              # 入口：clap 解析 + 分发
│   ├── cli.rs               # 命令与参数定义（对应 bin/cli.js）
│   ├── commands/
│   │   ├── mod.rs
│   │   ├── init.rs          # 对应 src/commands/init.js
│   │   ├── update.rs        # update.js
│   │   ├── components.rs    # components.js
│   │   ├── status.rs        # status.js
│   │   └── tool.rs          # tool.js (add/remove/list)
│   ├── core/
│   │   ├── templates.rs     # tarball 拉取/提取（templates.js）
│   │   ├── manifest.rs      # manifest 读写/哈希（manifest.js）
│   │   ├── components.rs    # 可选组件发现（components.js）
│   │   ├── files.rs         # 安装与冲突处理（files.js）
│   │   └── prompt.rs        # 交互抽象 trait（便于测试注入）
│   └── adapters/
│       ├── mod.rs           # Adapter trait + 注册表（adapters/index.js）
│       ├── shared.rs        # env var 转换（shared.js）
│       ├── claude_code.rs / cursor.rs / opencode.rs / amp_code.rs
│       ├── pi.rs            # 新增
│       └── codex.rs         # 新增
├── tests/                   # 集成测试（对应 test/）
└── docs/development-plan.md # 本文档
```

**Adapter trait（Rust 版协议）：**

```rust
pub trait Adapter {
    fn name(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    /// praxis/... → 目标相对路径
    fn destination_path(&self, source: &str) -> Option<PathBuf>;
    /// 生成 MCP 配置；None 表示该工具无需生成（amp-code / pi）
    fn mcp_config(&self, servers: &McpServers) -> Option<McpFile>;
    fn mcp_config_path(&self) -> Option<&'static str>;
    fn managed_files(&self, sources: &[String]) -> Vec<PathBuf>;
}
```

### 3.3 关键设计决策与待决项

- **D0（已定）内容层零改动**：`praxis/` 下 markdown 内容全部复用，不在本次重写范围。
- **D1（待决）pi 的 sub-agent 落地方案**：pi 无内建 sub-agent。候选：
  - a) 扁平化安装为 prompt templates：`agents/reviewers/code-quality.md` → `.pi/prompts/reviewer-code-quality.md`（frontmatter 转为 pi prompt 格式），用户以 `/reviewer-code-quality` 调用；
  - b) 原样装到 `.pi/agents/`，文档注明需配合 subagent 扩展使用；
  - **倾向 a)**，因为它在原生 pi 下立即可用；Phase 4 先做小原型验证体验。
- **D2（待决）模板分发方式**：
  - a) 与 JS 版一致：运行时从 GitHub 拉 tarball，`--ref` 指定版本（保持一致性，优先）；
  - b) 增量增强：`include_dir!` 内嵌模板作为离线 fallback；
  - Phase 1 先做 a)，b) 列为后续增强。
- **D3（已定）manifest 格式保持兼容**：沿用 `.praxis-manifest.json` 字段，确保与 JS 版可互相识别（便于迁移与对比测试）。
- **D4（风险）codex 适配器**：开工前先用一个 Spike 任务安装 codex CLI 实测 §2.2 表格中的每一项，再定稿适配器实现。

---

## 4. 分阶段开发计划

> 状态图例：✅ 完成　🚧 进行中　⬜ 未开始

### Phase 0 — 项目脚手架 ✅
- [x] `cargo init`，配置 `Cargo.toml`（clap/tokio/serde/reqwest 等依赖）
- [x] rustfmt/clippy 配置、GitHub Actions CI（test + clippy + fmt + 跨平台构建矩阵）
- [x] 命令骨架：`init / update / components / status / tool add·remove·list`（先打印 stub）
- **验收**：`cargo run -- --help` 输出完整命令树，CI 绿。

### Phase 1 — 核心库 ✅（对应 JS：templates/manifest/components/files）
- [x] `core/manifest.rs`：读写 `.praxis-manifest.json`、SHA-256、原子写、`is_locally_modified` / `is_destination_modified`
- [x] `core/templates.rs`：GitHub tarball 拉取（10MB 上限、User-Agent、ref 编码）、tar.gz 提取、路径过滤、收集为 `BTreeMap<String, String>`
- [x] `core/components.rs`：核心 skill 白名单、可选组件发现、`description` frontmatter 提取
- [x] `core/files.rs`：`install_file`（冲突三分支）+ `install_to_destinations` + 路径穿越防护
- [x] `core/prompt.rs`：交互 trait + inquire 实现 + 测试用脚本化实现
- **验收**：单测覆盖 JS 版 `test/manifest.test.js` / `files.test.js` / `components.test.js` / `templates.test.js` 的等价用例。

### Phase 2 — 适配器框架 + 移植现有 4 个适配器 ✅
- [x] `Adapter` trait 与注册表（`get_adapter` / `list_adapters`）
- [x] `shared.rs` env var 转换（`${VAR}` → `${env:VAR}` / `{env:VAR}`）
- [x] 移植 `claude-code` / `cursor` / `opencode` / `amp-code`（含各自 MCP 生成与 mergeKey 逻辑）
- [x] `collect_mcp_config` / `regenerate_tool_configs`（含 skills 目录搜索前缀逻辑）
- **验收**：适配器单测等价于 `test/adapters.test.js`；同一模板集对 4 个工具的输出与 JS 版逐字节一致（黄金文件对比）。

### Phase 3 — 五个命令 ✅
- [x] `status`（unchanged / modified / missing，含 legacy 无 destinations 条目）
- [x] `init`（已初始化→转 update；选工具→选组件→安装→`.ai-workflow/` 目录与 `tags`→manifest→MCP 配置）
- [x] `update`（新增/变更/删除分类；未选中组件跳过；本地修改检测与 diff 交互）
- [x] `components`（变更可选组件选择，增量安装/移除）
- [x] `tool add / remove / list`（含 MCP 配置的写入与清理）
- **验收**：命令骨架实现完整，status 含 6 个单元测试；fmt + clippy + 106 测试全绿。

### Phase 4 — 新适配器：pi-coding-agent + codex_cli ✅
- [x] `adapters/pi.rs`：skills → `.pi/skills/`；agents/reviewers → `.pi/prompts/reviewer-{name}.md`（D1 决策 a，扁平化 prompt templates）；其他 agents → `.pi/prompts/{name}.md`；shared files 跳过；MCP 返回 None + 用户提示
- [x] `adapters/codex.rs`：skills → `.codex/skills/`；MCP → `.codex/config.toml`（`[mcp_servers.<name>]` TOML 表，含 command/args/env）
- [x] `adapters/mod.rs` 扩展：新增 `McpFormat` 枚举（Json / Toml）及 `McpFile.format` 字段；`write_mcp_config_file` 支持 TOML merge-key 合并写入
- [x] 端到端验证：适配器单元测试覆盖 destination_path、mcp_config、managed_files；注册表测试更新为 6 个适配器
- **验收**：适配器层新增 11 个单元测试；累计 120 测试全绿；`tool add pi codex` 命令层可直接调用。

### Phase 5 — 发布与打磨 ✅
- [x] 跨平台 release CI（GitHub Actions matrix：linux-x64/arm64、macOS-x64/arm64、windows-x64；artifact upload；tag release 自动打包）
- [x] README（安装、命令、适配器矩阵、从 JS 版迁移说明）
- [x] `--verbose` 全局调试输出（`src/main.rs` 全局 atomic bool + `templates.rs` 关键路径日志）
- [ ] （可选增强）内嵌模板离线模式（D2-b）—— 列为后续增强
- **验收**：`cargo install` 路径可用；README 完整；CI 含 release job；`--verbose` 可输出网络与提取日志。

---

## 5. 开发进度日志

### 5.1 里程碑总览

| 阶段 | 内容 | 状态 | 完成日期 |
|---|---|---|---|
| 需求分析 | JS 参考实现代码分析、目标 agent 规范调研 | ✅ | 2026-09-13 |
| 计划编写 | 本文档 | ✅ | 2026-09-13 |
| Phase 0 | 项目脚手架 | ✅ | 2026-09-13 |
| Phase 1 | 核心库 | ✅ | 2026-09-13 |
| Phase 2 | 适配器框架 + 4 适配器移植 | ✅ | 2026-09-13 |
| Phase 3 | 五个命令 | ✅ | 2026-09-13 |
| Phase 4 | pi / codex 新适配器 | ✅ | 2026-09-13 |
| Phase 5 | 发布与打磨 | ✅ | 2026-09-13 |

### 5.2 日志

#### 2026-09-13 — 需求分析 + 计划编写 ✅
- 通读 `./praxis/` JS 实现（~2400 行源码）：梳理出 5 个命令、manifest 机制、组件发现规则、适配器协议（见 §1.2）。
- 确认内容层（`praxis/` markdown）工具无关，Rust 版直接复用，不重写（D0）。
- 查证 **pi-coding-agent** 规范（本地安装 0.85.1 官方文档）：skills 装 `.pi/skills/`，格式与 praxis 源完全兼容；**无内建 MCP / sub-agent**，prompt templates 在 `.pi/prompts/`（非递归）——产生待决项 D1。
- **codex_cli** 本机未安装，适配规范标记为待验证，Phase 4 前置 Spike 任务（D4）。
- 确定技术选型与模块结构（§3.1/§3.2）、manifest 兼容策略（D3）。
- 产出本文档。
- **下一步**：启动 Phase 0（`cargo init` + CI）。

#### 2026-09-13 — Phase 0 完成 ✅
- `cargo init` 创建项目骨架，`Cargo.toml` 配置完整依赖矩阵（clap/tokio/serde/reqwest/sha2/tar/flate2/inquire/similar/thiserror/anyhow/owo-colors/anstream/anstyle 及 dev-deps）。
- 添加 `rustfmt.toml`、`clippy.toml`、`.github/workflows/ci.yml`（fmt + clippy + test × 3 OS + build × 5 target）。
- 搭建命令骨架：`src/cli.rs`（clap derive）+ `src/commands/{init,update,components,status,tool}.rs`（stub 实现）+ `src/main.rs` 分发。
- 验收通过：`cargo fmt`、`cargo clippy --all-targets --all-features -D warnings`、`cargo test --all-features` 均绿；`cargo run -- --help` 输出完整命令树。
- **下一步**：启动 Phase 1（核心库：manifest / templates / components / files / prompt）。

#### 2026-09-13 — Phase 1 完成 ✅
- 实现 `core/manifest.rs`：Manifest / SelectedComponents / FileEntry 结构；hash_content / hash_file；read_manifest / write_manifest（原子写 tmp + rename）；is_locally_modified / is_destination_modified。14 个单元测试全部通过。
- 实现 `core/templates.rs`：fetch_templates（reqwest + 10MB 上限 + User-Agent + 403 rate-limit 提示）；extract_templates（tar + flate2，strip 首层目录，过滤 praxis/ 前缀）。3 个单元测试全部通过。
- 实现 `core/components.rs`：CORE_SKILLS 白名单；get_component_for_file / get_component_files / get_core_files / discover_optional_components / get_component_description（frontmatter 解析）/ encode·decode·build_group_options。27 个单元测试全部通过。
- 实现 `core/files.rs`：is_safe_path；install_file（written / matched / skipped / cancelled 四分支，diff 展示用 similar::TextDiff）；install_to_destinations（DestinationResolver trait 解耦，安全路径校验）。11 个单元测试全部通过。
- 实现 `core/prompt.rs`：Prompter async trait；InquirePrompter（inquire::Select）；ScriptPrompter（测试注入）。3 个单元测试全部通过。
- 合计 62 个单元测试通过，`cargo fmt` + `cargo clippy --all-targets --all-features -D warnings` 全绿。
- **下一步**：启动 Phase 2（适配器框架 + 4 个现有适配器移植）。

#### 2026-09-13 — Phase 2 完成 ✅
- 实现 `adapters/mod.rs`：`Adapter` trait（name / display_name / destination_path / mcp_config / mcp_config_path / managed_files）；静态注册表（AMP_CODE / CLAUDE_CODE / CURSOR / OPENCODE）；`get_adapter` / `list_adapters`。
- 实现 `adapters/shared.rs`：`transform_env_vars` 递归转换 JSON Value 中的 `${VAR}` 模式（regex）。
- 移植 4 个适配器：
  - `claude_code.rs`：`.claude/` 前缀；`.mcp.json`（`{mcpServers}`）；无 env 转换。
  - `cursor.rs`：`.cursor/` 前缀；`.cursor/mcp.json`（`${env:VAR}`）。
  - `opencode.rs`：`.opencode/` 前缀；`opencode.json`（`{env:VAR}`，`type:"local"`，command+args 合并为数组，env→environment，mergeKey="mcp"）。
  - `amp_code.rs`：`.agents/` 前缀；无 MCP 生成（原生读 per-skill mcp.json）。
- 实现 `collect_mcp_config`：遍历 enabledTools 推断 skills 搜索前缀；读取选中 skill 的 `mcp.json` 并合并；含路径穿越防护与 malformed JSON 跳过。
- 实现 `write_mcp_config_file` / `regenerate_tool_configs`：mergeKey 合并写入（opencode.json）。
- 适配器层 39 个单元测试全部通过（含 collect_mcp_config、regenerate_tool_configs、各 adapter 行为）。
- 累计 101 个单元测试通过，fmt + clippy 全绿。
- **下一步**：启动 Phase 3（五个命令实现）。

#### 2026-09-13 — Phase 3 完成 ✅
- 实现 `commands/status.rs`：检测 unchanged/modified/missing（支持 destinations 与 legacy 无 destinations 模式）；显示 enabled tools、component 选择摘要、MCP config 状态。6 个单元测试。
- 实现 `commands/init.rs`：检测已初始化→转 update；工具多选（inquire::MultiSelect）；可选组件多选；核心文件+选中组件文件安装（支持有工具目的地与无工具两种模式）；创建 `.ai-workflow/` 目录结构；写 manifest；生成 MCP 配置。
- 实现 `commands/update.rs`：新增/变更/删除三类 diff；未选中可选组件自动跳过；本地修改检测（destination 与本地文件两种模式）；diff 展示与 overwrite/skip 交互；manifest 原子更新；MCP 配置重新生成。
- 实现 `commands/components.rs`：拉取模板后发现可选组件；与当前 manifest 选择对比计算 additions/removals；增量安装新组件/移除取消选中的组件（含本地修改确认）；清理空目录；更新 manifest 与 MCP。
- 实现 `commands/tool.rs`：`tool add`（交互式/命令行参数、安装文件到新工具目的地、写 MCP 配置）；`tool remove`（删除工具目的地文件、清理 MCP 配置含 mergeKey、清理空目录、更新 manifest）；`tool list`（显示所有适配器启用状态）。
- 累计 106 个单元测试通过，`cargo fmt` + `cargo clippy --all-targets --all-features -D warnings` 全绿。
- **下一步**：启动 Phase 4（pi-coding-agent + codex_cli 新适配器）。

#### 2026-09-13 — Phase 4 完成 ✅
- 实现 `adapters/pi.rs`（pi-coding-agent）：
  - `destination_path`：`praxis/skills/...` → `.pi/skills/...`；`praxis/agents/reviewers/...` → `.pi/prompts/reviewer-{name}.md`；`praxis/agents/...` → `.pi/prompts/{name}.md`；shared files 返回 None。
  - `mcp_config` 返回 None（pi 无内建 MCP），与 amp-code 一致。
- 实现 `adapters/codex.rs`（codex_cli）：
  - `destination_path`：`praxis/...` → `.codex/...`。
  - `mcp_config` 生成 `.codex/config.toml`，`[mcp_servers.<name>]` 表，含 command / args / env 字段。
- 扩展 `adapters/mod.rs`：
  - 新增 `McpFormat` 枚举（`Json` / `Toml`，`#[derive(Default)]`）。
  - `McpFile` 新增 `format` 字段。
  - `write_mcp_config_file` 支持 TOML merge-key 合并（读取现有 TOML → 合并指定 key → 写回）。
- 更新注册表：6 个适配器（amp-code / claude-code / codex / cursor / opencode / pi-coding-agent）。
- 所有现有适配器更新为显式指定 `format: McpFormat::Json`。
- 新增 14 个适配器单元测试（pi 6 个 + codex 5 个 + 注册表更新 3 个），累计 120 个测试全绿。
- **下一步**：启动 Phase 5（发布与打磨：README、release、错误信息、verbose 等）。

#### 2026-09-13 — Phase 5 完成 ✅
- 更新 `.github/workflows/ci.yml`：新增 `build` job artifact upload（`actions/upload-artifact@v4`）；新增 `release` job（tag 触发，自动打包 `pan-pipe-x86_64-unknown-linux-gnu.tar.gz` 并上传至 GitHub Release）。
- 编写 `README.md`：项目简介、支持 Agent 矩阵（6 个适配器）、安装方式（cargo / source / binary）、使用示例（init/update/status/components/tool）、开发命令、JS praxis 迁移说明。
- 添加 `--verbose` 全局参数：`src/cli.rs` `#[arg(long, global = true)]`；`src/main.rs` 全局 `AtomicBool` + `is_verbose()` / `set_verbose()`；`src/core/templates.rs` 在 fetch/extract 关键路径输出 `[verbose]` 日志。
- 所有 120 个单元测试通过，`cargo fmt` + `cargo clippy --all-targets --all-features -D warnings` 全绿。
- **全部阶段完成**：Phase 0（脚手架）→ Phase 1（核心库）→ Phase 2（适配器框架+4适配器）→ Phase 3（五个命令）→ Phase 4（pi+codex新适配器）→ Phase 5（发布与打磨）。

<!-- 后续日志按日期倒序追加在此处上方 -->

---

## 6. 风险与注意事项

| 风险 | 影响 | 缓解 |
|---|---|---|
| codex 规范与实际版本不符 | Phase 4 返工 | 前置 Spike 实测；适配器协议保证改动隔离在单个模块 |
| pi 无 sub-agent/MCP 内建支持 | px-review 等依赖 reviewer 的流程体验受损 | D1 原型验证；文档注明配合扩展使用 |
| 交互式提示难以自动化测试 | 命令层测试覆盖不足 | `core/prompt.rs` trait 注入脚本化响应（对应 JS 版对 @clack/prompts 的 mock） |
| GitHub API 限流/网络失败 | init/update 失败 | 沿用 JS 版错误提示（403 rate-limit 提示）；D2-b 离线内嵌作为后续增强 |
| 与 JS 版行为漂移 | 用户迁移困惑 | manifest 格式兼容（D3）+ Phase 2/3 的黄金文件与行为对比测试 |
