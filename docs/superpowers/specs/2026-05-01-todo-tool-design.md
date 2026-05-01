# TodoTool — Phase 10a 设计

最后更新：`2026-05-01` (rev 2 — 吸收 codex review 反馈)
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

brainstorm 八轮 Q&A 收敛：

1. **风格**：Claude Code 风格（todos 是工具不是阶段）— vs Codex（hard plan 阶段）/ Aider（双模型）
2. **工具接口**：`整体替换`，一次调用 rewrite 整个 list — vs 离散 add/update/remove ops
3. **生命周期**：`session-scoped` —— `dscode run` 任务结束消失；`dscode chat` 跨轮保留，与 transcript 同生死
4. **Schema**：三字段 `content` (imperative) / `activeForm` (present continuous) / `status` (pending|in_progress|completed)
5. **显示**：`paint_tool_result` body 自动渲染（复用现有 streaming UI 管线）
6. **Prompt 注入**：user prompt 加 `Todos:` block（与 `Observations:` 平级）— vs system prompt 注入（破坏 prefix cache）
7. **System nudge**：强风格 5-6 句静态文本（vs 软风格被忽略 / 关键词检测脆弱）
8. **持久化**：SessionSnapshot schema bump v1 → v2，嵌入 `todos` 字段

## 术语澄清

- 本 spec **只动 `src/repl/session.rs`** —— REPL 用的 JSON SessionSnapshot
- `src/core/session.rs` 是另一套 legacy TOML snapshot（与 `dscode resume` 配合），**与本 spec 完全无关**，不动
- 文档里"session"/"SessionSnapshot" 一律指 `repl/session.rs`

## 架构

### 模块边界

```
src/
├── core/
│   ├── todos.rs                # 新：Todo / TodoList / TodoStatus 数据类型
│   ├── mod.rs                  # 改：加 pub mod todos;
│   ├── loop_runtime.rs         # 改:AgentLoop 拥有 Rc<RefCell<TodoList>>，注入 ModelRequest
│   └── observations.rs         # 改：KIND_COUNT 7→8、kind_index、summarize_for_kind 加 Todos
├── tools/
│   ├── todo.rs                 # 新：TodoWriteTool impl Tool
│   ├── mod.rs                  # 改：加 pub mod todo;
│   └── registry.rs             # 改：default_registry_with_todos(Rc<RefCell<TodoList>>)
├── model/
│   ├── protocol.rs             # 改:ModelRequest.todos; ObservationKind::Todos
│   └── deepseek.rs             # 改:json_object_to_string_args 处理嵌套值; build_user_prompt 加 Todos block; TOOL_SPECS 加 todo_write; system prompt nudge
├── util/
│   └── json.rs                 # 改：加 pub fn json_value_to_string (writer for nested values)
└── repl/
    ├── repl.rs                 # 改:Repl 拥有 Rc<RefCell<TodoList>>; /clear 清空
    ├── slash.rs                # 改:/todos 命令
    ├── session.rs              # 改:Schema v2 + v1→v2 迁移（in-memory only）
    └── transcript.rs           # 改：render_for_prompt 对 todo_write input 缩略
```

