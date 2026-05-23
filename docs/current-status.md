# DeepSeekCode 当前状态与后续路线

最后更新：2026-05-23

## 最终目标

DeepSeekCode 的目标是成为一个 DeepSeek-first 的 code agent CLI：用户在终端里运行
`deepseek` 后，可以像使用 Claude Code CLI、Codex CLI 或 DeepSeek-TUI 一样，完成真实
仓库里的读代码、改代码、跑命令、查看 diff、继续修复、恢复会话和发布前验证。

最终验收口径不是“功能列表看起来很多”，而是：

- 裸 `deepseek` 在真实 TTY 里稳定进入 coding-agent TUI；
- 模型能稳定调用文件、shell、patch、diagnostics、review、runtime、subagent、MCP/ACP 等工具；
- 一个真实代码任务可以从需求进入、修改文件、跑测试、修失败、输出 diff 和总结；
- shell/PTY、审批、回滚、secret scan、dogfood 和 CI gate 都能证明行为可靠；
- 安装分发覆盖 GitHub Release、GHCR、npm、Homebrew，并且 README 能让新用户快速理解和试用；
- 和 Claude Code CLI / Codex CLI / DeepSeek-TUI 的核心使用差距收敛到 5% 以内。

## 当前做到哪里

当前项目已经不是早期原型，可以直接用于仓库内 dogfood 和中小型代码任务。当前主入口是
`deepseek`，历史兼容入口 `dscode` 仍保留。

已经具备的核心能力：

