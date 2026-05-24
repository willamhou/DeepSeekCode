use crate::cli::app::HelpArgs;
use crate::error::AppResult;

pub fn run(args: HelpArgs) -> AppResult<()> {
    println!("{}", render_help(&args.topics));
    Ok(())
}

fn render_help(topics: &[String]) -> String {
    match topics.first().map(String::as_str) {
        None => global_help().to_string(),
        Some("dogfood") => dogfood_help(topics.get(1).map(String::as_str)).to_string(),
        Some("tui") => tui_help().to_string(),
        Some("quickstart") | Some("onboarding") => quickstart_help().to_string(),
        Some("run") => run_help().to_string(),
        Some("exec") => exec_help().to_string(),
        Some("config") => config_help().to_string(),
        Some("stats") => stats_help().to_string(),
        Some("events") | Some("event") => events_help().to_string(),
        Some("benchmark") => benchmark_help().to_string(),
        Some("mcp") => mcp_help().to_string(),
        Some("hooks") => hooks_help().to_string(),
        Some("skills") => skills_help().to_string(),
        Some("task") | Some("tasks") => task_help().to_string(),
        Some("github") => github_help().to_string(),
        Some("help") => global_help().to_string(),
        Some(other) => format!(
            "{}\n\nUnknown help topic `{}`. Use `deepseek --help` for the command list.",
            global_help(),
            other
        ),
    }
}

fn global_help() -> &'static str {
    concat!(
        "DeepSeekCode\n",
        "\n",
        "Usage:\n",
        "  deepseek                         Start the full-screen terminal workbench in a TTY\n",
        "  deepseek chat                    Start the line-oriented coding agent REPL\n",
        "  deepseek tui                     Explicitly start the terminal workbench\n",
        "  deepseek quickstart              Show first-run readiness and next commands\n",
        "  deepseek run \"<task>\"             Run one coding task and exit\n",
        "  deepseek exec run \"<task>\"        Run a durable one-shot agent task\n",
        "  deepseek stats                    Inspect runtime usage, cache, and repair evidence\n",
        "  deepseek events replay <thread>   Summarize a runtime event timeline\n",
        "  deepseek task start \"<task>\"       Start an isolated background worktree task\n",
        "  deepseek benchmark                Run deterministic benchmark gates\n",
        "  deepseek dogfood <action>        Run self-verification and release evidence commands\n",
        "  deepseek help [topic]            Show command help\n",
        "  deepseek --version               Show version\n",
        "\n",
        "Common commands:\n",
        "  chat, repl, interactive          Explicit aliases for the interactive REPL\n",
        "  tui                              Terminal workbench with sessions, tools, and approvals\n",
        "  run                              One-shot coding task\n",
        "  exec                             Durable exec/resume task runner\n",
        "  config                           Workspace provider, model, preset, budget, auth, and network config\n",
        "  stats                            Runtime usage, cache, repair, and prompt-layer stats\n",
        "  events                           Runtime event replay and thread comparison evidence\n",
        "  agents                           Durable runtime, service, and shell supervisor tools\n",
        "  task                             Local background worktree task runner\n",
        "  quickstart                       First-run readiness, next commands, and starter tasks\n",
        "  mcp                              MCP client/server configuration tools\n",
        "  hooks                            Local hook fixture and verification tools\n",
        "  skills                           Skill discovery and metadata validation\n",
        "  pr                               GitHub PR review/fix/patch helpers\n",
        "  github                           GitHub Action event bridge\n",
        "  dogfood                          Project self-test and release evidence workflow\n",
        "  benchmark                        Deterministic offline regression manifest runner\n",
        "\n",
        "Examples:\n",
        "  deepseek\n",
        "  deepseek chat\n",
        "  deepseek tui\n",
        "  deepseek quickstart\n",
        "  deepseek run \"fix the failing tests and summarize the diff\"\n",
        "  deepseek stats --json\n",
        "  deepseek events replay thread-123 --limit 50\n",
        "  deepseek events diff thread-before thread-after\n",
        "  deepseek dogfood live-plan --limit 10\n",
        "  deepseek dogfood live-run --limit 4\n",
        "\n",
        "More help:\n",
        "  deepseek help tui\n",
        "  deepseek help run\n",
        "  deepseek help config\n",
        "  deepseek help stats\n",
        "  deepseek help events\n",
        "  deepseek help quickstart\n",
        "  deepseek help benchmark\n",
        "  deepseek help mcp\n",
        "  deepseek help hooks\n",
        "  deepseek help skills\n",
        "  deepseek help task\n",
        "  deepseek help github\n",
        "  deepseek help dogfood\n",
        "  deepseek help dogfood replay-benchmark"
    )
}

