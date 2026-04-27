# PR / CI 集成设计

最后更新：`2026-04-27`
状态：`spec` (未实现)
关联 Phase：8（高级能力）

## 背景

`DeepseekCode` 完成 P0–P4 后已具备稳定的本地 agent 闭环：交互式审批 (P3)、可配 observation 类型与裁剪 (P4)、patch 多文件支持与失败重试 (P1)、双协议 tool use (P2)。Phase 8 第一项是 PR / CI 集成 —— 把这个闭环延伸到 GitHub 工作流。

## 目标

为 GitHub PR 提供三种交互入口：
- **`dscode pr review <pr>`** —— 拉 PR diff，跑只读 planner，输出结构化 markdown 报告
- **`dscode pr fix <pr>`** —— 抓首个失败 CI job 的日志，本地复现并迭代修复
- **`dscode pr patch <pr>`** —— 根据 PR 上下文提出补充改动，输出 diff 或可选自动 commit

三命令共享认证、PR 上下文获取、agent loop 调用路径。

## 非目标（v1）

- 自动 push 到 PR 分支（`--commit --push` 推迟到 v2）
- inline review comments（`gh api review` 推迟到 v2）
- GitLab / Gitea / Bitbucket（GitHub-only，via `gh` CLI）
- 多轮 attempt（`--max-attempts` 推迟到 v2；v1 用扩展步预算）
- PR 会话恢复（`pr` 命令不写 SessionSnapshot）
- 自动 checkout PR 分支（用户自行 `gh pr checkout` 后再调用）

## 架构

### 模块边界

```
src/
├── integrations/             # 新增顶级目录
│   ├── mod.rs
│   └── github.rs             # gh CLI 封装：fetch_pr, fetch_first_failed_job, post_pr_comment
├── util/                     # 新增顶级目录
│   ├── mod.rs
│   └── json.rs               # 从 model/deepseek.rs 提出的共享 JSON parser
├── cli/commands/
│   └── pr.rs                 # 三个 sub-action 的入口
└── core/
    └── loop_runtime.rs       # AgentLoop 加 run_with_steps(ctx, steps)
```

### 数据契约

```rust
// src/integrations/github.rs

pub enum PrRef {
    Number(u64),                       // 当前 repo 内 PR 编号
    Qualified { repo: String, number: u64 }, // "owner/repo#N" 或 URL
}

pub struct PrContext {
    pub number: u64,
    pub repo: String,                  // "owner/repo"
    pub title: String,
    pub branch: String,                // headRefName
    pub base_branch: String,           // baseRefName
    pub diff: String,                  // gh pr diff 全文
    pub changed_files: Vec<String>,
}

pub struct CiFailure {
    pub run_id: u64,
    pub job_name: String,
    pub job_id: u64,
    pub log_tail: String,              // 抓取阶段就限到 ~200 行
    pub failed_step: Option<String>,
}
```

公开 API：

```rust
pub fn parse_pr_ref(input: &str) -> AppResult<PrRef>;
pub fn ensure_gh_auth() -> AppResult<()>;
pub fn fetch_pr(reference: &PrRef) -> AppResult<PrContext>;
pub fn fetch_first_failed_job(
    pr: &PrContext,
    job_filter: Option<&str>,
) -> AppResult<Option<CiFailure>>;
pub fn post_pr_comment(repo: &str, number: u64, body: &str) -> AppResult<()>;
```

`gh` 调用映射：

| 操作 | `gh` 命令 |
|---|---|
| Auth 检查 | `gh auth status` |
| PR 元数据 | `gh pr view <ref> --json number,title,headRefName,baseRefName,headRepository,files` |
| PR diff | `gh pr diff <ref>` |
| 失败 check 列表 | `gh pr checks <ref> --json name,state,link` |
| 解析 run_id | 从 `link` 字段正则提取 `/runs/(\d+)` |
| Job 列表 | `gh run view <run_id> --json jobs` |
| 失败日志（尾部） | `gh run view --job <job_id> --log-failed \| tail -n 200` |
| 评论 | `gh pr comment <ref> --body-file <tmp_path>` |

`--body-file` 走临时文件而非 stdin pipe（更简单，避免 `Stdio::piped()` 模板代码）。