15 文件，3 新（`core/todos.rs`、`tools/todo.rs`、`docs/todos.md`）+ 12 改。

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
    /// 用新 list 完全替换（旧 items 全部丢）
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

    /// 紧凑摘要：用于 transcript replay + summarize_for_kind
    /// "5 todos: 2 completed, 1 in_progress, 2 pending"
    pub fn render_compact_summary(&self) -> String;
}
```

#### **重要：`util/json.rs::json_value_to_string` 新增**（修复 C1）

当前 `json_object_to_string_args`（`src/model/deepseek.rs:1078-1106`）对嵌套
`Object/Array` 直接 `app_error`。LLM **会**发 `"items": [{...}]`（字面数组），
命中错误路径，返回 `app_error` 后整轮 abort（不只是红 ✗），重试也无意义。

修法：在 `util/json.rs` 加一个递归 JSON writer：

```rust
pub fn json_value_to_string(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        JsonValue::Number(n) => n.clone(),
        JsonValue::String(s) => {
            let mut out = String::with_capacity(s.len() + 2);
            out.push('"');
            out.push_str(&json_escape(s));
            out.push('"');
            out
        }
        JsonValue::Array(items) => {
            let mut out = String::from("[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 { out.push(','); }
                out.push_str(&json_value_to_string(item));
            }
            out.push(']');
            out
        }
        JsonValue::Object(map) => {
            let mut out = String::from("{");
            for (i, (k, v)) in map.iter().enumerate() {
                if i > 0 { out.push(','); }
                out.push('"');
                out.push_str(&json_escape(k));
                out.push_str("\":");
                out.push_str(&json_value_to_string(v));
            }
            out.push('}');
            out
        }
    }
}
```

然后修 `json_object_to_string_args`（在 `src/model/deepseek.rs`）：

```rust
JsonValue::Object(_) | JsonValue::Array(_) => {
    // 把嵌套结构 re-serialize 回 JSON 字符串，让 ToolInput.args 仍是 BTreeMap<String, String>
    // 但工具拿到字符串后可以二次 parse。修复 Phase 10a items 数组传输的 codex C1 阻断。
    result.insert(key.clone(), crate::util::json::json_value_to_string(value));
}
```

修复后**全部工具自动受益** —— 未来任何带嵌套参数的工具都不再被 protocol parser 卡死。

#### `tools/todo.rs`（新）

```rust
pub struct TodoWriteTool {
    pub list: Rc<RefCell<TodoList>>,
}

impl Tool for TodoWriteTool {
    fn name(&self) -> &'static str { "todo_write" }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        // 1. items_str = input.args.get("items").ok_or(tool_failure(...))?;
        //    错误信息要 educational：
        //      "todo_write expects an `items` field containing a JSON array of
        //       {content, activeForm, status} objects. The model can emit it as
        //       a literal array or a JSON-stringified one — both work."
        // 2. parse_json_value(items_str) → 必须是 JsonValue::Array
        // 3. 遍历 array，校验每项：
        //      - content: 非空 string
        //      - activeForm: 非空 string
        //      - status: 三合法值之一
        // 4. 上限：items.len() <= 100（rev 2: 50 → 100，留头）
        // 5. self.list.borrow_mut().replace(parsed)
        // 6. ToolOutput { summary: list.render_for_display() }
    }
}
```

错误分类（每条都给 LLM 可恢复的提示）：
- `items` 缺失 → `tool_failure("todo_write expects an `items` field ...")`
- `items` 不是合法 JSON → `tool_failure("malformed todo items JSON: <detail>; expected JSON array of {content, activeForm, status}")`
- `items` 顶层不是数组 → `tool_failure("`items` must be a JSON array, got <type>")`
- 单 todo 缺字段 → `tool_failure("todo at index N missing field <name>")`
- `status` 非法值 → `tool_failure("todo at index N: status must be pending|in_progress|completed (got <value>)")`
- `>100 items` → `tool_failure("too many todos (got N, max 100)")`

**不强校验** "exactly one in_progress" — 仅 system prompt 引导，dscode 不当 LLM 老师；
渲染时多个 in_progress 会全部显示，用户能看到 LLM 走偏。

#### Tool spec for LLM（OpenAI + Anthropic schema）

`items` 在 schema 里声明为字符串（与 dscode 当前所有工具一致：所有 args 都是字符串）。
LLM 可能发字面数组（修 C1 后也能走通）也可能发字符串，**两条路径都被支持**。

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

#### `core/observations.rs` 改动（修复 C2）

**KIND_COUNT 必须从 7 提升到 8**：

```rust
pub const KIND_COUNT: usize = 8;        // 7 → 8