fn quickstart_help() -> &'static str {
    concat!(
        "DeepSeekCode quickstart\n",
        "\n",
        "Usage:\n",
        "  deepseek quickstart [--json]\n",
        "\n",
        "Prints a side-effect-free first-run checklist: workspace config state, API key\n",
        "env readiness, terminal mode, next commands, and starter coding tasks. Secret\n",
        "values are never printed."
    )
}

fn tui_help() -> &'static str {
    concat!(
        "DeepSeekCode TUI\n",
        "\n",
        "Usage:\n",
        "  deepseek tui [--demo] [--once] [--runtime-url <url>]\n",
        "  deepseek tui --entrypoint-smoke [--smoke-bin <path>]\n",
        "\n",
        "Options:\n",
        "  --demo                 Render deterministic demo state instead of local runtime state\n",
        "  --once                 Render one snapshot and exit; useful for CI and README captures\n",
        "  --runtime-url <url>    Connect to a running DeepSeekCode HTTP runtime\n",
        "  --entrypoint-smoke     Verify bare `deepseek` enters the TUI in a real PTY\n",
        "  --smoke-bin <path>     Smoke a selected binary instead of the current executable"
    )
}

fn run_help() -> &'static str {
    concat!(
        "DeepSeekCode run\n",
        "\n",
        "Usage:\n",
        "  deepseek run [--skill <name>] [--budget <1..200>] [--preset <auto|flash|pro>] [--pro-next] [--benchmark-gate] \"<task>\"\n",
        "\n",
        "Runs one coding-agent task and exits. Use bare `deepseek` for the interactive\n",
        "full-screen workbench or `deepseek chat` for the line-oriented REPL."
    )
}

fn exec_help() -> &'static str {
    concat!(
        "DeepSeekCode exec\n",
        "\n",
        "Usage:\n",
        "  deepseek exec [--skill <name>] [--budget <1..200>] [--preset <auto|flash|pro>] [--pro-next] [--image <path>] [--json] \"<task>\"\n",
        "  deepseek exec run [--skill <name>] [--budget <1..200>] [--preset <auto|flash|pro>] [--pro-next] [--image <path>] [--json] \"<task>\"\n",
        "  deepseek exec resume [session-id] [--skill <name>] [--budget <1..200>] [--preset <auto|flash|pro>] [--pro-next] [--image <path>] [--json] [task]\n",
        "\n",
        "Runs or resumes durable coding-agent tasks with structured output support."
    )
}

fn config_help() -> &'static str {
    concat!(
        "DeepSeekCode config\n",
        "\n",
        "Usage:\n",
        "  deepseek config init [--force]\n",
        "  deepseek config auth [ENV] --stdin\n",
        "  deepseek config provider [show|list|<name> [model]]\n",
        "  deepseek config model [show|list|<model>]\n",
        "  deepseek config preset [show|auto|flash|pro]\n",
        "  deepseek config budget [show|off|MICROUSD|raise MICROUSD|+MICROUSD]\n",
        "  deepseek config network allow|deny <host>\n",
        "  deepseek config --print-default\n",
        "\n",
        "Manages project configuration for provider/model selection, DeepSeek V4\n",
        "routing presets, optional session budget limits, safe API-key setup, and\n",
        "persistent network policy."
    )
}

