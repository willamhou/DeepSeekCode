# Streaming SSE — Phase 9b 设计

最后更新：`2026-04-30`
状态：`spec` (未实现)
关联 Phase：9b (REPL 第二阶段：流式 token 输出)

## 背景

Phase 9a 把 `dscode chat` 升级为 REPL，但 LLM 调用仍是 curl 一次拿完整 response。Claude Code 与 Codex CLI 都默认流式 SSE — 用户体验差距明显。本 spec 定义全命令 + 全协议的流式接入。

## 目标

- 所有命令 (`dscode run` / `dscode chat` / `dscode pr review|fix|patch`) 默认流式
- OpenAI-compatible + Anthropic-compatible 两条路径都接 SSE
- TTY 时 ANSI 着色（cyan = assistant text, yellow = tool block, green/red = tool result）；非 TTY 自动降级为纯文本
- Tool calls 拼装等待：args 跨 chunk 累积成完整 JSON 后整块渲染（与 Claude Code 一致）
- Token usage 从最后一帧 (`usage` / `message_delta`) 抽取
- 零新依赖 — 复用 curl `-N` (no-buffer) + 手写 SSE 解析

## 非目标 (v1)

- Ctrl+C 中断（Phase 9c）
- 上下箭头历史（Phase 9c）
- patch / shell tool 输出流式（v1 一次性印整块）
- syntax highlight（需 syntect dep）
- 多色主题切换 / 用户配置颜色

## 架构

### 模块边界

```
src/
├── util/sse.rs              # 通用 SSE 框解析 (新)
├── ui/stream.rs             # StreamEvents trait + TtyRenderer (新)
├── model/client.rs          # ModelClient trait 增 events 参数
├── model/deepseek.rs        # parse_openai_stream + parse_anthropic_stream (新)
├── util/process.rs          # spawn_streaming + StreamingProcess (新)
├── core/loop_runtime.rs     # 切到 renderer，删老 println
└── repl/repl.rs             # handle_line 复用 AgentLoop renderer
```

### 数据契约

#### `StreamEvents` trait (位于 `src/ui/stream.rs`)

```rust
pub trait StreamEvents {
    /// 每个 SSE text content chunk 立即调用一次
    fn on_text_delta(&mut self, chunk: &str);

    /// LLM 流结束 (finish_reason / message_stop)
    fn on_assistant_done(&mut self, full_text: &str);

    /// Tool call args 拼齐后调用一次（在 on_assistant_done 之后）
    fn on_tool_call(&mut self, name: &str, input: &BTreeMap<String, String>);
}

pub struct NoopStreamEvents;  // 测试 / 离线 fallback 时使用
```

#### `TtyRenderer<W>` (同文件)

```rust
pub struct TtyRenderer<W: Write> {
    out: W,
    use_ansi: bool,
    text_started: bool,
}

impl TtyRenderer<StdoutLock<'static>> {
    pub fn from_stdout() -> Self;  // 自动检测 is_terminal()
}

impl<W: Write> TtyRenderer<W> {
    pub fn new_with(out: W, use_ansi: bool) -> Self;
    pub fn paint_step_divider(&mut self, step_index: usize);
    pub fn paint_tool_result(&mut self, ok: bool, label: &str, body: &str);
}

impl<W: Write> StreamEvents for TtyRenderer<W> { ... }
```

#### `ModelClient` trait (修改 `src/model/client.rs`)

```rust
pub trait ModelClient {
    fn respond(
        &self,
        input: ModelRequest,
        events: &mut dyn StreamEvents,
    ) -> AppResult<(ModelResponse, Option<TokenUsage>)>;
}
```

#### `SseFrame` (位于 `src/util/sse.rs`)

```rust
pub struct SseFrame {
    pub event: Option<String>,    // None when default ("message")
    pub data: String,             // 多 data: 行用 \n 串接
}

pub fn read_frame<R: BufRead>(reader: &mut R) -> std::io::Result<Option<SseFrame>>;
```

#### `StreamingProcess` (位于 `src/util/process.rs`)

```rust
pub struct StreamingProcess {
    child: std::process::Child,
    stdout: BufReader<std::process::ChildStdout>,
}

pub fn spawn_streaming(bin: &str, args: &[&str]) -> AppResult<StreamingProcess>;

impl StreamingProcess {
    pub fn stdout(&mut self) -> &mut BufReader<std::process::ChildStdout>;
    pub fn finish(self) -> AppResult<(ExitStatus, String)>;  // 返回 status + stderr 末尾 64 KB
}
```

### Curl 调用

OpenAI 路径请求：
```rust
body 加 "stream": true, "stream_options": {"include_usage": true}
header 加 "Accept: text/event-stream"
curl args: -sS -N --max-time 60 -X POST <endpoint> ...
```

Anthropic 路径请求：
```rust
body 加 "stream": true
header 加 "Accept: text/event-stream"
curl args: -sS -N --max-time 60 -X POST <endpoint> ...
```

`-N` 关闭 stdout buffering — **关键**，否则 curl 会等到 buffer 满才喷数据。