pub fn kind_index(kind: ObservationKind) -> usize {
    let index = match kind {
        ObservationKind::FileExcerpt => 0,
        ObservationKind::Listing => 1,
        ObservationKind::SearchResults => 2,
        ObservationKind::Patch => 3,
        ObservationKind::Diff => 4,
        ObservationKind::ShellOutput => 5,
        ObservationKind::Other => 6,
        ObservationKind::Todos => 7,    // 新增
    };
    debug_assert!(index < KIND_COUNT);
    index
}
```

**`summarize_for_kind` 必须加 Todos arm**（修复 I4 — passthrough → trim）：

```rust
pub fn summarize_for_kind(text: &str, kind: ObservationKind) -> String {
    match kind {
        // 现有 7 个 arm 不变
        ObservationKind::Todos => {
            // 不 passthrough：transcript replay 时只留紧凑摘要
            // 完整 list 已通过 ModelRequest.todos 前向注入，无需再走 summary
            // 文本格式："5 todos: 2 completed, 1 in_progress, 2 pending"
            // 直接抽第一行（render_for_display 把摘要放第一行）
            text.lines().next().unwrap_or(text).to_string()
        }
    }
}
```

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
- 老 `default_registry()` 简化为 `default_registry_with_todos(Rc::new(RefCell::new(TodoList::default())))`，向后兼容（用于现有 3 个测试调用点）

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

#### `repl/transcript.rs::render_for_prompt` 改动（修复 I4 / smell #4）

当前 `transcript.render_for_prompt` 把每个 tool 调用的 `input` 字段原样 inline 进
prompt。`todo_write` 的 `input.args["items"]` 是完整 JSON 数组字符串（多 todo 时
～KB 级），每轮 replay 全文进 prompt — 严重 context-window 泄漏。

修法：在 `render_for_prompt` 渲染 tool 行时特判 `todo_write`：

```rust
// 现有伪代码：
// out.push_str(&format!(
//     "[tool] {name}({input_repr}) -> {status_label}\n{trimmed_output}\n\n",
// ));

// 改为：
let input_repr = if name == "todo_write" {
    // 只显示 items 数量，不展开 JSON
    let count = input.get("items")
        .and_then(|s| crate::util::json::parse_json_value(s).ok())
        .and_then(|v| match v { JsonValue::Array(a) => Some(a.len()), _ => None })
        .unwrap_or(0);
    format!("items=<{count} todos>")
} else {
    /* 现有逻辑 */
};
```

`trimmed_output` 也走 `summarize_for_kind`（现已对 Todos 走紧凑摘要），双层防泄漏。

#### `repl/session.rs` schema v2（修复 C3）

```rust
const SCHEMA_VERSION_LATEST: u64 = 2;

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

**`/save` 写**：始终写 v2（`"version": 2`），`todos` 字段始终存在（空 list 也存 `[]`）。

**`/load` 行为**（修复 C3 — 旧严格 check 不再适用）：

```rust
// 当前 strict check 改为分版本处理：
match version {
    1 => {
        // v1 文件：内存里注入 todos: vec![]，其他字段不变
        // 不修改原文件，下次 /save 才会升级到 v2
        // 不向用户输出警告（保持安静）
        SessionSnapshot {
            todos: vec![],
            // ... 其他字段从 v1 解析
        }
    }
    2 => {
        // v2 文件：直读所有字段
        // 缺 `todos` 字段：app_error("session v2 missing required field `todos`")
        // —— v2 schema 严格，不静默补默认值（避免 round-trip 数据丢失）
    }
    other => {
        return Err(app_error(format!(
            "unsupported session version: {other} (this dscode supports v1 and v2)"
        )));
    }
}
```

**重要语义**：
- v1 → v2 是 **opt-in upgrade**：load 不修改原文件，只在内存里补 todos
- 用户不 `/save` 就不会变成 v2 —— v1 老文件无副作用
- v2 缺 `todos` 字段是错误（不是 v1）—— 因为 v2 写出来的文件必有此字段；缺了说明是 corruption / 手改坏了
- v3+ 拒绝（保留当前 state，与 v1 时同保守）

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