fn stats_help() -> &'static str {
    concat!(
        "DeepSeekCode stats\n",
        "\n",
        "Usage:\n",
        "  deepseek stats [--thread <id>|--session <id|name>] [--limit <N>] [--json] [--require-prefix-stable]\n",
        "\n",
        "Aggregates durable runtime usage records, prompt cache hit/miss tokens,\n",
        "estimated cost, model split, repair events, repeated-tool suppressions, and\n",
        "prompt-layer snapshots when they have been recorded by the runtime. The\n",
        "--require-prefix-stable gate requires snapshots and fails if cache-stable\n",
        "prompt layers changed hash."
    )
}

fn events_help() -> &'static str {
    concat!(
        "DeepSeekCode events\n",
        "\n",
        "Usage:\n",
        "  deepseek events replay <thread-id> [--limit <N>] [--json]\n",
        "  deepseek events diff <left-thread-id> <right-thread-id> [--json]\n",
        "\n",
        "`replay` renders a compact, chronological summary of durable runtime events.\n",
        "`diff` compares two threads for cost, cache hit rate, tool calls, failed\n",
        "tool calls, repair events, repeated-tool suppressions, and modified-file\n",
        "evidence when file paths were recorded."
    )
}

fn benchmark_help() -> &'static str {
    concat!(
        "DeepSeekCode benchmark\n",
        "\n",
        "Usage:\n",
        "  deepseek benchmark [--manifest <path>] [--out <path>] [--category <name>] [--case <name>]... [--accept-live-baseline]\n",
        "\n",
        "Runs benchmark cases from the manifest. `--category` and `--case` select a\n",
        "targeted slice for local evidence; filtered runs write a report but do not\n",
        "advance benchmark history or enforce full trend/live gates."
    )
}

fn github_help() -> &'static str {
    concat!(
        "DeepSeekCode GitHub Action bridge\n",
        "\n",
        "Usage:\n",
        "  deepseek github action [--event <path>] [--event-name <name>] [--mode auto|review|fix|patch] [--trigger <text>] [--post]\n",
        "  deepseek github action --background-task [--task-id <id>] [--task-no-run]\n",
        "  deepseek github action --dry-run [--github-output] [--require-mode <mode[,mode]>]\n",
        "  deepseek github pr-head <reference> [--repo-owner <owner>] [--github-output]\n",
        "  deepseek github fixture-smoke [--mode all|review|write] [--json] [--keep-workdir]\n",
        "\n",
        "Reads GitHub Actions event payloads, resolves a PR target, and delegates to\n",
        "`deepseek pr review|fix|patch`. Use --dry-run for parse-only workflow checks.\n",
        "`--background-task` delegates the resolved request into `deepseek task start`.\n",
        "`--github-output` appends target fields to $GITHUB_OUTPUT, and\n",
        "`--require-mode` fails early if auto mode resolves to an unexpected workflow.\n",
        "`pr-head` resolves the PR head branch and can refuse fork-owned branches\n",
        "before a write-capable checkout. `fixture-smoke` runs a local no-network\n",
        "review/write workflow simulation in a temporary Git repository."
    )
}

fn mcp_help() -> &'static str {
    concat!(
        "DeepSeekCode MCP\n",
        "\n",
        "Usage:\n",
        "  deepseek mcp list\n",
        "  deepseek mcp tools [server]\n",
        "  deepseek mcp call <server> <tool> [json-args]\n",
        "  deepseek mcp fixture-smoke [--json]\n",
        "\n",
        "Manages MCP servers and exposes stdio, HTTP, and SSE MCP tools to the agent.\n",
        "`fixture-smoke` runs a local no-network smoke across stdio, HTTP, and SSE\n",
        "discovery, tool calls, dynamic mcp__server__tool exposure, and schema cache."
    )
}

fn hooks_help() -> &'static str {
    concat!(
        "DeepSeekCode hooks\n",
        "\n",
        "Usage:\n",
        "  deepseek hooks fixture-smoke [--json]\n",
        "\n",
        "Runs a local no-network hook smoke through the agent loop. The fixture verifies\n",
        "session_start, user_prompt_submit, pre_tool_use, post_tool_use, and\n",
        "session_stop against a real tool call and structured allow/add_context output."
    )
}