### SSE 解析

#### 通用框 (util/sse.rs)
- 一行 = `name: value`，UTF-8
- 字段名：`data`, `event`, `id`, `retry`（v1 只用前两个）
- 空行触发 frame 提交
- `:`-开头行是 comment（忽略）
- 多 `data:` 行用 `\n` 串接成单一 `data` 字符串

#### OpenAI 帧
每个 frame 的 `data` 是 chat completion chunk：
```json
{"choices":[{"delta":{"content":"Hello"},"finish_reason":null}]}
{"choices":[{"delta":{"tool_calls":[{...}]},"finish_reason":"tool_calls"}]}
{"usage":{"prompt_tokens":N,"completion_tokens":N}}  // 最后一帧（include_usage=true）
```
`data: [DONE]` 收尾。

`tool_calls[]` 跨多个 frame 分片送达：每片可能含 `id`（首次）/ `function.name` (首次) / `function.arguments` (累积)。客户端拼成 `OpenAiToolAssembly { id, name, arguments }`。

#### Anthropic 帧
每个 frame 带显式 `event: <type>`：

| event | 内容 |
|---|---|
| `message_start` | 初始 `usage.input_tokens` |
| `content_block_start` | `content_block.type` 是 `text` 或 `tool_use`；tool_use 含 `name` + `id` |
| `content_block_delta` | `delta.type` 是 `text_delta` (含 `text`) 或 `input_json_delta` (含 `partial_json`) |
| `content_block_stop` | 块结束（不需要处理） |
| `message_delta` | `usage.output_tokens`，`stop_reason` |
| `message_stop` | 流结束 |

`partial_json` 累积成完整 JSON 后用现有 `parse_tool_arguments` 解析。

### Tool call 拼装策略

**两边一致**：
1. 流式过程中 `on_text_delta` 立即转发到 renderer (cyan 立即写)
2. tool call 数据静默 buffer（不写屏）
3. 流结束 (`[DONE]` / `message_stop`)：
   a. `events.on_assistant_done(&full_text)` → renderer 关闭 cyan + `\n`
   b. 若有 tool call：解析 args JSON → `events.on_tool_call(name, input)` → renderer 写 yellow `🛠 name(k=v, ...)`
4. 返回 `(ModelResponse, Option<TokenUsage>)` 给 caller

错误时（curl 非 0 / SSE 解析失败）→ 当前 step 返 Err，AgentLoop 捕作 failed observation，已经流出去的 token 不回滚。

### TtyRenderer 渲染规则

| 元素 | TTY (ANSI) | 非 TTY |
|---|---|---|
| Assistant text content | `\x1b[36m...\x1b[0m` (cyan) | 纯文本 |
| Tool call header `🛠 name(args)` | `\x1b[33m...\x1b[0m` (yellow) | `> name(args)` |
| Tool result header `✓ name` | `\x1b[32m✓\x1b[0m name` | `OK: name` |
| Tool failure header `✗ name` | `\x1b[31m✗\x1b[0m name` | `ERR: name` |
| Step 分隔 | `\x1b[2m─── step N ───\x1b[0m` (dim) | `─── step N ───` |
| Tool result body | 缩进 2 空格 | 缩进 2 空格 |

#### Tool call args 内联格式
```
🛠 read_file(path=src/main.rs, max_lines=40)
🛠 apply_patch(cwd=., patch=--- src/foo.rs\n+++ src/foo.rs\n@@…)
```

每个 value 通过 `abbreviate_for_inline`：
- 替换 `\n` `\r` `\t` 为字面 `\\n` `\\r` `\\t`
- 字符数 > 80 → head + `…`

### AgentLoop::run_with 改造

```rust
let mut renderer = TtyRenderer::from_stdout();
for step in 0..steps {
    renderer.paint_step_divider(step + 1);

    let (response, step_usage) =
        client.respond(request, &mut renderer)?;

    last_message = response.message.clone();
    if let Some(u) = step_usage {
        total_usage.prompt += u.prompt;
        total_usage.completion += u.completion;
    }

    match response.action {
        ModelAction::CallTool { tool_name, input } => {
            match registry.execute_with_policy(...) {
                Ok(output) => {
                    let summary = summarize_for_kind(...);
                    renderer.paint_tool_result(true, &tool_name, &summary);
                    // observations + tool_events push (unchanged)
                }
                Err(error) => {
                    let summary = summarize_for_kind(...);
                    renderer.paint_tool_result(false, &tool_name, &summary);
                    // observations + tool_events push (unchanged)
                }
            }
        }
        ModelAction::Finish => break,
    }
}
```

老的 `println!("Step {N}: {message}")` / `println!("Tool 'x' output [...]:")` 一律删除。

### 离线 planner fallback

`respond_offline` 没有真实 SSE，但仍走 `StreamEvents`：

```rust
fn respond_offline(&self, input, events: &mut dyn StreamEvents) -> ModelResponse {
    let response = self.compute_offline_response(input);
    events.on_text_delta(&response.message);
    events.on_assistant_done(&response.message);
    if let ModelAction::CallTool { tool_name, input } = &response.action {
        events.on_tool_call(tool_name, &input.args);
    }
    response
}
```

