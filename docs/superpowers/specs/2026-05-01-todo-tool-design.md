# TodoTool — Phase 10a 设计

最后更新：`2026-05-01`
状态：`spec` (未实现)
关联 Phase：10a (LLM-driven planner — Claude Code 风格 todos-as-tool)

## 背景

Phase 9 系列把 `dscode` 升级成可流式 SSE 输出的本地代码 agent，但 agent loop 仍是
"LLM 选下一个工具 → dscode 执行 → 回填 observation" 的纯反应式循环。**没有显式的多
步规划阶段**。简单任务（"replace X with Y"）走得通，但项目级（"实现 sub-agent 派发
+ failure retry + 项目模板"）的 50+ 步迭代会把 LLM 拖进局部最优，反复调 list_files /
read_file 而不知道整体进度。

Claude Code 的解法是 `TodoWrite` 工具：todos 是 *工具* 不是 *阶段*，LLM 自己决定
何时建/改/完成 task list。dscode 复用同样心智 + 复用 Phase 9 的 streaming + transcript
基础设施。

## 目标

- 给 dscode 加一个 `todo_write` 工具，LLM 可主动维护 task list
- todo 数据 session-scoped：与 transcript 同生死，可 `/save`/`/load`/`/clear`
- 当前 todos 每轮注入到 user prompt 让 LLM 看见自己的进度
- system prompt 强 nudge：3+ 步任务必用 todo_write
- 渲染复用 Phase 9b 的 `paint_tool_result` —— 黄色 tool 调用行 + 绿色 ✓ + 缩进 list body
- 零新依赖

## 非目标 (Phase 10a)

- Sub-agent 派发（Phase 10b）
- 跨进程 / workspace 级 todos 持久化（Phase 10c 候选）
- LLM 自我 replan / 失败自动回路（Phase 10c）
- `dscode init <template>` 项目模板（Phase 10c）
- cargo / npm 专用工具（Phase 10c）
- todos 字段拓展（notes / due / priority — YAGNI）

## 锁定的设计决策

brainstorm 五轮 Q&A 收敛：

1. **风格**：Claude Code 风格（todos 是工具不是阶段）— vs Codex（hard plan 阶段）/ Aider（双模型）
2. **工具接口**：`整体替换`，一次调用 rewrite 整个 list — vs 离散 add/update/remove ops
3. **生命周期**：`session-scoped` —— `dscode run` 任务结束消失；`dscode chat` 跨轮保留，与 transcript 同生死
4. **Schema**：三字段 `content` (imperative) / `activeForm` (present continuous) / `status` (pending|in_progress|completed)
5. **显示**：`paint_tool_result` body 自动渲染（复用现有 streaming UI 管线）
6. **Prompt 注入**：user prompt 加 `Todos:` block（与 `Observations:` 平级）— vs system prompt 注入（破坏 prefix cache）
7. **System nudge**：强风格 5-6 句静态文本（vs 软风格被忽略 / 关键词检测脆弱）
8. **持久化**：SessionSnapshot schema bump v1 → v2，嵌入 `todos` 字段

## 架构

### 模块边界

```
src/
├── core/
│   ├── todos.rs                # 新：Todo / TodoList / TodoStatus 数据类型
│   ├── mod.rs                  # 改：加 pub mod todos;
│   ├── loop_runtime.rs         # 改:AgentLoop 拥有 Rc<RefCell<TodoList>>，注入 ModelRequest
│   └── observations.rs         # 改:summarize_for_kind 处理 Todos kind（passthrough）
├── tools/
│   ├── todo.rs                 # 新：TodoWriteTool impl Tool
│   ├── mod.rs                  # 改：加 pub mod todo;
│   └── registry.rs             # 改：default_registry_with_todos(Rc<RefCell<TodoList>>)
├── model/
│   ├── protocol.rs             # 改:ModelRequest.todos; ObservationKind::Todos
│   └── deepseek.rs             # 改：build_user_prompt 加 Todos block; TOOL_SPECS 加 todo_write; system prompt nudge
└── repl/
    ├── repl.rs                 # 改:Repl 拥有 Rc<RefCell<TodoList>>; /clear 清空
    ├── slash.rs                # 改：/todos 命令
    └── session.rs              # 改:Schema v2 + v1→v2 迁移
```

13 文件，3 新（`core/todos.rs`、`tools/todo.rs`、`docs/todos.md`）+ 10 改。

### 数据契约

#### `core/todos.rs`（新）

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

impl TodoStatus {
    /// "pending" | "in_progress" | "completed" → variant
    pub fn from_str(s: &str) -> Option<Self>;