`Repl::handle_line` 调 `AgentLoop::run_with` 时，把 `todos.clone()`（`Rc::clone`，
浅复制 Rc 指针，共享内部 TodoList）传进 `AgentLoopOptions`。

`/clear` 同时重置：transcript / tokens / **todos**。budget 与 skill 不动（与现有规则一致）。

`/save` 把 `todos.borrow().items.clone()` 装进 SessionSnapshot；
`/load` 反过来 `*todos.borrow_mut() = TodoList { items: snapshot.todos }`。

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

`dscode run` 在多次 `todo_write` 时顺序打印多份 list（最后一份是真相，前面的是历史）。
非 TTY 输出文件里每段都被 step 分隔包围，可读。

### Transcript replay（context-window 防泄漏）

LLM 第二轮看到的 prompt 中，过去的 `todo_write` 调用被压缩：

```
[tool] todo_write(items=<5 todos>) -> ok
5 todos: 1 completed, 1 in_progress, 3 pending

```

不再把完整 JSON 回灌进 prompt。当前 list 走 `Todos:` block 前向注入，单一信源。

### 单线程 ownership 模型

`Rc<RefCell<TodoList>>` 在四处 share：
1. `Repl` 拥有原 Rc
2. `Repl::handle_line` 调 `Rc::clone` 传给 `AgentLoopOptions.todos`
3. `AgentLoop::run_with` 把 Rc clone 传给 `default_registry_with_todos` → `TodoWriteTool.list`
4. `AgentLoop` 每步 `options.todos.borrow().snapshot()` 复制给 `ModelRequest.todos`

dscode 全同步、单线程（无 tokio、无 async fn）—— `Rc<RefCell<>>` 安全；`borrow_mut()`
都在 `TodoWriteTool::execute` 内立即释放，不跨工具调用边界。

**Phase 10b 风险预警**：未来 sub-agent 派发若让一个工具的 `execute` 内部回调 registry，
`borrow_mut()` 会在 nested 调用时 runtime panic。届时需切换为 `Cell<Vec<Todo>>` +
`take`/`replace` 模式。本 spec 不处理（YAGNI）。

## 切片：5 PR、~3 天（rev 2 — 8→5 合并）

| PR | 工作 | 估时 | 测试增量 | Land 条件 |
|----|------|------|----------|-----------|
| M1 | `core/todos.rs` 数据类型 + render + validate + `util/json::json_value_to_string` | 0.5d | +9 | 221 → 230；零 warnings |
| M2 | `model/protocol.rs` ObservationKind::Todos + `core/observations.rs` KIND_COUNT 7→8 + summarize_for_kind + `tools/todo.rs` TodoWriteTool + `tools/registry.rs::default_registry_with_todos` + `model/deepseek.rs::json_object_to_string_args` 修嵌套值 | 0.75d | +8 | 230 → 238；4 错误路径 + nested-args fix 覆盖 |
| M3 | `model/deepseek.rs` TOOL_SPECS + build_user_prompt Todos block + system prompt nudge + `core/loop_runtime.rs` AgentLoopOptions.todos + 注入 ModelRequest | 0.75d | +6 | 238 → 244；端到端 LLM 模拟 todo_write 更新 list |
| M4 | `repl/session.rs` schema v2 + v1→v2 migration + `repl/repl.rs::Repl.todos` + /clear 同步重置 + /save/load round-trip + `repl/transcript.rs` todo_write input 缩略 | 0.5d | +6 | 244 → 250；v2 round-trip + v1→v2 内存 migrate + v2 缺 todos 拒绝 + v3 拒绝 + transcript 缩略 |
| M5 | `repl/slash.rs::/todos` 命令 + `docs/todos.md` + `docs/roadmap.md` Phase 10a 标完成 + dogfood | 0.5d | +1 | 250 → 251；slash test + 手测 LLM 主动调 todo_write |

总：5 PR、+30 测试（221 → 251）、~3 天。

