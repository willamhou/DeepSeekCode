# DeepSeek-TUI RLM Python Interpreter Fallback Parity Spec

日期：2026-05-13

对比对象：`Hmbown/DeepSeek-TUI`，`main` HEAD `3382242`

## 背景

DeepSeek-TUI 的 changelog 明确修复过 Windows 和部分主机上没有 `python3` 的问题，
通过 `python3`、`python`、`py -3` 探测 Python 解释器。DeepSeekCode 当前
`rlm_python` / `rlm_python_session` hardcode `python3`，在这些环境下会误报不可用。

## 目标

- RLM Python sandbox 启动前按顺序探测 `python3`、`python`、`py -3`。
- 使用第一个能执行 `--version` 的解释器运行 sandbox。
- 错误信息说明尝试过的解释器，而不是只说 `python3`。
- 测试路径使用同一个 resolver，避免本机只有 `python` 时跳过或失败。

## 非目标

- 不自动安装 Python。
- 不改变 sandbox 参数、权限或 helper surface。

## 验收标准

1. resolver 候选顺序为 `python3`、`python`、`py -3`。
2. sandbox 使用 resolver 返回的解释器。
3. RLM Python 相关测试通过。

## 实现结果

- `RLM_PYTHON_INTERPRETERS` 按 `python3`、`python`、`py -3` 定义候选。
- `resolve_rlm_python_interpreter()` 用 `--version` 探测第一个可用解释器。
- `run_rlm_python_sandbox()` 使用 resolver 返回的 program/args 启动 sandbox。
- 没有可用解释器时，错误信息说明已尝试 `python3`、`python`、`py -3`。
- RLM Python 测试用 resolver 判断是否跳过，避免只检查 `python3`。

## 验证

- `/home/willamhou/.cargo/bin/cargo fmt`
- `/home/willamhou/.cargo/bin/cargo fmt --check`
- `/home/willamhou/.cargo/bin/cargo test rlm_python_interpreter`
- `/home/willamhou/.cargo/bin/cargo test rlm_python`
- `/home/willamhou/.cargo/bin/cargo test rlm`
- `git diff --check`
- `/home/willamhou/.cargo/bin/cargo test`
- `/home/willamhou/.cargo/bin/cargo package --allow-dirty`