fn skills_help() -> &'static str {
    concat!(
        "DeepSeekCode skills\n",
        "\n",
        "Usage:\n",
        "  deepseek skills list [--json] [--dir <path>]...\n",
        "  deepseek skills validate [--json] [--strict] [--dir <path>]...\n",
        "\n",
        "Lists bundled and user skills, then validates TOML metadata with the same\n",
        "directory precedence used by the runtime. `--strict` turns metadata warnings\n",
        "into a non-zero exit for CI or release gates."
    )
}

fn task_help() -> &'static str {
    concat!(
        "DeepSeekCode task\n",
        "\n",
        "Usage:\n",
        "  deepseek task start [--cwd <repo>] [--base <ref>] [--id <id>] [--branch <name>] [--skill <name>] [--budget <1..200>] [--no-run] [--json] \"<task>\"\n",
        "  deepseek task list [--cwd <repo>] [--json]\n",
        "  deepseek task show <id> [--cwd <repo>] [--tail <lines>] [--json]\n",
        "  deepseek task stop <id> [--cwd <repo>] [--json]\n",
        "  deepseek task diff <id> [--cwd <repo>] [--stat] [--json]\n",
        "  deepseek task merge <id> [--cwd <repo>] [--check] [--allow-dirty] [--json]\n",
        "  deepseek task reject <id> [--cwd <repo>] [--keep-worktree] [--json]\n",
        "  deepseek task fixture-smoke [--json] [--keep-workdir]\n",
        "\n",
        "`task start` creates an isolated git worktree under `.dscode/task-runner/`,\n",
        "records metadata and logs, then launches `deepseek exec --json` in the\n",
        "worktree. Use `--no-run` to create only the worktree and record for local\n",
        "release smoke checks without spending model calls. `merge` applies the task\n",
        "worktree diff back to the original repo only after a clean-worktree check;\n",
        "`reject` marks the record rejected and removes the managed task worktree by\n",
        "default."
    )
}

fn dogfood_help(topic: Option<&str>) -> &'static str {
    match topic {
        Some("run") => dogfood_run_help(),
        Some("external-fixture") | Some("external-write-fixture") => {
            dogfood_external_fixture_help()
        }
        Some("external-evidence")
        | Some("external-fixture-evidence")
        | Some("verify-external-fixture-evidence") => dogfood_external_evidence_help(),
        Some("repair-cache-evidence") | Some("repair-evidence") => {
            dogfood_repair_cache_evidence_help()
        }
        Some("replay-benchmark") | Some("replay-bench") => dogfood_replay_help(),
        Some("live-plan") | Some("plan-live") => dogfood_live_plan_help(),
        Some("live-run") | Some("run-live") => dogfood_live_run_help(),
        Some("live-evidence") | Some("verify-live-evidence") => dogfood_live_evidence_help(),
        Some("report") => dogfood_report_help(),
        Some("export-benchmark") | Some("export-bench") => dogfood_export_help(),
        Some("promote-benchmark") | Some("promote-bench") => dogfood_promote_help(),
        _ => concat!(
            "DeepSeekCode dogfood\n",
            "\n",
            "Usage:\n",
            "  deepseek dogfood run \"<task>\"\n",
            "  deepseek dogfood run --from-benchmark <case> [--manifest <path>]\n",
            "  deepseek dogfood external-fixture --workdir <path> \"<task>\"\n",
            "  deepseek dogfood external-evidence --file <path> [--json]\n",
            "  deepseek dogfood repair-cache-evidence [--out <path>] [--json]\n",
            "  deepseek dogfood replay-benchmark [--manifest <path>] [--category <name>] [--limit <n>]\n",
            "  deepseek dogfood live-plan [--limit <n>] [--json]\n",
            "  deepseek dogfood live-run [--limit <n>] [--category <name>] [--execute]\n",
            "  deepseek dogfood live-evidence --file <path> [--json]\n",
            "  deepseek dogfood report [requirements]\n",
            "  deepseek dogfood export-benchmark [--out <path>]\n",
            "  deepseek dogfood promote-benchmark [--dry-run]\n",
            "\n",
            "Dogfood commands are for self-verification, benchmark evidence, and release\n",
            "gates. Normal product use is `deepseek`, `deepseek tui`, or `deepseek run`.\n",
            "\n",
            "More help:\n",
            "  deepseek help dogfood external-fixture\n",
            "  deepseek help dogfood external-evidence\n",
            "  deepseek help dogfood repair-cache-evidence\n",
            "  deepseek help dogfood replay-benchmark\n",
            "  deepseek help dogfood live-plan\n",
            "  deepseek help dogfood live-run\n",
            "  deepseek help dogfood live-evidence\n",
            "  deepseek help dogfood report"
        ),
    }
}