- 终端入口：`deepseek`、`deepseek chat`、`deepseek run`、`deepseek tui`、`deepseek exec`。
- TUI：全屏 workbench、Plan / Agent / YOLO 模式、approval modal、command palette、session/thread 视图、MCP 管理、setup/onboarding、provider/model picker。
- Runtime：`.dscode/runtime/` 下持久化 sessions、threads、turns、items、events、tasks、usage、automations。
- 工具：文件读取/搜索、patch、diff、shell、background jobs、diagnostics、review、notes、memory、rollback、skills、subagents。
- 模型协议：OpenAI-compatible tool calls，同轮 batch tool calls，DeepSeek provider/model alias 兼容。
- 审批与安全：approve-once、approve-for-session、deny fingerprint、secret scan、shell/network policy、rollback snapshot。
- Shell/PTY：后台 shell job、wait/replay/attach/stdin/resize/cancel；Linux native-supervisor PTY；workspace shell-supervisor protocol bridge。
- 本轮新增：`deepseek chat` / `deepseek repl` / `deepseek interactive` 的真实 TTY 输入现在走内置 raw-mode line editor，补齐 Claude Code-like REPL 的 Up/Down history、history draft restore、左右移动、Home/End、Backspace/Delete、Ctrl+A/E/U/K/W、Tab slash/session completion、空行 Ctrl+D 和 prompt Ctrl+C 退出；运行中的 REPL turn 也会把 SIGINT 接到 `AgentLoopOptions.cancel_check`，让模型 stream 和 cancel-aware tools 协作取消，并在取消后恢复本轮 transcript/snapshot 指针，避免半截 prompt 污染后续上下文；`/sessions [prefix]` 可以列出保存的 REPL session，`/load ` 后 Tab 可补全 session 名；非交互测试路径仍保留 buffered reader，不需要真实终端。
- 本轮新增：Phase 12E background worktree runner 第一片。新增 `deepseek task start/list/show/stop/diff/merge/reject` 和 `deepseek task fixture-smoke --json`：`task start` 会在当前 git repo 的 `.dscode/task-runner/worktrees/<id>` 创建隔离 worktree 和默认 `deepseek-task/<id>` 分支，把记录写到 `.dscode/task-runner/records/`，stdout/stderr 写到 `.dscode/task-runner/logs/`，并在该 worktree 中启动 `deepseek exec --json`；父 CLI 退出后 child 进程仍可继续。`--no-run` 可只创建 worktree/record，用于无 API key 的本地 gate；`task diff` 展示 task worktree 的 tracked patch/stat 和 untracked files，`task merge --check` dry-run 验证，`task merge` 要求原 worktree 干净后把 patch 和 untracked regular files 合回原 repo，`task reject` 默认删除受管 task worktree 并把记录标记为 rejected。`deepseek github action --background-task` 现在也可把解析出的 GitHub PR review/fix/patch 请求委派到同一 task runner，`--task-id` 支持 workflow 稳定 id，`--task-no-run` 支持无凭据本地 workflow gate。当前 `task fixture-smoke --json` 实测 `ok=true`、`worktree_created=true`、`record_listed=true`、`merge_check_ok=true`、`merge_apply_ok=true`、`reject_ok=true`、`cleanup_ok=true`；CI 已把该 smoke 接到 Linux/macOS/Windows debug binary，Release Matrix 也会在各平台 release binary packaging 前运行。
- 本轮新增：`deepseek agents shell attach <task_id> --interactive` / `--takeover`。它会进入本地 raw mode，把按键转发到 supervisor `stdin`，把 resize 转发到 supervisor `resize`，并把 output 事件的 raw bytes replay 回当前终端；Linux 集成 smoke 已覆盖 raw-mode PTY 启动、`tty=true` job、stdin、resize、replay 和 bounded detach。它是可用的 bounded interactive attach，不是字节级 PTY fd 直连代理。
- 本轮新增：shell-supervisor terminal event log 已为 PTY output/input 记录可选 `raw_base64`，supervisor `attach` JSON 响应会透出结构化 `terminal_raw_outputs`，`exec_shell_attach` 摘要仍带兼容的 `terminal_raw_base64` section。human `--follow` 现在走同一个 Unix socket 上的 `attach_stream` newline-JSON frame stream，`--follow` / `--interactive` 会优先解码 output raw bytes；`deepseek agents shell attach <task_id> --raw` 可为脚本直接输出 PTY bytes。新增 `deepseek agents shell byte-stream <task_id>` / supervisor `byte_stream`，可在一个 socket stream 里处理初始 stdin/resize、后续 newline-JSON stdin/resize/close/detach control frames，并以 `byte_outputs[].bytes_base64` 输出 raw PTY bytes；`--raw-proxy` / `raw_proxy=true` 会在初始 JSON 后切到原始 socket bytes：socket input 直接进 PTY stdin，PTY output bytes 直接写回 socket。`deepseek agents shell proxy <task_id>` 现在是面向人的 raw-proxy wrapper，会进入本地 raw mode、同步终端尺寸、转发 key/paste/resize、直接写回 PTY bytes，并用 `Ctrl-]` detach。Linux 上还新增 supervisor `pty_fd` / `deepseek agents shell fd-proxy <task_id>`：通过 SCM_RIGHTS 把 native-supervisor PTY master fd 临时交给本地 Unix client，handoff 期间暂停 supervisor replay reader，detach 后恢复事件记录；测试已覆盖 fd 交还后普通 supervisor `resize`、`stdin`、terminal `replay` 继续可用，Ctrl-D EOF 触发目标 PTY 关闭时 fd-proxy 把 Linux `EIO` 当作正常 EOF 成功退出，运行中本地终端 SIGWINCH resize 会同步到目标 PTY，Ctrl-C 会中断目标 PTY 的 foreground process group，以及 fd-proxy 被 SIGKILL 后 handoff lease 释放并恢复普通 supervisor 控制。HTTP SSE、ACP `session/shell/subscribe` 和 MCP `exec_shell_terminal_events` progress metadata 也会透出 raw bytes。
- 本轮新增：`deepseek agents shell-fixture-smoke --json` 本地 Shell/PTY gate。它创建短路径临时 workspace，启动当前二进制的 `agents shell-supervisor --json`，验证 `health`、start/wait/attach/replay、Linux native PTY stdin/resize/replay/cancel，并把 `byte_stream` duplex control frames、`raw_proxy=true` 原始 socket bytes、Linux `pty_fd` fd handoff 和 human `agents shell proxy` raw-mode wrapper 纳入同一可复跑 smoke。当前实测 `blockers=0`、`warnings=0`，且 `shell_control` 摘要包含 `byte_stream duplex/raw_proxy/fd_handoff/human_proxy smoke passed`；`service-smoke` 的 shell-supervisor control smoke 也同步覆盖这些 byte-stream/proxy/fd-handoff 切片。聚焦测试还验证了 direct `pty_fd` 和 CLI `fd-proxy` detach 后，普通 supervisor 控制面可继续 resize/stdin/replay 同一个 PTY job。
- 本轮新增：VS Code `DeepseekCode Agent` panel 的 native workbench 继续推进。侧栏 webview 现在直接启动 `deepseek exec --json`，在 panel 内流式显示 assistant delta、reasoning log、tool call/result、permission request、stderr 和完成状态，并把 active file、selection、diagnostics、dirty-buffer marker、Git status/diff summary 注入任务上下文；post-run 控件已支持 active-file `Review Diff`、panel 内 `Accept` 标记、确认后的 `Revert File`、`Refresh Diff`、workspace validation command 输出捕获、`Resume Latest` 继续最近 runtime session、workspace changed-file queue 的 open diff / mark reviewed / confirmed revert，以及 generated patch artifact queue。模型 final/tool result 里的 unified diff 会进入独立队列，单文件 patch 可打开 VS Code diff，pending patch 可确认后 `git apply --check` + `git apply`，也可 reject；`apply_patch` tool call 的 patch 会以 captured 状态记录，避免和已写入的 Git changes 混在一起。扩展目录也新增了 mocked DeepSeekCode binary 的 extension-host smoke harness，以及可在当前机器运行的 headless panel fixture；后者用临时 Git repo 证明 diagnostic context injection -> generated patch capture -> single-file diff opening -> checked `git apply` -> workspace queue refresh -> validation pass 的闭环，并修掉了 generated patch `-p0` fallback 和 `git status --short` 前导空格解析问题。当前机器没有 `code`/`codium` CLI，所以真实 VS Code runner 执行证据仍缺；这仍不是完整 Phase 12B，manual GUI fixture 证据还需要继续补。
- 本轮新增：GitHub Action bridge 第一片。`deepseek github action` 会读取 `GITHUB_EVENT_PATH` / `GITHUB_EVENT_NAME`，把 `pull_request`、`issue_comment`、`pull_request_review`、`pull_request_review_comment` 事件解析成 `owner/repo#PR`，comment/review 事件默认要求 `@deepseek` trigger。`--mode auto|review|fix|patch` 会复用现有 `deepseek pr review/fix/patch` 路径，`@deepseek fix` / `@deepseek patch` 可自动路由到 CI-log repair / patch workflow；`--background-task` 可改为创建 `deepseek task start` 背景 worktree 记录，`--task-id` / `--task-no-run` 支持稳定 workflow id 和无凭据 gate；`--dry-run` 只输出解析目标，`--github-output` 会把 target fields 写入 `$GITHUB_OUTPUT`，`--require-mode` 可让 write workflow 对非 fix/patch trigger fail fast，`--post` 通过现有 `gh pr comment` 发布 review summary。`deepseek github pr-head` 会把 PR head owner/ref 解析为 CLI-tested step output，并在 write-capable checkout 前拒绝 fork-owned PR branch。`deepseek github fixture-smoke` 现在可本地 no-network 验证 review/write trigger、同仓库 PR head guard、fork guard、临时 Git remote 上的 checkout/commit/push、pushed-head 校验和 background-task worktree/record 创建；CI 和 Release Matrix 也会用 debug/release binary 跑这个 smoke。仓库新增 disabled-by-default 的 `.github/workflows/deepseek-code-review.yml` 示例，需设置 `DEEPSEEK_CODE_REVIEW_ENABLED=true` 和 `DEEPSEEK_API_KEY` 后才运行；示例 workflow 固定 `--mode review` 作为安全默认。仓库也新增 disabled-by-default 的 `.github/workflows/deepseek-code-write.yml` 写入示例，需设置 `DEEPSEEK_CODE_WRITE_ENABLED=true`，它只响应 `@deepseek fix/patch`，先用 CLI dry-run 解析 PR 并输出 step outputs，再通过 `deepseek github pr-head` 解析并校验同仓库 PR head，运行 `--mode auto`，最后 commit/push 工作区改动。默认 benchmark manifest 也补到 `26` 条 `pr_workflow` cases，新增 action-labeled review-comment-plan、`@deepseek fix`、`@deepseek patch` 和 hosted exact replacement request 覆盖。
- 本轮新增：真实 hosted GitHub workflow 证据。PR #10 验证了 hosted write bridge 修复，`@deepseek patch change ... becomes ...` 由 GitHub Actions 成功生成并推送 `6fd5010 deepseek: apply requested PR update`；PR #11 在修复合入默认分支后重新做 post-merge smoke，同一路径再次生成并推送 `f0fe9a7 deepseek: apply requested PR update`；PR #12 删除了两份临时 evidence/smoke fixture，merge commit 为 `9423126`。三个临时分支已清理，证据保留在 PR 历史和对应 workflow run 记录里。
- 本轮新增：Phase 12D 的 MCP fixture smoke 第一片。`deepseek mcp fixture-smoke --json` 会创建临时 fixture workspace 和 MCP config，用当前二进制的 `serve --mcp` 验证 stdio discovery/call，再启动本地 loopback HTTP/SSE MCP fixtures 验证 HTTP/SSE discovery/call，并通过默认 agent tool registry 验证动态 `mcp__server__tool` 暴露和 input schema cache。当前实测 stdio 工具发现 `57` 个、HTTP/SSE 各 `1` 个，三类 call 均通过，动态 schema cache 覆盖 `stdio-self/read_file`、`http-fixture/echo`、`sse-fixture/echo`。fixture 现在还会配置一个故意失败的 `broken-stdio` server，并证明它不会隐藏或破坏健康 server 的动态工具发现；同时验证 generic `mcp_call` 和动态 `mcp__stdio-self__read_file` 的 permission request、allowlisted allow、allowlist deny 三条 policy 路径。最新 JSON 字段 `bad_server_isolated`、`mcp_call_permission_ok`、`dynamic_permission_ok`、`mcp_call_allow_ok`、`mcp_call_allowlist_deny_ok`、`dynamic_allow_ok`、`dynamic_allowlist_deny_ok` 均为 `true`。本轮还把 prompts/resources/templates 纳入同一个 fixture smoke：stdio/HTTP/SSE 的 prompt discovery/get、resource discovery/read、resource template discovery 均通过，最新 JSON 中 `stdio_prompt_ok`、`http_prompt_ok`、`sse_prompt_ok`、`stdio_resource_ok`、`http_resource_ok`、`sse_resource_ok` 均为 `true`，template counts 为 `3/1/1`。这补上了 completion gate 中 MCP stdio/HTTP/SSE tool discovery/call/schema 注入、prompt/resource/template、bad-server isolation、MCP tool approval/allowlist 的本地证据。
- 本轮新增：Phase 12D hooks fixture smoke。新增 `deepseek hooks fixture-smoke --json`，它创建临时 hook root 和 workspace，安装结构化 `{"decision":"allow","add_context":"..."}` recorder scripts，然后通过真实 `AgentLoop::run_with_client` 触发 `list_files` 工具调用。当前实测 JSON 为 `deepseek.hooks_fixture_smoke.v1`，`session_start_ok`、`user_prompt_submit_ok`、`pre_tool_ok`、`post_tool_ok`、`session_stop_ok`、`hook_contexts_ok`、`tool_ran_ok` 全为 `true`，事件顺序为 `session_start -> user_prompt_submit -> pre_tool_use -> post_tool_use -> session_stop`。这给 Phase 12D hooks prompt/session/tool lifecycle 和 structured allow/add_context 提供了单命令本地 gate。
- 本轮新增：Phase 12D skills/custom command validation。新增 `deepseek skills list [--json]` 与 `deepseek skills validate [--strict] [--json]`，按运行时同一优先级扫描 bundled skills 和 `workspace.user_skills_dir`，报告同名覆盖、loader 错误、空核心元数据、空 `allowed_tools` 和未知工具名。当前 `cargo run --quiet -- skills validate --strict --json --dir skills` 实测 bundled `skills/` 共 `16` 个 skill，`valid_files=16`、`error_count=0`、`warning_count=0`、`ok=true`。`docs/skills-and-profiles.md` 也补了 PR review、release-check、security-lite 三类 skill 示例。这给 Phase 12D skill metadata validation 和 command/skill discovery 提供了单命令本地 gate。
- 本轮新增：Phase 12D subagent fixture smoke。`dispatch_subagent` / `dispatch_subagents` 现在支持 `write_scope` / `write_set` 元数据，child prompt 会带 assigned write scope，parallel summary 会输出 `meta.parallel_blocked_children`、`meta.parallel_readback_required`、`meta.parallel_next_action`、`meta.parallel_child_N_files`、`meta.parallel_child_N_write_scope` 和 `meta.parallel_write_scope_conflicts`。父 planner 现在会同时消费 `dispatch_subagent` 与 `dispatch_subagents` 汇总，看到 child files 后先 `read_file` 回读再继续。新增 `deepseek agents subagent-fixture-smoke --json` 本地 gate，当前实测 `parser_ok`、`disjoint_write_scope_ok`、`readback_required_ok`、`blocker_summary_ok`、`conflict_summary_ok`、`artifact_ok` 全为 `true`，`child_count=2`。默认 benchmark manifest 已有 `20` 条 `subagent` category cases；benchmark runner 现在支持 `--category <name>` 与可重复 `--case <name>`，filtered run 只生成 report，不推进 history，也不强制全量 trend/live gate。当前 targeted subagent benchmark 实测 `20/20`，报告在 `/tmp/deepseek-subagent-benchmark.md`，trend/live 均标记为 filtered selection skip。本轮补上了并发 subagent 汇总的 readback 单测、fixture gate 和 targeted benchmark evidence。
- 本轮新增：MCP 模型规划 benchmark 第一片。benchmark runner 现在支持 per-case 自举 `stdio-self` MCP fixture，可在 isolated workdir 写入 `.dscode/mcp.json`、按 case 开关动态 MCP 工具暴露，并设置 case-local `mcp_call_allowlist`。默认 manifest 新增 `fixture-mcp-dynamic-readme`、`fixture-mcp-generic-call-readme`、`fixture-mcp-allowlist-deny-recovery` 三条 MCP cases，分别覆盖动态 `mcp__stdio-self__read_file`、generic `mcp_call`、allowlist deny 后通过 `mcp_list_tools` 恢复。本轮 targeted MCP manifest 从 `/tmp` 运行实测 `3/3`，报告在 `/tmp/deepseek-mcp-benchmark.md`；完整默认 manifest 已扩到 `82` cases，并已刷新通过 `82/82`，最新完整报告在 `.dscode/benchmarks/latest.md`。
- 本轮新增：benchmark PR planner hardening。离线 planner 现在会保留 `github_pr_context` / `review` / `pr_review_comment_plan` 这类结构化观察，不会因为同属 `Other` kind 被后续工具压缩成 superseded stub；PR comment 失败恢复在成功重建 plan 后不再重复重建第二次；skill auto-select 也会避免把远程 PR review/comment 任务降级到隐藏 `github_pr_context` / `review` / `pr_review_comment_plan` 的 debug skill。`run_shell` 现在会把 `pytest` / `python -m pytest` 缺失以及 profile 生成的 `uv run pytest` 安全标准化到 `uv --with pytest` fallback，并会自动发现 `~/.local/toolchains/go/*/bin` / `~/sdk/go*/bin` 这类用户级 Go toolchain。当前完整离线 benchmark 历史实测 `82/82`；本轮新增 hosted exact patch request targeted benchmark 通过，`26` 条 `pr_workflow` 中 planner/action/comment-plan/exact-request 项均有覆盖，新增 `mcp` category `3/3`；当前 trend gate 处于 comparable warmup，`found 2`，live gate 基于本机 dogfood ledger 从 `runs=5` 到 `runs=20` 通过。
- 本轮新增：offline dogfood replay 覆盖补强。`cargo run --quiet -- dogfood replay-benchmark --category pr_workflow --limit 12 --benchmark-gate` 新增了 12 条 `pr_workflow` replay，覆盖 GitHub Action `@deepseek fix` / `@deepseek patch`、JS/Rust/Python/Go PR CI repair、PR retry validate、second-round feedback 和 Go patch validate，全部成功；该批次把 ledger 推到 `17` runs 后暴露 live coverage gate 还缺 `recovery` slice。随后 `cargo run --quiet -- dogfood replay-benchmark --category recovery --limit 3 --benchmark-gate` 新增 3 条 recovery replay，`recovery` 为 `3/3`，最终 `.dscode/dogfood/latest.md` 为 `20` runs、`19/20` success、`1` historical failed、`0` stuck、`0` manual；post-replay default benchmark 为 `82/82`，live gate `pass against previous dogfood snapshot (runs 5 -> 20)`。这些仍是 `offline` transport，不能替代后续真实 model-backed dogfood。
- 本轮新增：`deepseek dogfood live-plan` 的推荐命令改为 `deepseek dogfood live-run ...`，文本和 JSON 都同时输出 dry-run 与 `--execute` 命令，避免 release operator 为 model-backed 证据误走 offline-friendly `replay-benchmark` 路径。`deepseek dogfood live-plan` 和 `deepseek dogfood live-run --json` 现在还输出 `post_run_report_command` / `evidence_gate`，直接给出 `dogfood report --require-live-runs ... --require-live-category ...` 的后置验收命令，让真实 online 执行后的 model-backed 证据可以 fail closed。`deepseek dogfood live-run --json` 保持机器可读 dry-run plan，包含 selected cases、online readiness、execute blocker 和 follow-up `--execute` command；它故意不和 `--execute` 混用，避免在线执行日志污染 JSON。`dogfood live-run` 还支持 `--api-key-file`/`--key-file` 指向仓库外 key 文件，只把 key 注入当前进程的 `model.api_key_env` 并在返回时恢复，JSON 只记录 `credential_source` 和文件路径，不输出 key 值。`dogfood live-run --execute --evidence-out <path>` 现在会在批次结束或首个失败后写出 `deepseek.dogfood.live_run_evidence.v1` JSON，记录 before/after ledger live counts、每个 case 追加的 model-backed ledger 行、benchmark gate 结果、同一条 post-run report gate，以及当前 ledger 文件的 `fnv1a64` fingerprint，仍不写入 API key 值。`deepseek dogfood live-evidence --file <path>` 现在可验证该 evidence 文件，默认要求 completed、online、至少 1 条 appended model-backed row；`--require-benchmark-gate` 可把 benchmark gate 也纳入 release fail-closed 检查，`--require-report-gate` 会读取 evidence 的 structured `evidence_gate` 和 ledger path，用 `dogfood report` 同一套 live requirement 逻辑验证 full live gate，重新计算 ledger fingerprint 并逐条核对 evidence 中 appended case 的 timestamp/outcome/model_transport/category 能在 ledger 中找到匹配记录，而不是执行 JSON 里的 shell command；`--json` 输出 `deepseek.dogfood.live_evidence_verification.v1`，`--out <path>` 可把 verification JSON 落盘作为 release evidence artifact。`dogfood external-fixture` 真实执行现在也默认要求 `model_transport=online`，离线只能 dry-run 或显式 `--allow-offline` 做 rehearsal，避免把 offline disposable repo 样本误计为 release evidence；`--evidence-out` 会写出 `deepseek.dogfood.external_fixture_evidence.v1`，包含 appended external fixture row、release-evidence readiness 和 ledger fingerprint，便于上传发布证据。
- 本轮新增：在线 DeepSeek dogfood 从 smoke 推进到完整 release gate。使用当前进程注入的 DeepSeek key 执行 `dogfood live-run --execute --evidence-out ...`，最终 `deepseek dogfood report --limit 100 --require-live-runs 100 --require-live-success-rate 90 --require-live-category write_validate:25:90 --require-live-category recovery:25:90 --require-live-category pr_workflow:25:90` 通过；外部 fixture 跑完后 `live-plan` 显示 `105` 条 online run、`99` 条 success，分类为 `write_validate 29/30`、`recovery 23/25`、`pr_workflow 47/50`。执行过程中又修掉两类真实模型卡点：Python pytest retry readback 现在能识别 `def test_` / `assert ` 测试文件，并从错误的 `a * b` 回退到 `a + b`；空搜索恢复任务在看到 no matches 后完成 repository layout inspection 会 clean finish，不再重复列目录。release evidence verification 落在 `.dscode/dogfood/live-evidence-final-total-pr-4-release-verification.json`，`report_gate_passed=true`。
- 本轮新增：外部 disposable repo write-fixture 证据第一批。已在 `/tmp/deepseek-external-fixtures/` 下构造 Rust、Python、JavaScript 三个独立 git repo，初始测试均失败，然后用真实 online DeepSeek 跑 `dogfood external-fixture --workdir ... --evidence-out ...`，三条都完成 `read_file -> apply_patch -> validation -> finish`，并分别通过 `dogfood external-evidence --require-successful-external-fixtures 1`：`.dscode/dogfood/external-fixture-rust-add-v3-verification.json`、`.dscode/dogfood/external-fixture-python-add-verification.json`、`.dscode/dogfood/external-fixture-js-add-verification.json`。本轮还修复了 external fixture evidence record 缺少 `model_backed` 字段导致 verifier 无法和 ledger online row 对齐的问题。
- 本轮新增：README 真实 model-backed demo SVG。`docs/demo/record-model-backed-demo.sh` 使用当前 DeepSeek key 录制了 disposable Rust crate 的 failure -> `deepseek exec` -> patch -> passing `cargo test` -> diff transcript，`docs/demo/verify-model-backed-demo.js` 验证通过后由 `docs/demo/render-model-backed-demo-svg.js` 渲染为 `docs/demo/deepseek-code-model-demo.svg`。本轮还修复了 explicit edit parser 对 `in src/lib.rs, validate ...` 的路径截断问题，以及 renderer 把 `test result: ok ... 0 failed` 误标红的问题；README 英文、中文、日文都已引用该真实模型 SVG。
- 本轮新增：`deepseek update publish-status` 现在支持 `--live-evidence-verification <path>`（别名 `--live-evidence`），会读取 `dogfood live-evidence --out` 生成的 `deepseek.dogfood.live_evidence_verification.v1`，要求 `ok=true`、completed、online、appended model-backed row、report gate required/passed、ledger fingerprint/current ledger fingerprint 都成立。`--strict` 因此会把缺失或无效的 online dogfood verification artifact 计入 not-ready，`public_install` 对 GitHub Release、npm、Homebrew 和 GHCR 的 `ready_to_publish` 也不再只看包材料，还要求 release evidence 已验证。
- 本轮新增：Windows target warning cleanup。Unix-only shell byte-stream/PTY helpers、hook fixture helpers、rollback Unix metadata helpers 和相关测试 fixture 现在只在对应 Unix cfg 下编译；`cargo check --target x86_64-pc-windows-gnu --all-targets` 当前已无 warnings 通过。这让 Windows ConPTY/TCP runtime proof 的编译面更接近 release-quality，而不是只做到“能编过但带一串条件编译噪音”。
- 发布面：`v0.1.1` GitHub Release binaries、GHCR image、npm/Homebrew packaging metadata、release matrix、download-plan、publish-status、README 多语言、README TUI demo recorder。
- CI 证据：Linux/macOS/Windows bare `deepseek` TUI entrypoint smoke 已经在 CI 里通过；Windows 路径使用 ConPTY-backed smoke。