    /// variant → "pending" | "in_progress" | "completed"
    pub fn label(&self) -> &'static str;
}

#[derive(Debug, Clone)]
pub struct Todo {
    pub content: String,        // imperative form, e.g. "Run tests"
    pub active_form: String,    // present continuous, e.g. "Running tests"
    pub status: TodoStatus,
}

#[derive(Debug, Clone, Default)]
pub struct TodoList {
    pub items: Vec<Todo>,
}

impl TodoList {
    pub fn replace(&mut self, items: Vec<Todo>);

    /// "- [pending] Run tests\n- [in_progress] Add feature\n…"
    /// 用于 user prompt 的 Todos block
    /// 空 list 返回 ""
    pub fn render_for_prompt(&self) -> String;

    /// "3 items written (1 in_progress, 2 pending):\n  [in_progress] Adding…"
    /// 用于 TodoWriteTool 的 ToolOutput.summary 与 /todos 命令显示
    /// in_progress 用 active_form，其他用 content
    pub fn render_for_display(&self) -> String;

    /// 给 ModelRequest 复制
    pub fn snapshot(&self) -> Vec<Todo>;

    pub fn is_empty(&self) -> bool;
}
```

#### `tools/todo.rs`（新）

```rust
pub struct TodoWriteTool {
    pub list: Rc<RefCell<TodoList>>,
}

impl Tool for TodoWriteTool {
    fn name(&self) -> &'static str { "todo_write" }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        // 1. items = input.args.get("items").ok_or(...)? — 必须是 JSON 字符串
        // 2. parse_root_array(items)? → Vec<JsonValue>
        // 3. 校验每项：{ content: string, activeForm: string, status: enum }
        // 4. 上限校验：items.len() <= 50
        // 5. self.list.borrow_mut().replace(parsed)
        // 6. ToolOutput { summary: list.render_for_display() }
    }
}
```

错误分类：
- `items` 缺失 / 非字符串 / 非 JSON 数组 → `tool_failure("todo_write requires items as a JSON-encoded array")`
- 单 todo 缺 `content` / `activeForm` / `status` → `tool_failure("todo missing required field <name>")`
- `status` 不在合法值集合 → `tool_failure("todo status must be pending|in_progress|completed")`
- `>50 items` → `tool_failure("too many todos (max 50)")`

**不强校验** "exactly one in_progress" — 仅 system prompt 引导，dscode 不做 LLM 老师。

#### Tool spec for LLM（OpenAI + Anthropic schema）

由于 `ToolInput.args: BTreeMap<String, String>` 不支持嵌套数组，items 以 JSON 字符串
传递（与 `apply_patch.patch` 同模式）。

```json
{
  "name": "todo_write",
  "description": "Replace the entire todo list with a new set of items. Use proactively for tasks with 3+ steps; mark exactly one item as in_progress at a time.",
  "parameters": {
    "type": "object",
    "properties": {
      "items": {
        "type": "string",
        "description": "JSON array of objects with fields {content: string, activeForm: string, status: \"pending\"|\"in_progress\"|\"completed\"}. content is imperative form (e.g. \"Run tests\"); activeForm is present continuous (e.g. \"Running tests\")."
      }
    },
    "required": ["items"],
    "additionalProperties": false
  }
}
```

加到 `src/model/deepseek.rs::TOOL_SPECS` 数组末尾。

#### `model/protocol.rs` 改动

```rust
pub struct ModelRequest {
    // 现有字段不变
    pub system_prompt: String,
    pub task: String,
    pub profile_name: String,
    pub profile_hints: Vec<String>,
    pub primary_file: Option<String>,
    pub suggested_test_command: Option<String>,
    pub available_tools: Vec<String>,
    pub observations: Vec<Observation>,
    // 新增
    pub todos: Vec<Todo>,
}

pub enum ObservationKind {
    FileExcerpt,
    Listing,
    SearchResults,
    Patch,
    Diff,
    ShellOutput,
    Other,
    Todos,                   // 新增第 8 个
}

impl ObservationKind {
    pub fn from_tool_name(name: &str) -> Self {
        match name {
            "todo_write" => Self::Todos,    // 新增
            // 现有映射不变
            _ => Self::Other,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Todos => "todos",          // 新增
            // 现有
        }
    }
}
```

#### `core/observations.rs` 改动

`summarize_for_kind` 加 `Todos` arm — passthrough（不裁剪），让 transcript 看到完整 list。`compact_observations` 已经自动 supersede 老 `Todos` observation 为 stub，无需改动。

#### `core/loop_runtime.rs` 改动

```rust
pub struct AgentLoopOptions {
    pub steps: usize,
    pub initial_observations: Vec<Observation>,
    pub todos: Rc<RefCell<TodoList>>,    // 新增
}

