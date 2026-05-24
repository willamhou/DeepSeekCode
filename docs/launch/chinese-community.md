# 中文社区宣发文案

适合 V2EX、掘金、开源中国、即刻、小红书技术号或微信群。主链接建议放 GitHub repo，
因为 README 里已经有安装命令、demo 和状态说明。

## 标题

```text
我做了一个类似 Claude Code 的 DeepSeek 终端代码 Agent，macOS 可 brew 安装，Linux 有 release 包
```

更克制的版本：

```text
DeepSeekCode v0.1.3：一个 DeepSeek-first 的终端 code-agent CLI
```

## 长帖

```text
最近把 DeepSeekCode 推到了 v0.1.3 public beta。

它是一个 DeepSeek-first 的终端 code agent，目标是做一个更接近 Claude Code / Codex CLI 的本地开发闭环：在终端里看仓库、改文件、跑检查、看 diff，然后继续同一个 session 迭代。

现在 macOS 可以直接用 Homebrew 试：

brew tap willamhou/deepseekcode
brew install deepseek
deepseek config init
printf '%s\n' '<api-key>' | deepseek config auth DEEPSEEK_API_KEY --stdin
deepseek quickstart
deepseek

Linux 用户可以走 GitHub Release archive 或源码安装；v0.1.3 的 Linux x64 / Linux arm64 release assets 已经纳入 release-smoke。

这版补齐了几个之前不敢公开推的东西：

- GitHub Release 二进制产物
- Linux x64 / Linux arm64 / macOS x64 / macOS arm64
- 已验证的 macOS Homebrew tap
- GHCR 镜像
- release-smoke：从公开 release 下载、校验 sha256、解压并跑最小验证
- README 里有真实 model-backed 的编辑/测试闭环 demo

当前边界也写清楚了：

- npm 还没正式发布
- Windows 不是这一阶段重点
- 还需要更多外部真实仓库 dogfood 样本
- 更适合愿意折腾终端和本地 code-agent workflow 的用户

Repo:
https://github.com/willamhou/DeepSeekCode

如果你平时用 Claude Code、Codex CLI 或其他终端 code agent，比较想听你们对 first-run、approval flow、shell 行为、repo editing 体验的反馈。
```

## 短帖

```text
DeepSeekCode v0.1.3 public beta 发了。

一个 DeepSeek-first 的终端 code-agent CLI，面向 Linux/macOS 本地开发闭环：看仓库、改文件、跑命令、看 diff、继续 session。

macOS 安装：
brew tap willamhou/deepseekcode
brew install deepseek
deepseek config init
printf '%s\n' '<api-key>' | deepseek config auth DEEPSEEK_API_KEY --stdin
deepseek quickstart
deepseek

Linux 可以用 GitHub Release archive 或源码安装，README 里有命令。

Repo:
https://github.com/willamhou/DeepSeekCode

现在主要想收 terminal-first code agent 用户的真实反馈。
```

## V2EX 发帖建议

- 节点：`分享创造` 或 `程序员`。
- 标题不要写成营销口号，直接讲「我做了什么」。
- 开头先给安装命令，再讲状态和限制。
- 明确说 npm 未发布，避免用户第一时间问 `npm install`。
- 不要引导点赞；只请求试用反馈和 issue。