当前可以怎么用：

```bash
deepseek
deepseek chat
deepseek run "explain this repository"
deepseek tui --entrypoint-smoke --smoke-bin "$(command -v deepseek)"
deepseek agents service-smoke --workdir /tmp/dsc-smk --bin "$(command -v deepseek)" --json
deepseek agents shell-fixture-smoke --json
```

真实模型调用需要配置 DeepSeek API key。不要把 key 写进仓库；推荐使用环境变量或仓库外文件。

## 还差什么

当前距离 Claude Code CLI / Codex CLI / DeepSeek-TUI 的成熟产品形态，主要差在以下几类：

1. Shell/PTY 深水区
   - 已有 bounded interactive attach、duplex `byte_stream` raw-output proxy slice、human `agents shell proxy` raw-mode wrapper、Windows `native-supervisor` ConPTY backend compile gate，以及 Linux 本地 `pty_fd` / SCM_RIGHTS PTY master fd handoff slice。
   - `deepseek agents shell-fixture-smoke --json` 已把 Linux native PTY、duplex `byte_stream`、`raw_proxy`、`pty_fd` fd handoff 和 human `agents shell proxy` wrapper 纳入本地单命令 gate；direct `pty_fd` 与 CLI `fd-proxy` 测试已覆盖交还后 supervisor stdin/resize/replay 恢复，CLI `fd-proxy` Ctrl-C、Ctrl-D/PTY EOF、SIGWINCH resize 和异常 client 退出恢复也已有集成测试。
   - Linux shell-supervisor native PTY 已有；Windows shell-supervisor ConPTY 已接入 `portable-pty` 后端，daemon/client 控制面已有 loopback TCP 第一片，端点写入 `.dscode/shell-supervisor/supervisor.tcp` 并复用 newline JSON 协议；`cargo check --target x86_64-pc-windows-gnu --all-targets` 已通过，CI 已新增 Windows endpoint/status、TCP daemon/client runtime smoke、真实二进制 `agents shell-fixture-smoke --json` 和 targeted ConPTY start/resize smoke，但真实 Windows runner 结果仍需产出后才能关闭该证据缺口。Windows `byte_stream/raw_proxy` 和 `pty_fd` 仍未支持。
   - `service-doctor` 已有本地模板安装前 gate：会解析生成的 systemd `ExecStart`/`WorkingDirectory` 和 launchd `ProgramArguments`/`WorkingDirectory`，并精确比对 expected argv/workdir；`service-doctor --installed` 会只读检查实际 user service 的 systemd unit 或 launchd label 是否 loaded/running/enabled；`service-smoke --installed` 会继续探活实际 runtime `/health` 和已有 shell-supervisor endpoint，Unix 读取 socket、Windows 读取 `supervisor.tcp`，且不会停止 service-manager-owned 进程。真实干净机器安装后的 systemd/launchd service smoke 证据仍需要外部环境产出。