阶段化 land：
- **M1 + M2**：基础类型 + 工具 + protocol parser fix。LLM 此时若被诱导调 todo_write，技术上能成功（但还没接入 prompt nudge / AgentLoop）—— 隐性可用
- **M3**：prompt 注入 + AgentLoop 接管，LLM 看到 nudge + 真正能用
- **M4**：REPL 集成、可持久化
- **M5**：dogfood + 文档收尾

## 测试策略

### 单测（无外部依赖，+30）

`core/todos.rs` × 7：
- `TodoStatus::from_str` 三个合法值
- `TodoStatus::from_str` 非法值返 None
- `TodoStatus::label` 与 `from_str` round-trip 一致
- `TodoList::replace` 完全覆写旧 items（不追加）
- `TodoList::render_for_prompt` 输出格式 `- [status] content` per line
- `TodoList::render_for_display` 混合状态格式（in_progress 用 active_form）
- `TodoList::render_compact_summary` 计数正确

`util/json.rs::json_value_to_string` × 2：
- 嵌套 array 与 object 双向 round-trip：parse → write → parse 等价
- 字符串中含特殊字符（quote / newline）正确转义

`model/deepseek.rs::json_object_to_string_args` × 1：
- 输入嵌套数组 `{"items": [{...}]}` → `args["items"]` 是合法 JSON 字符串（codex C1 修复回归测试）

`model/protocol.rs` + `core/observations.rs` × 4：
- `ObservationKind::from_tool_name("todo_write")` → `Todos`
- `KIND_COUNT == 8`
- `kind_index(Todos) == 7`
- `compact_observations` 中老 `Todos` observation 被新的取代为 superseded

`tools/todo.rs` × 5：
- `execute` 成功路径（合法 items array）
- `execute` 失败：items 缺失
- `execute` 失败：items 非合法 JSON
- `execute` 失败：todo 缺 content / activeForm / status 任一字段
- `execute` 失败：>100 items

`model/deepseek.rs::build_user_prompt` + `core/loop_runtime.rs::build_system_prompt` × 4：
- `build_user_prompt` 空 todos → 不输出 Todos 段
- `build_user_prompt` 多状态混合 → 格式 `- [pending] X\n- [in_progress] Y\n- [completed] Z\n` exact
- `build_system_prompt` 含 nudge 关键字
- `build_system_prompt` 与 skill `system_append` 共存时 nudge 在末尾

`core/loop_runtime.rs` × 2：
- 端到端：单步循环 LLM 调 `todo_write` 后 `Rc<RefCell<TodoList>>` 状态被更新
- `AgentLoopOptions::default()` 给空 TodoList

`repl/session.rs` × 4：
- v2 round-trip（写 + 读 + 校验 todos 字段）
- v1 → v2 自动 migrate：手写 v1 JSON → load → todos 为空 vec、其他字段完整
- v2 缺 todos 字段 → app_error
- v3+ 未知版本拒绝

`repl/transcript.rs` × 1：
- `render_for_prompt` 中 `todo_write` 调用的 input 显示为 `items=<N todos>` 而非完整 JSON

`repl/slash.rs` × 4：
- `/todos` 空 list 输出 "no todos yet"
- `/todos` 含 items 输出格式正确
- `/clear` 同时重置 transcript + todos + tokens
- `/save` /  `/load` round-trip 保留 todos

### 集成 / 手测（M5）

设 `DEEPSEEK_API_KEY` 跑：
```bash
dscode chat
> 实现一个 Phase 10b 的 sub-agent 派发逻辑，分四步走，先列 todos 再开始
[期望:LLM 看到强 nudge + 多步请求 → 主动调 todo_write]
[屏幕看到 🛠 todo_write(items=...) → ✓ todo_write [todos] → 缩进 list]
> /todos                    # 看见当前 list
> /save phase-10b-plan      # 落盘 v2
> /load phase-10b-plan      # 还原
> /clear                    # 清空 todos + transcript
> /todos                    # → "no todos yet"
> /quit
```

