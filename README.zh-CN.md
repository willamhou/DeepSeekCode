# DeepSeekCode

[English](./README.md) | [中文](./README.zh-CN.md) | [日本語](./README.ja-JP.md)

DeepSeekCode 是一个 DeepSeek-first 的终端 code agent，面向本地开发闭环：
阅读仓库、修改文件、运行检查、查看 diff，然后继续在同一个终端里迭代。

> Public beta 状态：今天已经可以用于 Linux/macOS dogfood 和仓库内代码任务。
> `v0.1.4` 已提供 GitHub Release 二进制、实测 GHCR 镜像、TUI/service smoke gate、
> `deepseek quickstart`、release-binary smoke verifier 和已验证的 Homebrew tap。
> README 中也已经有真实 model-backed 发布素材。`v0.1.4` 开始补齐 npm/npx
> 安装入口；更大型的外部仓库证据和更广的 hosted product proof 仍属于产品硬化工作。

<p align="center">
  <strong>DeepSeekCode 交互式 REPL 从空仓库写出可玩的 2048 游戏</strong><br>
  <img src="./docs/demo/deepseek-code-2048-interactive-demo.svg" alt="DeepSeekCode 交互式 REPL 录屏：真实 model-backed CLI 从空仓库生成可玩的 2048 游戏" width="100%">
</p>

<p align="center">
  <strong>生成后的游戏，本地试玩</strong><br>
  <a href="./docs/demo/deepseek-code-2048-interactive-gameplay.mp4">
    <img src="./docs/demo/deepseek-code-2048-interactive-gameplay.gif" alt="DeepSeekCode 真实 model-backed 2048 demo：交互式 REPL 生成可玩的浏览器游戏" width="100%">
  </a><br>
  <sub>两段录屏来自同一次真实交互式 <code>deepseek chat</code> 运行，目标仓库是一个空的临时 Web repo。</sub>
</p>

## 为什么做它

DeepSeekCode 的目标不是普通聊天壳，而是更接近 Claude Code CLI / Codex CLI
的终端开发体验。默认路径是 terminal-first、repo-aware：

- 在真实 TTY 中运行 `deepseek` 会打开全屏 coding-agent TUI。
- `deepseek chat` 保留行式 REPL。
- `deepseek run` 执行一次性代码任务。
- sessions、threads、events、tasks、usage 和 automations 会持久化到
  `.dscode/runtime/`。
- 文件工具、patch、diff review、rollback、todos、hooks、skills、subagents、
  diagnostics、MCP/ACP、本地 runtime API 共用同一套 permission 和 recovery 路径。
- shell 支持前台命令、后台 jobs、replay、bounded interactive attach、stdin、
  resize metadata、cancel 和本地 shell-supervisor bridge。

## 快速开始

通过 npm 安装：

```bash
npm install -g @deepseek-code/cli
deepseek version
deepseek quickstart
```

或者不安装直接运行：

```bash
npx @deepseek-code/cli version
npx @deepseek-code/cli quickstart
```

通过 Homebrew 安装（已验证 macOS x64/arm64）：

```bash
brew tap willamhou/deepseekcode
brew install deepseek
deepseek version
deepseek quickstart
```

或者从源码安装：

```bash
cargo install --git https://github.com/willamhou/DeepSeekCode.git --locked
deepseek version
deepseek quickstart
deepseek doctor --json
```

或者下载 release archive：

```bash
deepseek update download-plan --version 0.1.4
curl -L -o deepseek-linux-x64.tar.gz \
  https://github.com/willamhou/DeepSeekCode/releases/download/v0.1.4/deepseek-linux-x64.tar.gz
curl -L -o deepseek-linux-x64.tar.gz.sha256 \
  https://github.com/willamhou/DeepSeekCode/releases/download/v0.1.4/deepseek-linux-x64.tar.gz.sha256
shasum -a 256 -c deepseek-linux-x64.tar.gz.sha256
tar -xzf deepseek-linux-x64.tar.gz
./deepseek version
```

或者运行已发布的容器镜像：

```bash
docker run --rm ghcr.io/willamhou/deepseekcode:0.1.4 version
```

本地 checkout 安装：

```bash
cargo install --path .
deepseek quickstart
deepseek config init
printf '%s\n' '<api-key>' | deepseek config auth DEEPSEEK_API_KEY --stdin
deepseek doctor --json
```