fn dogfood_run_help() -> &'static str {
    concat!(
        "DeepSeekCode dogfood run\n",
        "\n",
        "Usage:\n",
        "  deepseek dogfood run [--skill <name>] [--budget <1..200>] [--workdir <path>] [--isolate-workdir] [--benchmark-gate] [--notes <text>] \"<task>\"\n",
        "  deepseek dogfood run --from-benchmark <case> [--manifest <path>] [--budget <1..200>] [--benchmark-gate] [--notes <text>]\n",
        "\n",
        "Runs a coding-agent task and records the outcome in the dogfood ledger."
    )
}

fn dogfood_external_fixture_help() -> &'static str {
    concat!(
        "DeepSeekCode dogfood external-fixture\n",
        "\n",
        "Usage:\n",
        "  deepseek dogfood external-fixture --workdir <path> [--budget <1..200>] [--benchmark-gate] [--evidence-out <path>] [--dry-run] [--allow-offline] [--notes <text>] \"<task>\"\n",
        "\n",
        "Runs an isolated write fixture from an external repository workdir. Real\n",
        "external fixture evidence requires online model-backed transport by default;\n",
        "`--allow-offline` is only for rehearsal runs that will not satisfy release gates.\n",
        "`--evidence-out` writes a JSON summary with appended external-fixture rows and\n",
        "the dogfood ledger fingerprint for release evidence upload."
    )
}

fn dogfood_external_evidence_help() -> &'static str {
    concat!(
        "DeepSeekCode dogfood external-evidence\n",
        "\n",
        "Usage:\n",
        "  deepseek dogfood external-evidence --file <path> [--out <path>] [--require-successful-external-fixtures <n>] [--allow-incomplete] [--allow-offline] [--allow-ledger-mismatch] [--json]\n",
        "\n",
        "Verifies a `deepseek.dogfood.external_fixture_evidence.v1` summary from an\n",
        "external fixture run. Defaults fail closed: completed, online, at least one\n",
        "successful external write fixture, and matching current dogfood ledger\n",
        "fingerprint."
    )
}

fn dogfood_repair_cache_evidence_help() -> &'static str {
    concat!(
        "DeepSeekCode dogfood repair-cache-evidence\n",
        "\n",
        "Usage:\n",
        "  deepseek dogfood repair-cache-evidence [--out <path>] [--json]\n",
        "\n",
        "Generates deterministic runtime evidence for the DeepSeek-native repair/cache\n",
        "loop: a malformed DeepSeek tool-call trace that fails strict parsing, the\n",
        "same trace recovered through tool-call repair, prompt-layer events, and a\n",
        "before/after cache hit-rate comparison. No model call is made."
    )
}

fn dogfood_replay_help() -> &'static str {
    concat!(
        "DeepSeekCode dogfood replay-benchmark\n",
        "\n",
        "Usage:\n",
        "  deepseek dogfood replay-benchmark [--manifest <path>] [--category <name>] [--limit <1..200>] [--benchmark-gate]\n",
        "\n",
        "Replays selected benchmark cases through dogfood recording. This can make real\n",
        "model calls when the configured provider is online."
    )
}