2. 真实模型 dogfood 证据
   - 已有 recorder、verifier、redaction self-test、release evidence verifier 和 `100` 条 online run release gate 证据。
   - 已有 `3` 个真实 disposable repo 外部 write-fixture 样本，覆盖 Rust/Python/JavaScript 的 failure -> edit -> test 链路；还可以继续扩到 5 个样本并补一个更接近真实项目的 multi-file fixture。
   - README 现在已有真实 model-backed SVG；后续可选补更精致的 GIF/MP4 或 TUI 录屏版。

3. 发布渠道
   - GitHub Release 和 GHCR 已通。
   - npm registry 和 Homebrew tap 还被凭据阻塞。
   - crates.io 是否发布仍需要明确 crate 命名、license/package policy。

4. 产品打磨
   - TUI 已能用，但还需要更多真实工作流下的性能、长输出、失败恢复、窗口 resize、旧终端兼容性验证。
   - VS Code 已有 native panel、resume、active-file review、workspace changed-file queue、generated patch queue/apply/reject、extension-host smoke harness、headless diagnostic patch fixture 和 validation 第一片，但完整 IDE agent workbench 仍缺真实 VS Code CLI runner 证据和 manual GUI fixture 证据。
   - GitHub automation 已有 event bridge、review/fix/patch mode routing、review workflow 示例、写入型 PR-head checkout workflow 示例、本地 workflow fixture smoke、真实 hosted write workflow 证据、`26` 条 `pr_workflow` benchmark cases，以及 `14/14` offline `pr_workflow` dogfood replay；当前本机已有 `50` 条 online model-backed `pr_workflow` 样本、`47` 条 success，分类 release gate 已通过。后续重点是把这类远端证据沉淀成更稳定的周期性 smoke，而不是继续保留临时 fixture 文件。
   - MCP 已有 stdio/HTTP/SSE 本地 fixture smoke、动态工具暴露、schema cache、prompt/resource/template 单命令 smoke、bad-server isolation、generic/dynamic MCP approval/allowlist policy 证据，也已有三条模型规划型 MCP benchmark 和完整默认 benchmark `82/82` 证据；当前没有已知 MCP-specific Phase 12D smoke 缺口。
   - Hooks 已有 prompt submit、session start/stop、pre/post tool use 和 structured allow/add_context 的 `deepseek hooks fixture-smoke --json` 本地 gate；skills/custom command 已有 `deepseek skills validate --strict --json` 元数据 gate 和 discovery 文档；subagent 已有 `deepseek agents subagent-fixture-smoke --json` gate、20 条 subagent benchmark cases、targeted subagent benchmark `20/20`、完整默认 benchmark `82/82`、并发 child readback 和 write-scope conflict metadata。Phase 12D 本地 extension gates 已基本齐备，offline dogfood coverage gate 和 online dogfood release gate 都已通过；下一步应继续外部兼容性和真实 demo 证据。
   - 文档需要继续压缩成新用户能快速理解的安装、配置、试用、故障排查路径。
   - 和上游 DeepSeek-TUI 的新变化需要持续周期性 refresh。