- `dscode run > out.txt`：piped 时 ANSI 自动降级，list body 仍正确缩进
- 验证 v1 老 session：手写 v1 JSON → `/load` → `/todos` 空，其他字段完整保留 → `/save` 成 v2
- 多模型实测 `nudge` 接受度：DeepSeek-coder / DeepSeek-v3.2 至少 1 个能在 4+ 步任务主动调 todo_write

## 错误分类

延续 P3 `AppErrorKind`：

| 场景 | 分类 |
|------|------|
| `items` 字段缺失 | `tool_failure("todo_write expects an `items` field ...")` |
| `items` 非合法 JSON | `tool_failure("malformed todo items JSON: <detail>")` |
| `items` 顶层非数组 | `tool_failure("`items` must be a JSON array")` |
| 单 todo 缺字段 | `tool_failure("todo at index N missing field <name>")` |
| `status` 非法值 | `tool_failure("todo at index N: status must be ... (got <value>)")` |
| `>100 items` | `tool_failure("too many todos (got N, max 100)")` |
| Session schema v3+ | `app_error("unsupported session version: <N> (supports v1 and v2)")` |
| Session v2 缺 todos | `app_error("session v2 missing required field `todos`")` |
| Session 文件读写失败 | 现有 `session.rs` 错误路径不变 |

错误透过 `paint_tool_result(Failed, ...)` 红 ✗ 渲染，与其他工具失败一致。
错误信息**针对 LLM 可读性**优化（包含 hint），下一轮 LLM 能修正自己。

## 风险

| 风险 | 缓解 |
|------|------|
| LLM 不愿意调 `todo_write` 即使有强 nudge | M3 借用 Claude Code 验证过的措辞；M5 dogfood 多个模型实测；不达标时迭代 nudge 文本 |
| `Rc<RefCell<TodoList>>` 在 Phase 10b 嵌套工具调用时 runtime panic | 本 spec 不引入嵌套；预警写入"单线程 ownership 模型"段；10b 启动时切换 `Cell<Vec<Todo>>` 模式 |
| schema v1 → v2 migration 破坏现有老数据 | M4 单测覆盖；v1 加载只在内存注入 todos: []，下次 /save 才升 v2；不动原文件 |
| LLM 发字面数组 vs 字符串 vs 类型混乱 | C1 修复后 `json_object_to_string_args` 双路径都接受 + re-serialize；educational 错误信息 |
| `items` 嵌套引号 + UTF-8 + 控制字符转义 | Phase 9b C2 修复后 `\u/\b/\f` 都正确解码；`json_value_to_string` 复用 `json_escape` |
| 强 nudge 让 LLM 在简单任务上滥用 todo_write | nudge 末尾明确 "Skip todo_write only for trivial single-step requests"；M5 dogfood 验证 |
| ToolOutput.summary 极长（100 todos）影响 transcript context | `summarize_for_kind(Todos)` 走紧凑摘要（不 passthrough）；`transcript.render_for_prompt` 缩略 todo_write input；compact_observations 自动 supersede 老 Todos observation |
| LLM 把 active_form 写错时态 | active_form 是纯 cosmetic（never 注入回 prompt）；UI 偶尔 awkward 但不影响 agent 行为；不强校验 |
| LLM 标多个 in_progress 同时 | 不强校验；`render_for_display` 全部显示，用户能看到 LLM 走偏 |
| `tool_choice: "auto"` 不能保证 LLM 调 todo_write | 现有设计；用户可 `/clear` 重述；spec 明确"by design no enforcement" |

## 待解项

无。所有 critical / important review 反馈已吸收。

## 后续 (Phase 10b/10c 候选)

明确为 10a 之外，不写入此 spec：
- **Phase 10b**: Sub-agent 派发（让 LLM 通过 `dispatch_subagent` 工具派子 agent；切换 `Cell<>` ownership 模型）
- **Phase 10c**: 跨进程 todos 持久化、`/todos pin` workspace 锁、`dscode init <template>` 项目模板、cargo / npm 专用工具、LLM 自我 replan 失败回路
- TodoTool 字段拓展（notes / due / priority — YAGNI 不做）