fn dogfood_live_plan_help() -> &'static str {
    concat!(
        "DeepSeekCode dogfood live-plan\n",
        "\n",
        "Usage:\n",
        "  deepseek dogfood live-plan [--manifest <path>] [--target-live-runs <n>] [--target-live-success-rate <percent>] [--target-category <category>:<min-runs>:<min-success-percent>] [--limit <n>] [--json]\n",
        "\n",
        "Shows a zero-side-effect plan for collecting model-backed dogfood evidence."
    )
}

fn dogfood_live_run_help() -> &'static str {
    concat!(
        "DeepSeekCode dogfood live-run\n",
        "\n",
        "Usage:\n",
        "  deepseek dogfood live-run [--manifest <path>] [--api-key-file <path>] [--evidence-out <path>] [--category <name>] [--target-live-runs <n>] [--target-live-success-rate <percent>] [--target-category <category>:<min-runs>:<min-success-percent>] [--limit <n>] [--json] [--execute] [--benchmark-gate]\n",
        "\n",
        "Selects the next cases from the live dogfood plan. The default is a dry run;\n",
        "`--json` emits a machine-readable dry-run plan. Add --execute, without\n",
        "--json, to run online model-backed benchmark replays. Add --evidence-out\n",
        "with --execute to write a machine-readable batch evidence summary."
    )
}

fn dogfood_live_evidence_help() -> &'static str {
    concat!(
        "DeepSeekCode dogfood live-evidence\n",
        "\n",
        "Usage:\n",
        "  deepseek dogfood live-evidence --file <path> [--out <path>] [--require-appended-model-backed <n>] [--require-benchmark-gate] [--require-report-gate] [--require-loop-surface-gate] [--allow-incomplete] [--allow-offline] [--json]\n",
        "\n",
        "Verifies a `deepseek.dogfood.live_run_evidence.v1` batch summary. Defaults\n",
        "fail closed: completed, online, and at least one appended model-backed row.\n",
        "`--require-report-gate` validates evidence_gate, live recency, ledger fingerprint, and row matches.\n",
        "`--out` writes the verification JSON as a release evidence artifact."
    )
}

fn dogfood_report_help() -> &'static str {
    concat!(
        "DeepSeekCode dogfood report\n",
        "\n",
        "Usage:\n",
        "  deepseek dogfood report [--out <path>] [--limit <n>] [--require-min-runs <n>] [--require-success-rate <percent>] [--require-live-runs <n>] [--require-live-success-rate <percent>] [--require-live-recent-days <days>] [--require-external-write-fixtures <n>] [--require-recent-clean <n>] [--require-category <category>:<min-runs>:<min-success-percent>] [--require-live-category <category>:<min-runs>:<min-success-percent>]\n",
        "\n",
        "Renders dogfood ledger stats and optionally enforces release gates."
    )
}

fn dogfood_export_help() -> &'static str {
    concat!(
        "DeepSeekCode dogfood export-benchmark\n",
        "\n",
        "Usage:\n",
        "  deepseek dogfood export-benchmark [--out <path>] [--limit <n>] [--outcome success|failed|stuck|manual]\n",
        "\n",
        "Exports eligible dogfood records as benchmark seed candidates."
    )
}

