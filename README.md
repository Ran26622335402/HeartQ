<div align="center">

<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://media.x.ai/v1/website/spacexai-symbol-white-transparent-0c31957f.png">
    <source media="(prefers-color-scheme: light)" srcset="https://media.x.ai/v1/website/spacexai-symbol-black-transparent-6435cf42.png">
    <img alt="HeartQ logo" src="https://media.x.ai/v1/website/spacexai-symbol-black-transparent-6435cf42.png" width="96">
  </picture>
  <br>
  HeartQ Build（<code>heartq</code>）
</h1>

**HeartQ Build（红桃 Q）** 是终端里的 AI 编程智能体（由 HyperAI 打造）。它以全屏 TUI 运行，能理解代码库、编辑文件、执行 shell、搜索网页并管理长任务；支持交互使用、无头模式（脚本 / CI），以及通过 Agent Client Protocol（ACP）嵌入编辑器。

[功能点总览](#功能点总览) ·
[安装预编译包](#安装预编译包) ·
[编译与安装](#编译与安装) ·
[文档](#文档) ·
[仓库结构](#仓库结构) ·
[日常开发](#日常开发) ·
[贡献](#贡献) ·
[许可证](#许可证)

![HeartQ Build TUI](https://media.x.ai/v1/website/universe-tui-screenshot-6f7a0837.png)

本仓库包含 `heartq` CLI / TUI 与智能体运行时的 Rust 源码。根目录 `SOURCE_REV` 记录对应上游提交 SHA。

</div>

---

## 功能点总览

> **核对说明（2026-08-04）**：下列能力均已在源码中落实（非文档愿望）。完整 F01–F120 见 [`docs/patents/HeartQ-源码功能点报告.md`](docs/patents/HeartQ-源码功能点报告.md)；算法默认值见 [`docs/patents/HeartQ-算法逻辑与公式手册.md`](docs/patents/HeartQ-算法逻辑与公式手册.md)。

### 架构分层

```
A 会话运行时 → B 子智能体/Goal → C 元技能 → D 模型路由 → E 记忆/Dream
→ F 压缩安全 → G 权限/Hook → H 工具 → I UX → J 其它
```

| 层 | 代表能力 | 主 crate |
|----|----------|----------|
| A 会话 | ACP SessionActor、插话、回绕、Worktree、Doom Loop、Stop Gate | `heartq-shell`、`xai-interjection-core` |
| B 协同 | Task 子代理（深度≤1）、Goal DE-V、Rhai 工作流 Journal | `heartq-shell`、`heartq-tools`、`xai-workflow` |
| C 技能 | MetaSkill DAG、触发评分、Skills Guard、Curator | `heartq-memory`、`heartq-tools`、`heartq-agent` |
| D 路由 | 启发式 c0–c3 + Squilla（**默认关闭**） | `heartq-model-router` |
| E 记忆 | 分层 Markdown、混合检索、Prefetch、Dream 巩固 | `heartq-memory` |
| F 压缩 | 全量/轮内/轮间压缩、义务门控、工具剪枝、SafetyMode | `heartq-compaction` |
| G 治理 | 权限引擎、Hook、Plan 模式、OS 沙箱 | `heartq-sandbox`、`heartq-hooks` |
| H/I | 文件/Shell/Web/MCP、70+ 斜杠命令、TUI | `heartq-tools`、`heartq-pager` |

### 核心创新（已落实）

| 创新 | 状态 | 要点 | 源码锚点 |
|------|------|------|----------|
| **记忆系统** | 已落实 | Global/Workspace/Session 分层；FTS+向量混合检索（\(w_v=0.7\)）；时间衰减半衰期 7 天；每轮 Prefetch；压缩前 Flush | `heartq-memory`：`storage` / `search` / `prefetch` |
| **Dream 巩固** | 已落实 | 门控 min 4h / 3 sessions；增强晋升排序（权重 0.35/0.30/0.20/0.15） | `dream.rs`、`dream_enhanced/` |
| **MetaSkill 编排** | 已落实 | DAG `depends_on`、默认可并行 4；触发词边界分 1.0 / 子串 0.5；可暂停澄清与崩溃恢复 | `meta_skill/runner.rs`、`trigger.rs`、`store.rs` |
| **Goal DE-V** | 已落实 | Design–Execute–Verify；默认 3 路 skeptic；冷面板多数决 \(K=\lfloor N/2\rfloor+1\) | `goal_tracker.rs`、`goal_classifier.rs` |
| **多子智能体** | 已落实 | Task spawn；**最大嵌套深度 1**；可配 Worktree 隔离（快照默认关） | `task/mod.rs`、`xai-fast-worktree` |
| **压缩与安全** | 已落实 | 13 类义务提取与覆盖验证；工具结果剪枝；SafetyMode Protect/BestEffort/Block/Off | `heartq-compaction` |
| **脚本工作流** | 已落实 | Rhai + JSONL Journal；每会话最多 4 个活跃 run | `xai-workflow` |
| **模型路由** | 已落实（默认关） | 启发式分档 + 可选 ML sidecar；`enabled=false`，`Observe` 灰度 | `heartq-model-router` |
| **插话 / 回绕 / Doom** | 已落实 | 轮内插话；`/rewind`；尾部死循环恢复（Doom **默认关**） | `xai-interjection-core`、`rewind.rs`、`signals.rs` |
| **技能治理** | 部分 | Skills Guard Critical 阻断已落实；Curator 归档默认关；惰性检测已实现 | `skills_guard.rs`、`curator/`、`laziness_classifier.rs` |

### 关键默认值（源码）

| 常量 | 默认 |
|------|------|
| `MAX_SUBAGENT_DEPTH` | **1** |
| 元技能最大并行 | **4** |
| `goal_verifier_count` | **3**（钳位 1–5） |
| 混合检索 | \(w_v=0.7,\ w_t=0.3\)；`min_score=0.35` |
| Dream 门控 | 4 小时 / 3 会话 |
| 模型路由 | `enabled=false` |
| 工作流活跃 run | ≤ **4** / 会话 |

### 功能点分层摘要（F01–F120）

| 层 | ID 范围 | 示例功能 |
|----|---------|----------|
| A 会话运行时 | F01–F20 | SessionActor、插话、Rewind、Worktree、自动压缩、Doom Loop、MCP |
| B 子智能体 / Goal | F21–F35 | Task spawn、Goal DE-V、skeptic 面板、Rhai Journal、`/loop` |
| C 元技能 / 技能 | F36–F47 | MetaSkill DAG/触发/澄清、Skills Guard、自动提案、插件市场 |
| D 模型路由 | F48–F52 | 启发式分档、Squilla、Observe/Full、反降级地板 |
| E 记忆 / Dream | F53–F67 | 分层记忆、混合检索、MMR（默认关）、Prefetch、Dream、`/remember` |
| F 压缩安全 | F68–F80 | 三模式压缩、义务 13 类、剪枝、SafetyMode、Continuation |
| G 权限 / Hook | F81–F91 | 权限引擎、Stop Gate、Plan 模式、沙箱与子进程网络策略 |
| H 工具 | F92–F102 | Bash/文件/Web/LSP/Memory 工具、Computer Hub |
| I UX | F103–F114 | 70+ 斜杠命令、主题、语音、极简模式等 |
| J 其它 | F115–F120 | 配置热更、遥测开关、更新通道等（详见报告） |

逐条锚点与专利映射请以功能点报告为准，勿以 README 摘要替代交底真源。

## 安装预编译包

官方预编译包支持 macOS、Linux、Windows：

```sh
curl -fsSL https://x.ai/cli/install.sh | bash   # macOS / Linux / Git Bash
irm https://x.ai/cli/install.ps1 | iex          # Windows PowerShell
heartq --version
```

变更说明见 [changelog](https://x.ai/build/changelog)。

若使用本仓库自行编译的产物，请跳过本节，直接看 [编译与安装](#编译与安装)。

## 编译与安装

在本仓库源码上本地编译、安装与离线打包的推荐步骤。

### 1. 环境依赖

| 依赖 | 说明 |
|------|------|
| Rust / rustup | 版本由根目录 [`rust-toolchain.toml`](rust-toolchain.toml) 锁定（当前为 `1.92.0`）；首次构建时 rustup 会自动安装 |
| [DotSlash](https://dotslash-cli.com) | `cargo install dotslash`，并保证 `dotslash` 在 `PATH` 中（构建前必须可用） |
| protoc | 优先使用仓库 [`bin/protoc`](bin/protoc)（经 DotSlash）；也可使用 `PATH` 上的 `protoc` 或环境变量 `$PROTOC` |
| ripgrep（`rg`） | Release 构建可能尝试从网络下载捆绑的 `rg`；离线环境请用本机 `rg`（见下方环境变量） |

```sh
cargo install dotslash
dotslash --help   # 确认可用
```

**平台说明**

- **macOS / Linux**：正式支持的构建主机。
- **Windows**：源码构建为 best-effort，本树未做常规验证；若尝试，请安装 Visual Studio Build Tools（MSVC）。

### 2. 编译

在仓库根目录执行：

```sh
cd /path/to/HeartQ

# 离线 / 无法访问 GitHub 时建议先设置（指向本机 rg）
export HEARTQ_TOOLS_BUNDLE_RG_PATH="$(command -v rg)"
export HEARTQ_SHELL_BUNDLE_RG_PATH="$(command -v rg)"

# Debug（迭代更快）
cargo build -p heartq-pager-bin
# 或直接编译并启动 TUI
cargo run -p heartq-pager-bin

# Release（部署用）
cargo build -p heartq-pager-bin --release

# 快速类型检查（不产出完整二进制）
cargo check -p heartq-pager-bin
```

产物路径：

| 模式 | 路径 |
|------|------|
| Debug | `target/debug/heartq-pager` |
| Release | `target/release/heartq-pager` |

验证：

```sh
./target/release/heartq-pager --version
```

编译产物名为 `heartq-pager`；安装时通常复制 / 重命名为 `heartq`。

若修改了 `crates/codegen/heartq-agent/templates/prompt.md` 等系统提示模板，需先重新加密再编译：

```sh
cd crates/codegen/heartq-agent
python3 scripts/encrypt_templates.py
cd -
cargo build -p heartq-pager-bin --release
```

### 3. 安装到本机

```sh
mkdir -p ~/.heartq/bin ~/.heartq
cp -a target/release/heartq-pager ~/.heartq/bin/heartq
chmod 755 ~/.heartq/bin/heartq

# 可选：示例配置
cp -n share/examples/config.toml.example ~/.heartq/config.toml 2>/dev/null \
  || cp -n share/examples/config.toml ~/.heartq/config.toml 2>/dev/null \
  || true

# 可选：加入 PATH（bash / zsh）
# echo 'export PATH="$HOME/.heartq/bin:$PATH"' >> ~/.bashrc
export PATH="$HOME/.heartq/bin:$PATH"
heartq --version
```

覆盖**正在运行**的二进制时，若出现 `Text file busy`，可先写入临时文件再原子替换：

```sh
cp -a target/release/heartq-pager ~/.heartq/bin/heartq.new
mv -f ~/.heartq/bin/heartq.new ~/.heartq/bin/heartq
```

首次启动可能打开浏览器进行认证，参见 [认证说明](crates/codegen/heartq-pager/docs/user-guide/02-authentication.md)。

### 4. 打包（同架构 Linux 离线分发）

若当前机器为 **linux-arm64（aarch64）**，可用仓库脚本打 tar 包，拷到同架构机器解压即可：

```sh
./scripts/pack-linux-arm64.sh target/release/heartq-pager
# 输出：dist/heartq-build-<version>-linux-arm64.tar.gz
```

目标机：

```sh
tar -xzf heartq-build-*-linux-arm64.tar.gz
cd heartq-build-*-linux-arm64
mkdir -p ~/.heartq
cp share/examples/config.toml ~/.heartq/config.toml   # 按需修改 base_url / model
./bin/heartq
```

**注意：** 二进制与 CPU / OS 绑定。在 aarch64 Linux 上编出的包 **不能** 直接在 Windows 或 Linux x86_64 上运行；那些平台需在对应环境重新编译，或使用该平台的预编译包。

### 5. 清理编译缓存（保留产物）

```sh
# 仅保留可执行文件示例（按需调整）
cp -a target/release/heartq-pager /tmp/heartq-pager.keep
rm -rf target/release/{incremental,deps,build,.fingerprint} target/debug
cp -a /tmp/heartq-pager.keep target/release/heartq-pager

# 或直接：
# cargo clean   # 会删除整个 target，含产物
```

## 文档

| 文档 | 说明 |
|------|------|
| [`docs/README.md`](docs/README.md) | 仓库内文档索引 |
| [`docs/patents/HeartQ-源码功能点报告.md`](docs/patents/HeartQ-源码功能点报告.md) | F01–F120 功能点真源 |
| [`crates/codegen/heartq-pager/docs/user-guide/`](crates/codegen/heartq-pager/docs/user-guide/) | 产品用户指南（配置、斜杠命令、MCP、技能等） |
| [docs.x.ai/build/overview](https://docs.x.ai/build/overview) | 在线文档 |

## 仓库结构

| 路径 | 内容 |
|------|------|
| `crates/codegen/heartq-pager-bin` | 组合根包，产出 `heartq-pager` 二进制 |
| `crates/codegen/heartq-pager` | TUI：回滚区、提示符、模态框、渲染；`npm/heartq*` 发布包 |
| `crates/codegen/heartq-shell` | 智能体运行时（SessionActor、Goal、压缩触发等） |
| `crates/codegen/heartq-memory` | 记忆、Dream、MetaSkill、Prefetch |
| `crates/codegen/heartq-tools` | Task / Workflow / Skills Guard 等工具 |
| `crates/codegen/heartq-model-router` | 多模型路由（默认关） |
| `crates/common/heartq-compaction` | 压缩、义务、剪枝、SafetyMode |
| `crates/codegen/xai-workflow` | Rhai 工作流 + Journal |
| `crates/codegen/...` | 其余 CLI 闭包（配置、MCP、沙箱、workspace 等） |
| `docs/` | 功能点报告、专利交底、对话测试用例 |
| `scripts/` | 打包等辅助脚本（如 `pack-linux-arm64.sh`） |
| `share/examples/` | 示例配置 |

**请勿提交**（已在 `.gitignore`）：`target/`、`dist/`、`artifacts/`、`deploy_materials_*`、`docs/dialogue-tests/results/`、`**/.venv/`。

> [!IMPORTANT]
> 根目录 `Cargo.toml`（workspace 成员、依赖版本、lint、profile）为**生成文件**，请视为只读；优先修改各 crate 自己的 `Cargo.toml`。

## 日常开发

```sh
cargo check -p <crate>        # 请按 crate 构建；全 workspace 很慢
cargo test -p heartq-config   # 单 crate 测试示例
cargo clippy -p <crate>       # lint 配置见根目录 clippy.toml
cargo fmt --all               # rustfmt 配置见根目录 rustfmt.toml

# npm 安装逻辑冒烟（重命名后）
cd crates/codegen/heartq-pager/npm/heartq && node scripts/test-postinstall.js
```

## 贡献

> [!NOTE]
> 当前不接受外部贡献。详见 [`CONTRIBUTING.md`](CONTRIBUTING.md)。

## 许可证

本仓库第一方代码采用 **Apache License 2.0**，见 [`LICENSE`](LICENSE)。

第三方与内嵌代码仍遵循其原始许可证，详见：

- [`THIRD-PARTY-NOTICES`](THIRD-PARTY-NOTICES) — crates.io / git 依赖、内置主题、以及仓内移植实现等
- [`crates/codegen/heartq-tools/THIRD_PARTY_NOTICES.md`](crates/codegen/heartq-tools/THIRD_PARTY_NOTICES.md) — codex / opencode 相关移植说明
- [`third_party/NOTICE`](third_party/NOTICE) — 内嵌 Mermaid 相关索引