impl Default for AgentLoopOptions {
    fn default() -> Self {
        Self {
            steps: 4,
            initial_observations: Vec::new(),
            todos: Rc::new(RefCell::new(TodoList::default())),
        }
    }
}
```

`AgentLoop::run_with` 内部：
- 用 `default_registry_with_todos(options.todos.clone())` 替换 `default_registry()`
- 每轮 build `ModelRequest` 时调 `options.todos.borrow().snapshot()` 复制当前 todos
- 老 `default_registry()` 简化为 `default_registry_with_todos(Rc::new(RefCell::new(TodoList::default())))`，向后兼容

`build_system_prompt` 末尾追加常量 nudge：

```rust
const TODO_NUDGE: &str = r#"

You have access to a todo_write tool. Use it proactively when the request:
- involves three or more distinct steps,
- spans multiple files or non-trivial refactoring,
- requires running tests or shell commands as part of completion.

Each todo has fields: content (imperative, e.g. "Run tests"), activeForm (present continuous, e.g. "Running tests"), status ("pending" | "in_progress" | "completed").

Mark exactly one todo as in_progress at a time. Update the list (mark completed, add discovered tasks) before moving to the next step. Skip todo_write only for trivial single-step requests."#;
```

skill 的 `system_append` 仍按现规则插在 nudge 之前。

#### `model/deepseek.rs::build_user_prompt` 改动

在 `Available tools:` 行之后、`Observations:` block 之前注入：

```rust
if !input.todos.is_empty() {
    prompt.push_str("Todos:\n");
    for todo in &input.todos {
        prompt.push_str(&format!(
            "- [{}] {}\n",
            todo.status.label(),
            todo.content,
        ));
    }
}
```

**规则**：todos 为空时不注入 `Todos:` 段（不写 "none"）。与现有 `primary_file` /
`suggested_test_command` 同样"有就写、没就不写"模式一致。

prompt 里 todo 用 `[status] content` 简短格式（不带 `active_form`）— LLM 不需要看
active_form，那是给 UI 用的。

#### `repl/session.rs` schema v2

```rust
const SCHEMA_VERSION: u64 = 2;

pub struct SessionSnapshot {
    pub name: String,
    pub saved_at: String,
    pub skill: Option<String>,
    pub budget: usize,
    pub transcript: Vec<TranscriptTurn>,
    pub tokens_prompt: u64,
    pub tokens_completion: u64,
    pub todos: Vec<Todo>,                // 新增
}
```

`/save` 写：

```json
{
  "version": 2,
  "name": "fix-pr-42",
  "saved_at": "epoch+1746...",
  "skill": "pr-review",
  "budget": 30,
  "transcript": [...],
  "todos": [
    {"content": "Add TodoTool", "activeForm": "Adding TodoTool", "status": "in_progress"},
    {"content": "Wire to AgentLoop", "activeForm": "Wiring to AgentLoop", "status": "pending"}
  ],
  "tokens": {"prompt": 12345, "completion": 6789}
}
```

`/load` 行为：
- v2 文件：直读 `todos` 数组
- v1 文件：迁移 — 注入 `todos: vec![]`，其他字段不变；下次 `/save` 才升级到 v2
- v3+：拒绝并保留当前状态（与 v1 时同样保守）

#### `repl/repl.rs` 改动

```rust
pub struct Repl {
    pub config: AppConfig,
    pub transcript: Transcript,
    pub budget: usize,
    pub skill: Option<String>,
    pub tokens_prompt: u64,
    pub tokens_completion: u64,
    pub todos: Rc<RefCell<TodoList>>,    // 新增
}
```

`Repl::handle_line` 调 `AgentLoop::run_with` 时，把 `todos.clone()` 传进 `AgentLoopOptions`。

`/clear` 同时重置：transcript / tokens / **todos**。budget 与 skill 不动（与现有规则一致）。

`/save` / `/load` 通过 `SessionSnapshot` 自动持久化 todos。

#### `repl/slash.rs::/todos`

只读检视命令，不接受参数：

```
> /todos
no todos yet

> /todos
todos (3 items, 1 in progress):
  [completed]   Read existing tools/registry.rs
  [in_progress] Adding TodoTool
  [pending]     Wire AgentLoop
