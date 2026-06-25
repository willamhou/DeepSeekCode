# 下一轮宣发与录屏计划

最后更新：2026-05-25

这份计划用于下一轮开干时直接执行。当前口径是：

> DeepSeekCode 是一个 DeepSeek-first 的 Linux/macOS 终端 code agent CLI public beta，可以在本地仓库里看代码、改文件、跑命令、看 diff、审批工具调用、回滚和继续 session。

不要宣称已经完全等同 Claude Code / Codex CLI。可以说 workflow 类似，目标用户是愿意尝试 DeepSeek-first、本地终端 code-agent 工作流的人。

## 7 天节奏

| 天数 | 产品动作 | 宣发动作 | 产出 |
| --- | --- | --- | --- |
| Day 1 | 复查 README 首屏，确保定位、安装、2048 demo、TUI 信任感证据入口清楚 | 发短帖：DeepSeek-first 终端 code agent 已可 public beta 试用 | README diff、短帖 |
| Day 2 | 做真实 bugfix 录屏：小型 Node/Rust/Python repo，先展示失败测试，再让 agent 修复 | 发“真实测试失败到修复，不是 toy demo” | 终端录屏、transcript、diff stat |
| Day 3 | 做 2048 交互式完整录屏：空目录、TUI/REPL 输入 prompt、生成文件、启动浏览器试玩、看 diff | 发 B 站/知乎/小红书友好的可视化内容 | GIF/MP4、短剪辑 |
| Day 4 | 做 TUI 信任感录屏：approval、diff review、rollback dry-run/apply、失败恢复 | 发“为什么 code agent 需要可审查和可回滚” | TUI 信任感 clip |
| Day 5 | 再顺一遍安装路径：npm/npx、Homebrew、Linux release archive/source install | 发“1 分钟安装 DeepSeek Code CLI” | 安装短视频、安装命令帖 |
| Day 6 | 写对比型长文：DeepSeekCode vs Claude Code / Codex CLI / Aider，只讲体验差异和边界 | 发知乎长文 | 知乎稿、README 链接 |
| Day 7 | 收集反馈，整理 issue template、FAQ、roadmap、下一批真实 repo dogfood | 发 public beta 反馈帖 | FAQ/roadmap issue |

## 可蹭的话题

- DeepSeek 模型也能做 Claude Code 类终端工作流。
- 国产模型驱动的本地 code agent。
- 低成本 Claude Code 替代体验，但保持 public beta 边界。
- 可审查 diff、可审批工具调用、可回滚，比黑盒自动改代码更安心。
- 用 DeepSeek Code CLI 从零写 2048。
- 真实 repo 失败测试到自动修复。

## 知乎长文结构

推荐标题：

```text
我用 DeepSeek 做了一个类似 Claude Code 的本地 Code Agent CLI
```

正文结构：

1. 为什么做这个：DeepSeek 用户缺少 terminal-first code-agent loop。
2. 和普通 chat 写代码的区别：本地仓库、工具调用、命令执行、diff、session。
3. 2048 demo：空 repo 到可玩浏览器游戏，适合作为视觉入口。
4. 真实 repo 修测试 demo：失败测试 -> agent 修复 -> 测试通过，证明实用性。
5. TUI 信任感：approval、diff review、rollback、失败恢复。
6. 如何安装试用：npm/npx、Homebrew、Linux release archive/source install。
7. 当前限制：Linux/macOS 优先，Windows/hosted IDE 不是当前重点，还需要更多真实 repo 样本。
8. 请求反馈：first-run、approval flow、shell 行为、repo editing、长任务恢复。

## 录屏测试矩阵

| 类型 | 录什么 | 成功标准 | 证明什么 |
| --- | --- | --- | --- |
| Greenfield | 空目录写 2048、Todo app、Markdown editor | 文件生成、校验通过、能本地运行或试玩 | 能从零生成可运行项目 |
| Bugfix | 真实小 repo，先跑失败测试，再让 agent 修复 | 初始测试失败，agent 修改代码，最终测试通过 | 能读上下文、定位 bug、验证修复 |
| Feature | 给已有项目加 CLI flag、API endpoint、React 组件、Rust parser 分支 | 多文件 diff 合理，相关测试通过 | 能做多文件功能开发 |
| Recovery | 故意让第一次测试失败，观察 agent 读错误、自修、重跑 | 至少一次失败后恢复，最终通过 | agent loop 有失败恢复能力 |
| Trust Flow | 展示 approval、`/diff`、rollback snapshot、dry-run/apply | 用户能看懂 agent 在做什么，能撤回 | TUI 信任感和安全边界 |

## 下一条最推荐录屏

优先录真实 bugfix，而不是再录一个纯 toy demo。**当前模型可靠区间是「单行/单文件 + 引导式 prompt」**——录屏选这个粒度，命中率最高；开放多文件自主修复目前不稳，不上宣传口径：

1. 用一个边界明确的小 repo（单文件、单行 bug，比如算子写反、off-by-one），先手动 `cargo test` 展示失败。
2. 打开 `deepseek tui`。
3. 输入**点名 prompt**（"两条测试失败，找 src/lib.rs 的 bug，修好并重新跑测试"），不要给"自主修整个 repo"那种开放式 prompt。
4. 让 agent 跑、改、复测。如果第一轮失败，保留失败和自修过程，不要剪掉。
5. 最终测试通过；打开 `/diff` 展示 changed files、hunk preview。
6. 展示 rollback snapshot 或 `revert turn last` dry-run。
7. 结尾显示 GitHub repo 和安装命令。

参考已落盘的版本：[docs/demo/deepseek-code-calc-bugfix.gif](../demo/deepseek-code-calc-bugfix.gif)（`exec` 行式录到的同一类 take，stuck-directive 触发后 agent 用小锚点 `apply_patch` 修绿）。一键复现/重录：
`docs/demo/record-calc-bugfix-exec.sh`（自动）或 `record-calc-bugfix-tui.sh`（手动录全屏 TUI）。

这个录屏比 2048 更能证明"真的能干活"。2048 用来吸引注意力；引导式 bugfix 用来说服用户。**避免**承诺"agent 能自主修任何 bug"——那是当前模型做不稳的事，宣传期撞上就会翻车。

## 录屏注意事项

- API key 永远不要入镜，不要进入 shell history。
- 选择小 repo，任务控制在 2-5 分钟内。
- 不要只展示最终结果，保留关键工具调用、测试失败和自修节点。
- 录屏里一定要出现 diff 或测试结果，否则说服力弱。
- 公开视频避免展示私人路径、私人仓库名、浏览器隐私信息。
- 对比 Claude Code / Codex CLI 时讲 workflow 和差异，不做攻击性 benchmark。