执行代码任务：

```bash
deepseek
deepseek chat
deepseek run "explain the current repository structure"
```

启动本地 runtime 并让 TUI 连接：

```bash
deepseek serve --http --addr 127.0.0.1:13000
deepseek tui --runtime-url http://127.0.0.1:13000
```

真实模型调用需要设置 `DEEPSEEK_API_KEY`。本地 `.env` 文件会被 git 忽略。

## 已经可用

- 全屏 TUI：Plan / Agent / YOLO 模式、approval modal、command palette、
  setup/onboarding、provider/model picker 和 MCP 管理。
- REPL：raw-mode line editor、history、session list/load completion、
  SIGINT cancel、`/save`、`/load`、`/sessions` 和 custom slash commands。
- OpenAI-compatible 单个 tool call 与同轮 batch tool calls，都会经过正常的
  hook、permission 和 recovery 层。
- `deepseek quickstart` 与 `deepseek quickstart --json` 提供首跑检查。
- 本地 HTTP/SSE runtime、ACP stdio adapter、MCP client/server surface，以及由
  trust/approval 控制的 side-effect tooling。
- RLM helpers：递归/长输入分析、model-session context、live queue status、
  event replay、cancel、recover 和 drain controls。
- Linux/macOS/Windows entrypoint 已纳入 CI smoke；release assets 覆盖 Linux x64、
  Linux arm64、macOS x64、macOS arm64 和 Windows x64。
- 已提交真实 model-backed README demos，并记录 online multi-file external fixture
  证据。

## 当前限制

如果目标收敛到 Linux/macOS 本地 CLI，核心交互闭环已经成立。剩余差距主要是证据厚度
和产品硬化：

- disposable Python/Rust/Node 样本之外，可选再补更大型的真实外部 repo fixtures。

Windows 长尾 service proof、hosted IDE 证据和真实安装后的 service proof 属于更大的
产品硬化，不是 Linux/macOS 本地 code-agent CLI milestone 的 blocker。

## 证据与检查

常用本地检查：

```bash
cargo fmt --check
cargo test --lib -- --test-threads=1
node scripts/check-secrets.js
deepseek quickstart --json
deepseek update publish-status --json
deepseek update release-smoke --version 0.1.4 --json
deepseek tui --entrypoint-smoke --smoke-bin "$(command -v deepseek)"
```

发布和 dogfood 证据见：

- [Release checklist](./docs/release.md)
- [Dogfood evidence](./docs/dogfood-evidence.md)
- [Current status](./docs/current-status.md)
- 补充 demo：[scripted 2048 capture](./docs/demo/deepseek-code-2048-terminal-demo.svg)、
  [TUI 录屏](./docs/demo/deepseek-code-tui-demo.svg) 和
  [model-backed 编辑/测试闭环](./docs/demo/deepseek-code-model-demo.svg)

## 文档

- [安装](./docs/install.md)
- [Public beta 指南](./docs/public-beta.md)
- [当前状态与路线](./docs/current-status.md)
- [宣发素材包](./docs/launch/README.md)
- [发布 checklist](./docs/release.md)
- [Dogfood 证据](./docs/dogfood-evidence.md)
- [Demo 素材](./docs/demo/README.md)
- [架构](./docs/architecture.md)
- [DeepSeek-native loop design](./docs/deepseek-native-loop.md)
- [Runtime contract](./docs/runtime.md)
- [TUI workbench](./docs/tui.md)
- [REPL mode](./docs/repl.md)
- [Agent tasks](./docs/agents.md)
- [Skills and profiles](./docs/skills-and-profiles.md)
- [PR / CI integration](./docs/pr-integration.md)
- [Roadmap](./docs/roadmap.md)
- [Changelog](./CHANGELOG.md)

## 致谢

DeepSeekCode 为独立实现，但部分兼容接口和终端 agent 工作流设计参考了
[Hmbown/CodeWhale](https://github.com/Hmbown/CodeWhale)（原 DeepSeek-TUI）。
DeepSeekCode 没有 vendored 或复制 CodeWhale 源码；相关工作主要是接口与工作流兼容。

## 仓库说明

这个仓库公开是为了透明和协作。公开可见不代表在 [LICENSE](./LICENSE) 之外授予额外的
开源许可。

不要提交本地凭据、API keys、runtime state 或私有 `.env` 文件。已跟踪示例只使用占位符。