### 错误分类（复用 P3 `AppErrorKind`）

| 场景 | 分类 |
|---|---|
| `gh` 不在 PATH | `app_error("gh CLI not found; install from https://cli.github.com/")` |
| `gh auth status` 失败 | `policy_denied("gh not authenticated; run \`gh auth login\`")` |
| PR 不存在 / 无权访问 | `app_error("PR <ref> not found or access denied")` |
| 网络故障 / 超时 | `tool_failure(stderr)` |
| JSON 解析失败 | `tool_failure("could not parse gh JSON output: <detail>")` |
| 用户拒绝交互 prompt | `policy_denied("...")`（已由 P3 处理） |

### 命令面

```
dscode pr review <pr>                 # → stdout markdown
dscode pr review <pr> --post          # → gh pr comment
dscode pr review <pr> --out FILE.md   # → 本地文件

dscode pr fix <pr>                    # 自动选首个失败 job
dscode pr fix <pr> --job <name>       # 指定 job

dscode pr patch <pr>                  # → 仅本地修改 + git_diff 输出
dscode pr patch <pr> --commit         # → git commit（不 push），需干净工作区
```

`<pr>` 接受三种形式：纯编号 / `owner/repo#N` / `https://github.com/owner/repo/pull/N`。

退出码约定：

| Code | 含义 |
|---|---|
| 0 | 成功 |
| 1 | 内部错误（gh 调用失败 / planner panic） |
| 2 | 用户拒绝（confirm 返回 false / branch 错位 / 工作区不干净） |
| 3 | `gh` 未安装或未认证 |

## 流程：三个子命令

### `dscode pr review`

```
1. parse_pr_ref + ensure_gh_auth
2. fetch_pr() → PrContext
3. 构造任务文本：
   "Review pull request #<n> '<title>'. Highlight correctness risks,
    security concerns, and style violations. Output a markdown report."
4. 注入两条预填 Observation：
   - kind=Diff,    summary=PrContext.diff (走 summarize_for_kind 自动裁剪)
   - kind=Listing, summary=PrContext.changed_files.join("\n")
5. AgentLoop::run_with_steps(ctx, 4)，工具白名单 = {read_file, search_text, git_diff}
6. ModelResponse.message → 三路输出：
   - 默认 println!
   - --post → post_pr_comment(repo, number, body)
   - --out  → fs::write(path, body)
7. 退出 0
```

工具白名单确保 review 命令绝不修改文件系统。

### `dscode pr fix`

```
1. parse_pr_ref + ensure_gh_auth
2. fetch_pr() → PrContext
3. require_on_branch(PrContext.branch)：
   - 当前 git branch == PrContext.branch → 继续
   - 否则 policy_denied("switch to PR branch first: git checkout <X>")
4. fetch_first_failed_job(pr, job_filter)：
   - None → 提示 "no failed CI jobs"，退出 0
   - Some(failure) → 继续
5. 构造任务文本（含失败日志尾部）：
   "CI job '<job_name>' (run #<run_id>) failed at step '<failed_step>'.
    Reproduce locally, fix the root cause, and rerun the failing test.
    Failed log tail:\n\n<log_tail>"
6. 注入预填 Observation：
   - kind=ShellOutput, summary=log_tail
7. AgentLoop::run_with_steps(ctx, 12)，工具全开
   - apply_patch / run_shell 都走 P3 confirm（除非 env auto-approve）
8. 完成后调用 git_diff，输出到 stdout
9. 退出 0（无论 fix 成功与否，决定权交回用户）
```

12 步预算 = 3 个完整 4 步循环量级，足够 read → patch → run_shell → re-read → re-patch → re-run 节奏。

### `dscode pr patch`

```
1. parse_pr_ref + ensure_gh_auth
2. fetch_pr() → PrContext
3. require_on_branch(PrContext.branch)
4. 仅当 --commit 时：require_clean_worktree()（git status --porcelain 为空）
5. 构造任务文本：
   "Address review feedback or apply the requested change in PR #<n>.
    PR diff is the current head; propose minimal additional changes."
6. 注入 Observation：
   - kind=Diff, summary=PrContext.diff
7. AgentLoop::run_with_steps(ctx, 4)，工具全开
8. 完成后输出 git diff HEAD
9. --commit：
   - git add -A
   - git commit -m "dscode: fix PR #<n>"
   - 不 push
10. 退出 0
```