→ 离线模式下屏幕一致看到 cyan + yellow，只是 token 一次到位而非渐进。

### Repl 与 renderer 关系

`Repl::handle_line` 不持有 renderer。每次调 `AgentLoop::run_with` 内部新建 `TtyRenderer::from_stdout()`，stream 完该 turn 后丢弃。

`Repl` 提示符 `> ` 写 stderr（与今日一致，不动）。流式内容写 stdout 不冲突；用户在终端上看到 stderr 提示符与 stdout 流并列。

## 错误分类（沿用 P3 `AppErrorKind`）

| 场景 | 分类 |
|---|---|
| curl 进程非 0 退出 | `tool_failure(stderr_tail)` |
| curl 不在 PATH | `app_error("curl CLI not found")` |
| curl `--max-time` 触发 | `tool_failure("request timed out")` |
| SSE 帧解析失败 | `tool_failure("malformed SSE frame: <detail>")` |
| Tool args JSON 拼装后解析失败 | `tool_failure("malformed tool arguments: <detail>")` |
| HTTP 4xx/5xx (curl exit 0 但 server 错) | 解析 frame 时遇到 `error` 字段 → `tool_failure(<api error>)` |

## 测试策略

### 单测（无外部依赖）
- `util/sse.rs` × 5：单帧 / 多 data 行 / comment / 显式 event / EOF 中断
- `ui/stream.rs` × 8：ANSI on/off × text-delta + tool-call + tool-result + step-divider；header truncate / escape；NoopStreamEvents 不 panic
- `model/deepseek.rs` × 8：OpenAI 流式 4 (text deltas / tool assembly / usage / [DONE]) + Anthropic 流式 4 (text deltas / input_json_delta assembly / usage merge / message_stop)

总计 **+21** 测试 (175 → 196)。

### 集成 / 手测
- 设 `DEEPSEEK_API_KEY` 跑 `dscode chat`，看 token 渐进出现
- `dscode run "..." > out.txt`：stdout 进文件，无 ANSI
- `dscode pr review <pr>`：流式 review markdown
- 故意填错 base_url：错误显示在已流文本之后，红色

## 切片：7 PR、~3.5–4 天

| PR | 工作 | 估时 | Land 条件 |
|---|---|---|---|
| M1 | `util::sse` 框解析 | 0.5d | 175→180；0 warnings |
| M2 | `ui::stream` (`StreamEvents` + `TtyRenderer`) | 0.5d | 180→188；ANSI on/off 验证 |
| M3 | `ModelClient::respond` 加 `events` 参数（不改行为） | 0.5d | 188 测试不变；用户感知 0 |
| M4 | DeepSeek streaming 真接入（OpenAI + Anthropic） | 1d | 188→196；`DEEPSEEK_API_KEY` 设了能流 |
| M5 | `loop_runtime` 切到 renderer，删老 println | 0.5d | 196 通过；屏幕形态变 |
| M6 | Repl + 全命令 dogfood | 0.5d | 4 个命令路径手测通 |
| M7 | 文档 + roadmap 标完成 | 0.5d | docs/streaming.md + REPL doc 更新 |

阶段化 land：
- **M1+M2+M3**：基础设施，零行为变化
- **M4+M5+M6**：用户感知大变（从一次性 → 流式）
- **M7**：文档收尾

## 风险

| 风险 | 缓解 |
|---|---|
| curl `-N` 在某些环境（macOS old）行为差异 | M7 文档列已知；`dscode doctor` 加 curl 版本检查 |
| Anthropic `partial_json` 累积出来不是合法 JSON | `content_block_stop` 后再 dispatch；用现有 `parse_tool_arguments` 严格校验失败时降级为 tool_failure |
| 流式中断后已显示文本 + 红字 Err | 接受（与 Claude Code 一致）；测试覆盖 |
| Token 双计 | M3 后只有流式路径 emit usage；非流式（离线 fallback）emit None |
| `dscode run > out.txt` 颜色错乱 | M2 `is_terminal()` 检测自动降级 |
| 离线 fallback 体验突兀 | `respond_offline` 也调 events.on_text_delta；颜色一致 |
| 大 patch 输出阻塞 stdout | M5 后 `paint_tool_result` 一次性写；future M2 patch 工具流式（Phase 9c） |

## 待解项

无。所有交互式问题在 brainstorming 中收敛：
- Q1: 全部命令 + 全部协议流式默认
- Q2: tool call 拼装等待整块渲染
- Q3: stdout + ANSI conditional on TTY
- Q4: spawn curl + BufRead + stderr 末尾 dump

## 后续 (Phase 9c 候选)

- Ctrl+C 优雅中断（signal-hook + child.kill() 联动）
- 上下箭头历史（rustyline 或自写 raw mode）
- Patch / shell tool 输出流式
- Streaming token rate limiter
- Syntax highlight (syntect dep)
- 用户可配色 (`.dscode/colors.toml`)
- `--no-color` flag（强制关 ANSI）