## 下一步优先级

建议按这个顺序推进，避免在低价值 polish 上分散：

当前执行 spec：`docs/superpowers/specs/2026-05-23-final-parity-execution-spec.md`。
该 spec 固化了本轮重新核对后的剩余差距、可执行命令、外部阻塞项和停止条件。

1. 做 Shell/PTY 跨平台和安装态证明
   - 在现有 `raw_base64` terminal event、`attach_stream` frame channel、duplex `byte_stream` proxy slice、human `agents shell proxy` wrapper、Windows ConPTY/TCP daemon smoke wiring 和 Linux `pty_fd` fd handoff edge coverage 基础上，收集 Windows CI ConPTY/TCP smoke 结果和 installed service smoke。
   - Windows shell-supervisor 下一步是拿到 CI runner 的 TCP daemon/client、真实二进制 shell fixture、targeted start/resize smoke 证据；如果 loopback TCP 不能满足安装态要求，再评估 named pipe。

2. 补外部 model-backed 证据和真实 demo
   - 先轮换任何已经泄漏到聊天记录里的 key。
   - 保留 `.dscode/dogfood/live-evidence-final-total-pr-4-release-verification.json` 作为当前 online dogfood release 证据。
   - 已完成 3 个 disposable repo/write-fixture 样本；下一步可以扩到 5 个，并补一个 multi-file 或 dependency-backed 的真实项目样本。