## 上下文管理

复用 P4 已有：
- `summarize_for_kind` 处理 PR diff（hunk header 保留）+ CI log（取尾部）
- `compact_observations` 自动 supersede 同类旧观察
- `BTreeSet<&str>` 可用工具集 + 失败/成功观察分类

新增的预填 Observation 走相同管道，无特殊处理。Tool 白名单通过 `ExecutionPolicy` 在 review 路径强制，不引入新机制。

## 测试策略

### 单测（无外部依赖）
- `parse_pr_ref` —— 三种输入形式 + 错误体
- `parse_pr_view_json` / `parse_checks_json` / `parse_run_view_json` —— 喂样本 JSON 校验解析
- `extract_run_id_from_link` —— 正则路径
- `parse_log_tail` —— 200 行截尾
- AgentLoop `run_with_steps(_, 12)` —— 与 `run(_, 4)` 行为隔离测试
- `pr.rs` 任务文本构造 helper —— 不调用 agent loop
- `--post` / `--out` 路由分发

### 集成测（mock `gh`）
- 用环境变量 `DSCODE_GH_BIN` 覆盖 `gh` 二进制路径
- 在 `tests/fixtures/` 下放置 stub shell 脚本（按子命令 dispatch）
- CI 友好（无网络依赖）

### 手测（真 PR）
- 在 `willamhou/DeepseekCode` 自身的 PR 上执行 review / fix / patch 各一次
- doctor 扩展验证 `gh` 检测路径

## 切片：6 个 PR、~5.5 天

| PR | 工作 | 估时 | Land 条件 |
|---|---|---|---|
| M1 | `gh` 集成基础（github.rs + util/json.rs 抽取） | 1d | cargo test 通过；CLI 不变 |
| M2 | `dscode pr review`（含 stdout/post/out 三路） | 1d | 真 PR 手测打出 markdown 报告 |
| M3 | AgentLoop `run_with_steps` 重构 | 0.5d | 现有 72 项不变；新增 1 项 |
| M4 | `dscode pr fix`（含 `--job` 与 12 步预算） | 1.5d | stub 失败日志手测能跑完一轮 |
| M5 | `dscode pr patch`（含 `--commit` 与干净工作区检查） | 1d | 真 PR 手测一次 |
| M6 | 文档 + doctor 加 `gh` 检查 | 0.5d | README 示例通过；doctor 显示 gh 状态 |

阶段化 land：M1+M2 (review only) → M3+M4 (fix loop) → M5+M6 (patch + docs)。

## 风险

| 风险 | 缓解 |
|---|---|
| `gh` 版本兼容（≥2.40 才有完整 `--json` 支持） | doctor 检查 `gh --version` 并提示升级 |
| CI 日志超大 | 抓取阶段强制 200 行尾 + 二次走 `summarize_for_kind` |
| PR diff 超大（数千行） | 复用 P4 `ObservationKind::Diff` 的 hunk header 保留 |
| Fork PR 权限 | `gh pr comment` 自动用 BASE repo 权限；无需特殊处理 |
| Token 耗尽 | step 预算 12 触底自动停；用户可重跑 `pr fix` |
| `gh auth status` 慢（远程 ping） | 仅在三命令入口调一次，后续操作不重复 |

## 待解项

无。所有交互式问题均已在 brainstorming 中收敛：
- gh CLI（vs REST / 抽象层）
- 子命令组（vs flag / 三独立命令）
- 自动选首个 failed job + `--job` 覆盖
- 默认本地 + `--post`/`--commit` 旗标
- 不写 SessionSnapshot；硬拒分支错位；`--commit` 强干净工作区
- v1 不实现 `--max-attempts`（用 step 预算扩展替代）

## 后续（v2 候选，不在本次 spec）

- `--max-attempts` 多轮 fix（每轮独立 4 步预算）
- `--push` 自动推到 PR 分支
- `--inline` inline review comments（`gh api`）
- GitLab / Gitea provider（提取 `PrSource` trait）
- PR session resume（`SessionSnapshot` 加 `pr_ref` 字段）
- 大 PR 的自适应步预算
