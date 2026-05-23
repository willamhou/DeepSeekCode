# Skill 与 Language Profile 设计

## 为什么要分成两类

`Profile` 与 `Skill` 解决的是不同问题：

- `Language Profile` 关注仓库和语言环境
- `Skill` 关注当前任务的执行策略

这样能避免：

- 把语言逻辑硬编码进 agent prompt
- 把任务逻辑和运行时绑死
- 为了扩展一个语言或一个任务就修改核心代码

## Language Profile

Profile 主要定义：

- 文件优先级
- 忽略规则
- 常见测试命令
- 常见 lint/build 命令
- 对模型的补充提示

Rust 结构建议：

```rust
#[derive(Debug, Clone, serde::Deserialize)]
pub struct LanguageProfile {
    pub name: String,
    #[serde(default)]
    pub file_priority: Vec<String>,
    #[serde(default)]
    pub ignore_patterns: Vec<String>,
    #[serde(default)]
    pub test_commands: Vec<String>,
    #[serde(default)]
    pub lint_commands: Vec<String>,
    #[serde(default)]
    pub build_commands: Vec<String>,
    #[serde(default)]
    pub hints: Vec<String>,
}
```

示例：

```toml
name = "rust"
file_priority = ["Cargo.toml", "src/main.rs", "src/lib.rs", "tests/"]
ignore_patterns = ["target/", ".git/"]
test_commands = ["cargo test"]
lint_commands = ["cargo clippy --all-targets --all-features"]
build_commands = ["cargo build"]
hints = ["Prefer minimal compile-safe changes."]
```

## Skill

Skill 主要定义：

- 任务描述
- 可用工具
- 追加 system prompt
- 建议步骤
- 写入与 shell 审批策略
- shell allowlist

Rust 结构建议：

```rust
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SkillSpec {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub system_append: String,
    #[serde(default)]
    pub suggested_steps: Vec<String>,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub initial_todos: Vec<TodoSeed>,
    #[serde(default)]
    pub references: Vec<String>,
    pub policy: SkillPolicy,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TodoSeed {
    pub content: String,
    pub active_form: String,
    #[serde(default = "default_pending")]
    pub status: TodoStatus,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SkillPolicy {
    #[serde(default = "default_true")]
    pub require_write_confirmation: bool,
    #[serde(default = "default_true")]
    pub require_shell_confirmation: bool,
    #[serde(default)]
    pub shell_allowlist: Vec<String>,
}
```

示例：

```toml
name = "fix-tests"
description = "Focus on reproducing and fixing failing tests with minimal edits"
allowed_tools = ["list_files", "read_file", "search_text", "apply_patch", "run_shell", "git_diff"]
triggers = ["fix tests", "failing tests", "red tests"]
references = ["README.md", "Cargo.toml"]
system_append = """
Reproduce failures first. Prefer the smallest safe code change.
Rerun only relevant tests before broad test suites.
"""
suggested_steps = [
  "Find the test command",
  "Reproduce the failure",
  "Inspect the smallest relevant code path",
  "Apply a minimal patch",
  "Rerun the relevant tests"
]

[[initial_todos]]
content = "Reproduce the failure"
active_form = "Reproducing the failure"
status = "in_progress"

[[initial_todos]]
content = "Apply the minimal patch"
active_form = "Applying the minimal patch"
status = "pending"

[policy]
require_write_confirmation = true
require_shell_confirmation = false
shell_allowlist = ["cargo test", "pytest", "pnpm test", "npm test", "go test", "mvn test", "gradle test"]
```

## Discovery 与 Validation

运行时按下面顺序加载 skills，后面的同名 skill 覆盖前面的同名 skill：

1. bundled repo skills：`skills/`，或安装包旁边的 `skills/`
2. user skills：`workspace.user_skills_dir`，默认 `~/.config/dscode/skills`

CLI 提供同一套发现与验证入口：

```bash
deepseek skills list
deepseek skills list --json
deepseek skills validate --strict
deepseek skills validate --json --dir skills
```

`skills validate` 会检查 `.toml` loader 错误、空 `name` / `description` /
`system_append` / `suggested_steps`、空 `allowed_tools`，以及
`allowed_tools` 中不存在的工具名。`--strict` 会把 warning 也作为非零退出，
适合 CI、release gate 或技能库发布前检查。

## 常见 Skill 示例

PR review 只需要读 diff 与上下文，保持 read-only：

```toml
name = "pr-review"
description = "Review a GitHub PR diff and report correctness/security risks"
allowed_tools = ["list_files", "read_file", "search_text", "git_diff"]
system_append = "Review only. Do not modify files. Lead with actionable findings."
suggested_steps = ["Read the diff", "Inspect surrounding context", "Group findings by severity"]
```

Release 检查适合绑定 lint/test/build 和发布状态：

```toml
name = "release-check"
description = "Run release readiness checks before tagging or publishing"
allowed_tools = ["list_files", "read_file", "run_shell", "git_diff", "todo_write"]
system_append = "Run lint, tests, build, and publish-status before declaring release readiness."
suggested_steps = ["Run lint", "Run tests", "Run build", "Run publish-status", "Review diff"]

[policy]
require_write_confirmation = false
require_shell_confirmation = false
shell_allowlist = ["cargo test", "cargo build", "cargo clippy", "deepseek update publish-status"]
```

Security-lite 检查适合聚焦 secrets、unsafe shell、权限面和依赖风险：

```toml
name = "security-lite"
description = "Review a change for obvious secrets, shell, permission, and dependency risks"
allowed_tools = ["list_files", "read_file", "search_text", "git_diff"]
system_append = "Look for leaked secrets, unsafe shell commands, broad permissions, and dependency risk. Report findings only."
suggested_steps = ["Inspect diff", "Search for secrets", "Review shell and permissions", "Summarize risks"]
```

## 推荐的首批 Skills

- `fix-tests`
- `fix-lint`
- `explain-codebase`
- `small-refactor`

这些已经足够支撑第一版常见任务，不需要一开始做开放式插件生态。