```

写到 stderr（与 `> ` prompt 同 sink）。`render_for_display` 复用 TodoWriteTool 的同款渲染。

要清空 todos 走 `/clear`（同时清 transcript），不另设 `/todos clear`。

### 渲染管线（端到端）

LLM 调一次 `todo_write`，TTY 用户屏幕看到：

```
─── step 2 ───
deepseek-v3.2 plans the work first.
🛠 todo_write(items=[{"content":"Add TodoTool",…)
✓ todo_write [todos]
  3 items written (1 in_progress, 2 pending):
    [in_progress] Adding TodoTool
    [pending]     Wire TodoList into AgentLoop
    [pending]     Add /todos slash command
```

参数 `items` 通过 `abbreviate_for_inline` 自动截到 80 char + `…`。`paint_tool_result(Ok, "todo_write", "todos", &body)` 输出绿色 `✓` + `[todos]` 标签 + 缩进 body。

非 TTY（`dscode run > out.txt`）：

```
─── step 2 ───
deepseek-v3.2 plans the work first.
> todo_write(items=[{"content":"Add TodoTool",…)
OK: todo_write [todos]
  3 items written (1 in_progress, 2 pending):
    [in_progress] Adding TodoTool
    [pending]     Wire TodoList into AgentLoop
    [pending]     Add /todos slash command
```

复用 Phase 9b 的 `is_terminal()` ANSI 自动降级。

## 切片：8 PR、~4-5 天

| PR | 工作 | 估时 | 测试增量 | Land 条件 |
|----|------|------|----------|-----------|
| M1 | `core/todos.rs` 数据类型 + render + validate | 0.5d | +6 | 221 → 227；零 warnings |
| M2 | `tools/todo.rs` TodoWriteTool + `default_registry_with_todos` | 0.5d | +5 | 227 → 232；4 错误路径覆盖 |
| M3 | `model/protocol.rs` ModelRequest.todos + ObservationKind::Todos + observations passthrough | 0.5d | +2 | 232 → 234；行为零变化 |
| M4 | `model/deepseek.rs` TOOL_SPECS + build_user_prompt Todos block + system prompt nudge | 0.5d | +3 | 234 → 237；prompt 单测覆盖空/非空 |
| M5 | `core/loop_runtime.rs` AgentLoopOptions.todos + 注入 ModelRequest | 0.5d | +2 | 237 → 239；端到端 TodoWrite 调一次更新 list |
| M6 | `repl/session.rs` schema v1 → v2 + migration | 0.5d | +3 | 239 → 242；v2 round-trip + v1→v2 自动 migrate + 未知版本拒绝 |
| M7 | `repl/repl.rs` + `repl/slash.rs` /todos 命令、/clear 同步重置、/save/load 走 v2 | 0.5d | +3 | 242 → 245；3 slash 单测 |
| M8 | `docs/todos.md` + `docs/roadmap.md` Phase 10a 标完成 + dogfood 验证 | 0.5d | 0 | 245 持平；手测 LLM 主动调 todo_write |

总：8 PR、+24 测试（221 → 245）、~4-5 天。

阶段化 land：
- **M1 + M2 + M3**：基础类型 + 工具，零行为变化（用户看不见）
- **M4 + M5**：prompt 注入 + AgentLoop 接管，LLM 开始能调
- **M6 + M7**：REPL 集成、可持久化
- **M8**：dogfood + 文档收尾

## 测试策略

### 单测（无外部依赖，+24）

`core/todos.rs` × 6：
- TodoStatus::from_str 三个合法值
- TodoStatus::from_str 非法值返 None
- TodoList::replace 替换整个 list
- TodoList::render_for_prompt 输出格式
- TodoList::render_for_display 混合状态格式（in_progress 用 active_form）
- TodoList::is_empty 状态切换

`tools/todo.rs` × 5：
- execute 成功路径
- execute 失败:items 字段缺失
- execute 失败：items 不是合法 JSON 数组
- execute 失败：todo 缺必填字段
- execute 失败：>50 items

`model/protocol.rs` + `core/observations.rs` × 2：
- ObservationKind::from_tool_name("todo_write") → Todos
- summarize_for_kind 对 Todos 不裁剪

`model/deepseek.rs` × 3：
- build_user_prompt 空 todos → 不输出 Todos 段
- build_user_prompt 非空 todos → 输出格式正确
- build_system_prompt 注入了 nudge 文本

`core/loop_runtime.rs` × 2：
- 端到端：单步循环里 LLM 模拟调 todo_write 后 list 状态被更新
- AgentLoopOptions::default 给空 TodoList

`repl/session.rs` × 3：
- v2 round-trip
- v1 → v2 自动 migrate（todos 为空 vec）
- v3+ 拒绝并保留 state

`repl/slash.rs` × 3：
- /todos 空 list 输出 "no todos yet"
- /todos 含 items 输出格式正确
- /clear 同时重置 transcript + todos + tokens

### 集成 / 手测（M8）

设 `DEEPSEEK_API_KEY` 跑：
```bash
dscode chat
> 实现一个 Phase 10b 的 sub-agent 派发逻辑，分四步走，先列 todos 再开始
[期望:LLM 看到强 nudge + 多步请求 → 主动调 todo_write]
> /todos                    # 看见当前 list
> /save phase-10b-plan      # 落盘 v2
> /load phase-10b-plan      # 还原
> /clear                    # 清空 todos + transcript
> /todos                    # → "no todos yet"
> /quit
```

- 检查 `dscode run > out.txt`：piped 时 ANSI 自动降级，list body 仍正确缩进
- 验证 v1 老 session：手写或用 git history 找一份 v1 文件，`/load` 后 `/todos` 应为空、其他字段完整保留

## 错误分类

延续 P3 `AppErrorKind`：

| 场景 | 分类 |
|------|------|
| `items` 字段缺失 | `tool_failure("todo_write requires items as a JSON-encoded array")` |
| `items` 非合法 JSON | `tool_failure("malformed todo items JSON: <detail>")` |
| 单 todo 缺字段 | `tool_failure("todo missing required field <name>")` |
| `status` 非法值 | `tool_failure("todo status must be pending|in_progress|completed")` |
| `>50 items` | `tool_failure("too many todos (max 50)")` |
| Session schema 未知版本 | `app_error("session schema version <N> not supported")` |
| Session 文件读写失败 | 现有 `session.rs` 错误路径不变 |

错误透过 `paint_tool_result(Failed, ...)` 红 ✗ 渲染，与其他工具失败一致。

## 风险

| 风险 | 缓解 |
|------|------|
| LLM 不愿意调 `todo_write` 即使有强 nudge | M4 借用 Claude Code 验证过的措辞；M8 dogfood 多个模型实测；不达标时迭代 nudge 文本 |
| `Rc<RefCell<TodoList>>` 在某些场景 borrow check 运行时炸 | dscode 单线程；borrow 都在 TodoWriteTool::execute 内立即释放；不存在跨 await/spawn |
| schema v1 → v2 migration 破坏现有 .dscode/sessions 老数据 | M6 单测覆盖；version 严格匹配；v1 加载只在内存注入 todos: []，下次 /save 才升 v2 |
| `items` JSON 字符串里特殊字符（引号 / 换行）→ tool args 转义出错 | Phase 9b C2 fix 后 `\u/\b/\f` 都正确解码；嵌套引号靠 LLM 端转义 |
| 强 nudge 让 LLM 在简单任务上滥用 todo_write | nudge 末尾明确 "Skip todo_write only for trivial single-step requests"；M8 dogfood 验证 |
| ToolOutput.summary 极长（50 todos）影响 transcript context | summarize_for_kind 对 Todos passthrough；compact_observations 自动 supersede 老 Todos observation；不累积 |
| LLM 把 active_form 写错时态（"Run tests" 而不是 "Running tests"） | 不强校验 — 软问题，UI 显示稍微 awkward；后续 Phase 10c 可以加 lint 时迭代 |

## 待解项

无。所有交互式问题在 brainstorming 中收敛：
- Q1: Claude Code 风格 (vs Codex / Aider / 软引导)
- Q2: 整体替换 tool surface
- Q3: session-scoped 生命周期
- Q4: 三字段 schema (content/activeForm/status)
- Q5: paint_tool_result body 自动渲染
- Q6: user prompt Todos block (vs system prompt 注入)
- Q7: 强风格 system nudge
- Q8: SessionSnapshot v1→v2 嵌入持久化

## 后续 (Phase 10b/10c 候选)

明确为 10a 之外，不写入此 spec：
- **Phase 10b**: Sub-agent 派发（让 LLM 通过 `dispatch_subagent` 工具派子 agent，子 agent 独立 budget + 独立 transcript）
- **Phase 10c**: 跨进程 todos 持久化、`/todos pin` workspace 锁、`dscode init <template>` 项目模板、cargo / npm 专用工具、LLM 自我 replan 失败回路
- TodoTool 字段拓展（notes / due / priority — YAGNI 不做）