fn dogfood_promote_help() -> &'static str {
    concat!(
        "DeepSeekCode dogfood promote-benchmark\n",
        "\n",
        "Usage:\n",
        "  deepseek dogfood promote-benchmark [--manifest <path>] [--limit <n>] [--outcome success|failed|stuck|manual] [--dry-run]\n",
        "\n",
        "Promotes eligible dogfood records into the benchmark manifest."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_help_mentions_interactive_entrypoint() {
        let help = render_help(&[]);
        assert!(help.contains("deepseek"));
        assert!(help.contains("full-screen terminal workbench"));
        assert!(help.contains("deepseek chat"));
        assert!(help.contains("deepseek tui"));
    }

    #[test]
    fn dogfood_replay_help_warns_about_model_calls() {
        let topics = vec!["dogfood".to_string(), "replay-benchmark".to_string()];
        let help = render_help(&topics);
        assert!(help.contains("dogfood replay-benchmark"));
        assert!(help.contains("real\nmodel calls") || help.contains("real model calls"));
    }

    #[test]
    fn dogfood_report_help_documents_release_gate_flags() {
        let topics = vec!["dogfood".to_string(), "report".to_string()];
        let help = render_help(&topics);
        assert!(help.contains("--require-external-write-fixtures <n>"));
        assert!(help.contains("--require-recent-clean <n>"));
        assert!(help.contains("--require-live-category"));
    }

    #[test]
    fn dogfood_external_evidence_help_documents_release_verifier() {
        let topics = vec!["dogfood".to_string(), "external-evidence".to_string()];
        let help = render_help(&topics);
        assert!(help.contains("dogfood external-evidence --file <path>"));
        assert!(help.contains("--require-successful-external-fixtures <n>"));
        assert!(help.contains("--allow-ledger-mismatch"));
    }

    #[test]
    fn run_and_exec_help_document_model_routing_flags() {
        let run_help = render_help(&["run".to_string()]);
        assert!(run_help.contains("--preset <auto|flash|pro>"));
        assert!(run_help.contains("--pro-next"));

        let exec_help = render_help(&["exec".to_string()]);
        assert!(exec_help.contains("deepseek exec [--skill <name>]"));
        assert!(exec_help.contains("--preset <auto|flash|pro>"));
        assert!(exec_help.contains("--pro-next"));
    }

    #[test]
    fn config_help_documents_first_run_and_routing_surfaces() {
        let help = render_help(&["config".to_string()]);
        assert!(help.contains("config provider [show|list|<name> [model]]"));
        assert!(help.contains("config model [show|list|<model>]"));
        assert!(help.contains("config preset [show|auto|flash|pro]"));
        assert!(help.contains("config budget [show|off|MICROUSD|raise MICROUSD|+MICROUSD]"));
        assert!(help.contains("config auth [ENV] --stdin"));
    }

    #[test]
    fn stats_help_documents_session_title_selector() {
        let help = render_help(&["stats".to_string()]);
        assert!(help.contains("--session <id|name>"));
        assert!(help.contains("--require-prefix-stable"));
    }

    #[test]
    fn benchmark_help_documents_targeted_filters() {
        let topics = vec!["benchmark".to_string()];
        let help = render_help(&topics);
        assert!(help.contains("deepseek benchmark"));
        assert!(help.contains("--category <name>"));
        assert!(help.contains("--case <name>"));
        assert!(
            help.contains("do not\nadvance benchmark history")
                || help.contains("do not advance benchmark history")
        );
    }

    #[test]
    fn github_help_documents_action_outputs() {
        let topics = vec!["github".to_string()];
        let help = render_help(&topics);
        assert!(help.contains("github action"));
        assert!(help.contains("github pr-head"));
        assert!(help.contains("github fixture-smoke"));
        assert!(help.contains("--github-output"));
        assert!(help.contains("--require-mode"));
    }

    #[test]
    fn mcp_help_documents_fixture_smoke() {
        let topics = vec!["mcp".to_string()];
        let help = render_help(&topics);
        assert!(help.contains("mcp fixture-smoke"));
        assert!(help.contains("stdio, HTTP, and SSE"));
        assert!(help.contains("mcp__server__tool"));
    }

    #[test]
    fn hooks_help_documents_fixture_smoke() {
        let topics = vec!["hooks".to_string()];
        let help = render_help(&topics);
        assert!(help.contains("hooks fixture-smoke"));
        assert!(help.contains("session_start"));
        assert!(help.contains("post_tool_use"));
    }

    #[test]
    fn skills_help_documents_validation_gate() {
        let topics = vec!["skills".to_string()];
        let help = render_help(&topics);
        assert!(help.contains("skills validate"));
        assert!(help.contains("--strict"));
        assert!(help.contains("--dir"));
    }
}