3. 补 README 真实录屏
   - 已完成 CLI 版真实模型 SVG：失败测试、模型修改、通过测试和 diff。
   - 后续如果要更强视觉冲击，可以补 TUI/GUI 风格 GIF/MP4，但不是当前 minimum evidence gap。

4. 完成发布渠道
   - 配置 npm token 并发布 npm wrapper。
   - 配置 Homebrew tap token 并发布 formula。
   - 决定 crates.io 是否进入 v0.2 目标。

5. 最后一轮差距审计
   - 重新拉取 DeepSeek-TUI 最新 main。
   - 和 Claude Code CLI / Codex CLI 的核心 loop 对照：入口、TUI、tool use、approval、shell、resume、diff、release、docs。
   - 只保留会影响真实用户使用的差距，目标是核心差距低于 5%。

## 当前判断

DeepSeekCode 现在已经是一个可以实际使用的 code agent CLI，尤其适合在本仓库继续 dogfood。
但它还不是“可以公开宣称等同 Claude Code CLI / Codex CLI”的成熟产品。

最准确的公开表述是：

> DeepSeekCode is usable today for dogfooding and repository work, with a full-screen TUI, durable runtime, permissioned tools, release binaries, cross-platform entrypoint smoke, a 100-run online dogfood release gate, initial external disposable-repo write-fixture evidence, real hosted GitHub workflow evidence, and a committed real model-backed README demo SVG. The remaining work is hosted IDE evidence, Windows/service proof, optional richer demo media, and public package-channel publishing.
