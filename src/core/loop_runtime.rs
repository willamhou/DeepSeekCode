use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
#[cfg(test)]
use std::sync::{Mutex, OnceLock};
use std::thread;
#[cfg(test)]
use std::time::Duration;
use std::time::Instant;

use crate::config::types::AppConfig;
use crate::core::context::TaskContext;
use crate::core::memory::MemoryState;
use crate::core::observations::{compact_observations, summarize_for_kind};
use crate::core::prompt_layers::{prompt_layers_for_request, PromptLayerSnapshot};
use crate::core::session::{SessionSnapshot, SessionStore};
use crate::error::{app_error, AppResult};
use crate::language::detect::detect_profile;
use crate::language::infer::default_test_command;
use crate::model::client::ModelClient;
use crate::model::deepseek::DeepSeekClient;
use crate::model::protocol::{
    ModelAction, ModelRequest, ModelResponse, Observation, ObservationKind, TokenUsage,
    ToolCallRequest,
};
use crate::skills::registry::SkillRegistry;
use crate::skills::resolver::{resolve_skill, SkillResolution};
use crate::skills::schema::SkillSpec;
use crate::tools::registry::{
    mcp_remote_tool_is_read_only, tool_metadata_for_name, ExecutionPolicy,
};
use crate::ui::render::print_banner;
use crate::ui::stream::StreamEvents;
use crate::util::cancel::CancellationCheck;
use crate::util::json::{json_value_to_string, JsonValue};

pub struct AgentLoopOptions {
    pub steps: usize,
    pub initial_observations: Vec<Observation>,
    pub initial_recent_steps: Vec<String>,
    pub todos: std::rc::Rc<std::cell::RefCell<crate::core::todos::TodoList>>,
    pub subagent_depth: usize,
    pub emit_progress: bool,
    pub persist_session: bool,
    pub stream_events: Option<Box<dyn crate::ui::stream::StreamEvents>>,
    pub run_events: Option<SharedAgentRunEvents>,
    pub approval_resolver: Option<SharedAgentApprovalResolver>,
    pub user_input_resolver: Option<SharedAgentUserInputResolver>,
    pub cancel_check: Option<SharedAgentCancelCheck>,
    pub session_budget: Option<AgentSessionBudget>,
}

impl Default for AgentLoopOptions {
    fn default() -> Self {
        Self {
            steps: 4,
            initial_observations: Vec::new(),
            initial_recent_steps: Vec::new(),
            todos: std::rc::Rc::new(std::cell::RefCell::new(
                crate::core::todos::TodoList::default(),
            )),
            subagent_depth: 0,
            emit_progress: true,
            persist_session: true,
            stream_events: None,
            run_events: None,
            approval_resolver: None,
            user_input_resolver: None,
            cancel_check: None,
            session_budget: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentSessionBudget {
    pub budget_microusd: u64,
    pub used_microusd: u64,
}

#[derive(Debug, Clone)]
pub struct ToolEvent {
    pub tool_name: String,
    pub input: BTreeMap<String, String>,
    pub output: String,
    pub status: crate::model::protocol::ObservationStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRouteEvent {
    pub preset: String,
    pub model: String,
    pub reason: String,
    pub escalated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRepairEvent {
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default)]
pub struct RunResult {
    pub final_message: String,
    pub tool_events: Vec<ToolEvent>,
    pub usage: crate::model::protocol::TokenUsage,
    pub prompt_layers: Vec<PromptLayerSnapshot>,
    pub model_routes: Vec<ModelRouteEvent>,
    pub tool_repairs: Vec<ToolRepairEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemPromptPreview {
    pub workspace: PathBuf,
    pub profile_name: String,
    pub task: Option<String>,
    pub prompt: String,
    pub available_tools: Vec<String>,
    pub planning_mode: bool,
    pub research_bootstrap: bool,
    pub skill_name: Option<String>,
    pub skill_resolution: Option<String>,
    pub workspace_instruction_paths: Vec<PathBuf>,
    pub user_memory_path: Option<PathBuf>,
    pub user_memory_truncated: bool,
}

pub fn preview_system_prompt_for_workspace(
    config: &AppConfig,
    workspace: &Path,
    task: Option<&str>,
    has_plan: bool,
    subagent_depth: usize,
) -> AppResult<SystemPromptPreview> {
    let workspace = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    let workspace_str = workspace.to_string_lossy().to_string();
    let task = task
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let task_ref = task.as_deref().unwrap_or("");

    let profile = detect_profile(&workspace_str)?;
    let workspace_instructions =
        crate::core::instructions::load_workspace_instructions(&workspace, &config.workspace)?;
    let user_memory =
        crate::core::memory::load_user_memory(config.memory.enabled, &config.memory.memory_path())?;
    let todos = Rc::new(RefCell::new(crate::core::todos::TodoList::default()));
    let registry = crate::tools::registry::default_registry_with_context(
        config.clone(),
        subagent_depth,
        todos,
    );
    let user_skills_dir = crate::skills::tilde::expand_tilde(&config.workspace.user_skills_dir);
    let repo_skills_dir = crate::skills::paths::resolve_repo_skills_dir();
    let (skills, _stats) =
        SkillRegistry::load_dirs(&[repo_skills_dir.as_path(), user_skills_dir.as_path()])?;
    let resolved_skill = if task_ref.is_empty() {
        None
    } else {
        resolve_skill(&skills, None, task_ref)
    };
    let skill = resolved_skill.as_ref().map(|resolved| resolved.spec);
    let policy = ExecutionPolicy::with_network(&config.approval, &config.network, skill);
    let available_tools = registry
        .names_for_policy(&policy)
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let research_bootstrap = !task_ref.is_empty()
        && should_apply_research_bootstrap(task_ref, workspace.as_path(), &available_tools);
    let planning_mode = !task_ref.is_empty()
        && !research_bootstrap
        && should_use_explicit_planning(task_ref, skill, &available_tools);
    let subagent_available = available_tools
        .iter()
        .any(|tool| tool == "dispatch_subagent" || tool == "dispatch_subagents");
    let prompt = build_system_prompt_with_workspace_instructions(
        skill,
        research_bootstrap,
        planning_mode,
        has_plan,
        subagent_available,
        &workspace_instructions,
        user_memory.as_ref(),
    );

    Ok(SystemPromptPreview {
        workspace,
        profile_name: profile.name,
        task,
        prompt,
        available_tools,
        planning_mode,
        research_bootstrap,
        skill_name: skill.map(|spec| spec.name.clone()),
        skill_resolution: resolved_skill.map(|resolved| match resolved.resolution {
            SkillResolution::Explicit => "explicit".to_string(),
            SkillResolution::Auto => "auto".to_string(),
        }),
        workspace_instruction_paths: workspace_instructions
            .iter()
            .map(|file| file.path.clone())
            .collect(),
        user_memory_path: user_memory.as_ref().map(|memory| memory.path.clone()),
        user_memory_truncated: user_memory
            .as_ref()
            .map(|memory| memory.truncated)
            .unwrap_or(false),
    })
}

pub type SharedAgentRunEvents = Rc<RefCell<dyn AgentRunEvents>>;

pub trait AgentRunEvents {
    fn on_prompt_layers(&mut self, _snapshot: &PromptLayerSnapshot) {}

    fn on_tool_call(&mut self, tool_name: &str, input: &BTreeMap<String, String>);

    fn on_permission_request(
        &mut self,
        tool_name: &str,
        input: &BTreeMap<String, String>,
        kind: &str,
        target: &str,
    );

    fn on_tool_result(&mut self, event: &ToolEvent);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentApprovalRequest {
    pub tool_name: String,
    pub input: BTreeMap<String, String>,
    pub kind: String,
    pub target: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentApprovalDecision {
    Approved,
    Denied,
}

pub type SharedAgentApprovalResolver = Rc<RefCell<dyn AgentApprovalResolver>>;

pub trait AgentApprovalResolver {
    fn resolve(&mut self, request: &AgentApprovalRequest) -> AppResult<AgentApprovalDecision>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentUserInputRequest {
    pub input: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentUserInputResponse {
    pub answers: BTreeMap<String, String>,
}

pub type SharedAgentUserInputResolver = Rc<RefCell<dyn AgentUserInputResolver>>;

pub trait AgentUserInputResolver {
    fn resolve(&mut self, request: &AgentUserInputRequest) -> AppResult<AgentUserInputResponse>;
}

pub type SharedAgentCancelCheck = Rc<RefCell<dyn AgentCancelCheck>>;

pub trait AgentCancelCheck {
    fn is_cancelled(&mut self) -> AppResult<bool>;
}

struct AgentCancelAdapter<'a> {
    inner: &'a mut dyn AgentCancelCheck,
}

impl CancellationCheck for AgentCancelAdapter<'_> {
    fn is_cancelled(&mut self) -> AppResult<bool> {
        self.inner.is_cancelled()
    }
}

pub struct AgentLoop {
    config: AppConfig,
}

impl AgentLoop {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }

    pub fn run(&self, context: TaskContext) -> AppResult<()> {
        self.run_with(context, AgentLoopOptions::default())
            .map(|_| ())
    }

    pub fn run_with(
        &self,
        context: TaskContext,
        options: AgentLoopOptions,
    ) -> AppResult<RunResult> {
        let client = DeepSeekClient {
            config: self.config.model.clone(),
        };
        self.run_with_client(context, options, &client)
    }

    pub fn run_with_client<C: ModelClient>(
        &self,
        context: TaskContext,
        options: AgentLoopOptions,
        client: &C,
    ) -> AppResult<RunResult> {
        let AgentLoopOptions {
            steps,
            initial_observations,
            initial_recent_steps,
            todos,
            subagent_depth,
            emit_progress,
            persist_session,
            mut stream_events,
            run_events,
            approval_resolver,
            user_input_resolver,
            cancel_check,
            session_budget,
        } = options;
        if emit_progress {
            print_banner("DeepSeekCode");
        }

        let profile = detect_profile(".")?;
        let cwd = std::env::current_dir()?;
        let workspace_instructions =
            crate::core::instructions::load_workspace_instructions(&cwd, &self.config.workspace)?;
        let user_memory = crate::core::memory::load_user_memory(
            self.config.memory.enabled,
            &self.config.memory.memory_path(),
        )?;
        let hooks = crate::core::hooks::HookRunner::new(&self.config.hooks);
        let registry = crate::tools::registry::default_registry_with_context(
            self.config.clone(),
            subagent_depth,
            todos.clone(),
        );
        let user_skills_dir =
            crate::skills::tilde::expand_tilde(&self.config.workspace.user_skills_dir);
        let repo_skills_dir = crate::skills::paths::resolve_repo_skills_dir();
        let (skills, _stats) =
            SkillRegistry::load_dirs(&[repo_skills_dir.as_path(), user_skills_dir.as_path()])?;
        let resolved_skill = resolve_skill(&skills, context.skill.as_deref(), &context.task);
        let skill = resolved_skill.map(|resolved| resolved.spec);
        let policy =
            ExecutionPolicy::with_network(&self.config.approval, &self.config.network, skill);
        let memory = MemoryState::new(profile.name.clone());
        let primary_file = primary_file(&profile).map(str::to_string);
        let suggested_test_command = default_test_command(&profile).map(str::to_string);
        if let Some(skill) = skill {
            if todos.borrow().is_empty() && !skill.initial_todos.is_empty() {
                let seeded = skill
                    .initial_todos
                    .iter()
                    .map(crate::skills::schema::TodoSeed::to_todo)
                    .collect::<Vec<_>>();
                let seeded_count = seeded.len();
                todos.borrow_mut().replace(seeded);
                if emit_progress {
                    println!("Seeded todos from skill: {seeded_count}");
                }
            }
        }

        if emit_progress {
            println!("Task: {}", context.task);
            println!("Profile: {}", profile.name);
            if !profile.hints.is_empty() {
                println!("Profile hints:");
                for hint in &profile.hints {
                    println!("- {hint}");
                }
            }
        }
        let available_tools = registry
            .names_for_policy(&policy)
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let research_bootstrap =
            should_apply_research_bootstrap(&context.task, Path::new("."), &available_tools);
        let planning_mode = !research_bootstrap
            && should_use_explicit_planning(&context.task, skill, &available_tools);

        if emit_progress {
            println!("Available tools: {}", available_tools.join(", "));
            if planning_mode {
                println!("Planning mode: explicit");
            }
        }

        if let Some(skill) = skill {
            if emit_progress {
                println!("Skill: {}", skill.name);
                if let Some(resolved) = resolved_skill {
                    match resolved.resolution {
                        SkillResolution::Explicit => println!("Skill source: explicit"),
                        SkillResolution::Auto => println!("Skill source: auto (trigger match)"),
                    }
                }
                println!("Skill description: {}", skill.description);
                if !skill.suggested_steps.is_empty() {
                    println!("Suggested steps:");
                    for step in &skill.suggested_steps {
                        println!("- {}", step);
                    }
                }
                if !skill.references.is_empty() {
                    println!("References:");
                    for reference in &skill.references {
                        println!("- {}", reference);
                    }
                }
            }
        }

        if emit_progress {
            println!("Memory summary: {}", memory.summary());
            if !workspace_instructions.is_empty() {
                println!("Workspace instructions:");
                for file in &workspace_instructions {
                    let suffix = if file.truncated { " (truncated)" } else { "" };
                    println!("- {}{}", file.path.display(), suffix);
                }
            }
            if let Some(memory) = &user_memory {
                let suffix = if memory.truncated { " (truncated)" } else { "" };
                println!("User memory: {}{}", memory.path.display(), suffix);
            }
        }

        let mut observations = initial_observations;
        if let Some(hook_context) = hooks.session_start(&context.task, "startup")? {
            observations.push(Observation::ok(
                "hook",
                format!("session_start: {hook_context}"),
            ));
        }
        if let Some(hook_context) = hooks.user_prompt_submit(&context.task)? {
            observations.push(Observation::ok(
                "hook",
                format!("user_prompt_submit: {hook_context}"),
            ));
        }
        let mut last_message = String::new();
        let mut tool_events: Vec<ToolEvent> = Vec::new();
        let mut model_route_events: Vec<ModelRouteEvent> = Vec::new();
        let mut tool_repair_events: Vec<ToolRepairEvent> = Vec::new();
        let mut total_usage = crate::model::protocol::TokenUsage::default();
        let session_budget_microusd = session_budget
            .map(|budget| budget.budget_microusd)
            .unwrap_or(self.config.model.session_budget_microusd);
        let mut estimated_session_cost_microusd = session_budget
            .map(|budget| budget.used_microusd)
            .unwrap_or(0);
        let mut session_budget_warned = false;
        let mut prompt_layer_snapshots = Vec::new();
        let mut renderer = emit_progress.then(crate::ui::stream::TtyRenderer::from_stdout);
        let mut noop_events = crate::ui::stream::NoopStreamEvents;
        // Phase 10c-1: accumulate prior assistant messages and compact reasoning
        // summaries so each step sees what it already considered. Without this,
        // dscode run loops on "I'll start by …" because the LLM never sees its own
        // progress (REPL has Repl.transcript; one-shot did not).
        const RECENT_STEPS_KEEP: usize = 3;
        let mut recent_steps_log = initial_recent_steps
            .into_iter()
            .filter(|entry| !entry.trim().is_empty())
            .rev()
            .take(RECENT_STEPS_KEEP)
            .collect::<Vec<_>>();
        recent_steps_log.reverse();
        // Phase 10c-2: repeat-call detection. Track fingerprints of the last
        // REPEAT_WINDOW tool calls. Read-only tools get one retry with a
        // stuck-warning, then short-circuit on the 3rd identical call. Mutating or
        // unknown tools short-circuit on the 2nd identical call before execution so
        // model retries cannot duplicate writes, shell actions, MCP calls, or task
        // mutations.
        let mut recent_call_fingerprints: Vec<String> = Vec::new();
        // Fix A: parallel track of inspection targets (path/query), so re-reads
        // that vary only by size caps still count as repeats.
        let mut recent_inspection_fingerprints: Vec<String> = Vec::new();
        const REPEAT_WINDOW: usize = 3;
        // Fix B: force a decision once the model has inspected this many tool
        // steps in a row without making any edit.
        const INSPECTION_WITHOUT_EDIT_LIMIT: usize = 4;
        let mut steps_without_edit: usize = 0;
        let mut emitted_stuck_directive = false;
        for step in 0..steps {
            check_cancelled(cancel_check.as_ref())?;
            if session_budget_microusd > 0 {
                if estimated_session_cost_microusd >= session_budget_microusd {
                    return Err(app_error(format!(
                        "session budget exhausted: {estimated_session_cost_microusd}/{session_budget_microusd} microusd used; run `deepseek config budget raise <MICROUSD>` to raise it or `deepseek config budget off` to disable"
                    )));
                }
                if !session_budget_warned
                    && estimated_session_cost_microusd.saturating_mul(100)
                        >= session_budget_microusd.saturating_mul(80)
                {
                    if let Some(events) = stream_events.as_deref_mut() {
                        events.on_model_budget_warning(
                            estimated_session_cost_microusd,
                            session_budget_microusd,
                        );
                    } else if let Some(renderer) = renderer.as_mut() {
                        renderer.on_model_budget_warning(
                            estimated_session_cost_microusd,
                            session_budget_microusd,
                        );
                    }
                    session_budget_warned = true;
                }
            }
            let recent_window = recent_steps_log
                .iter()
                .rev()
                .take(RECENT_STEPS_KEEP)
                .rev()
                .cloned()
                .collect::<Vec<_>>();
            let todo_snapshot = todos.borrow().snapshot();
            let mut system_prompt = build_system_prompt_with_workspace_instructions(
                skill,
                research_bootstrap,
                planning_mode,
                !todo_snapshot.is_empty(),
                available_tools
                    .iter()
                    .any(|tool| tool == "dispatch_subagent" || tool == "dispatch_subagents"),
                &workspace_instructions,
                user_memory.as_ref(),
            );
            if let Some(target_language) = context.translation_target_language.as_deref() {
                append_translation_output_instruction(&mut system_prompt, target_language);
            }
            let request = ModelRequest {
                system_prompt,
                task: context.task.clone(),
                image_inputs: context.image_inputs.clone(),
                profile_name: profile.name.clone(),
                profile_hints: profile.hints.clone(),
                primary_file: primary_file.clone(),
                suggested_test_command: suggested_test_command.clone(),
                available_tools: available_tools.clone(),
                observations: compact_observations(&observations),
                todos: todo_snapshot,
                planning_mode,
                recent_steps: recent_window,
            };
            let prompt_layers = prompt_layers_for_request(step + 1, &request);
            if let Some(events) = run_events.as_ref() {
                events.borrow_mut().on_prompt_layers(&prompt_layers);
            }
            prompt_layer_snapshots.push(prompt_layers);

            if let Some(renderer) = renderer.as_mut() {
                renderer.paint_step_divider(step + 1);
            }
            let model_outcome = if let Some(events) = stream_events.as_deref_mut() {
                let mut capture = ReasoningCaptureEvents::new(events);
                let outcome =
                    model_respond_with_cancel(client, request, &mut capture, cancel_check.as_ref());
                let (reasoning, routes, repairs) = capture.into_parts();
                outcome.map(|outcome| (outcome.0, outcome.1, reasoning, routes, repairs))
            } else if let Some(renderer) = renderer.as_mut() {
                let mut capture = ReasoningCaptureEvents::new(renderer);
                let outcome =
                    model_respond_with_cancel(client, request, &mut capture, cancel_check.as_ref());
                let (reasoning, routes, repairs) = capture.into_parts();
                outcome.map(|outcome| (outcome.0, outcome.1, reasoning, routes, repairs))
            } else {
                let mut capture = ReasoningCaptureEvents::new(&mut noop_events);
                let outcome =
                    model_respond_with_cancel(client, request, &mut capture, cancel_check.as_ref());
                let (reasoning, routes, repairs) = capture.into_parts();
                outcome.map(|outcome| (outcome.0, outcome.1, reasoning, routes, repairs))
            };
            let (response, step_usage, step_reasoning, step_model_routes, step_tool_repairs) =
                match model_outcome {
                    Ok(outcome) => outcome,
                    Err(error) if is_recoverable_model_tool_call_parse_error(error.as_ref()) => {
                        let observation = model_tool_call_parse_failure_observation(error.as_ref());
                        if let Some(renderer) = renderer.as_mut() {
                            renderer.paint_tool_result(
                                crate::ui::stream::ToolResultKind::Failed,
                                "model",
                                "tool-call-parse",
                                &observation,
                            );
                        }
                        observations.push(Observation::failed("model", observation.clone()));
                        last_message = observation.clone();
                        recent_steps_log.push(format!("model response failed: {observation}"));
                        continue;
                    }
                    Err(error) => return Err(error),
                };
            model_route_events.extend(step_model_routes);
            tool_repair_events.extend(step_tool_repairs);
            if let Some(usage) = step_usage {
                if let Some(cost) = crate::core::runtime::estimate_token_usage_cost_microusd(
                    &self.config.model.model,
                    &usage,
                ) {
                    estimated_session_cost_microusd =
                        estimated_session_cost_microusd.saturating_add(cost);
                }
                total_usage.add_assign(&usage);
            }
            check_cancelled(cancel_check.as_ref())?;
            last_message = response.message.clone();
            if let Some(entry) = recent_step_replay_entry(&response.message, &step_reasoning) {
                recent_steps_log.push(entry);
            }

            let tool_calls = match response.action {
                ModelAction::CallTool { tool_name, input } => {
                    vec![ToolCallRequest { tool_name, input }]
                }
                ModelAction::CallTools(calls) => calls,
                ModelAction::Finish => {
                    break;
                }
            };

            let mut tool_call_index = 0;
            let mut step_attempted_tool = false;
            let mut step_executed_edit = false;
            while tool_call_index < tool_calls.len() {
                if let Some(consumed) = maybe_execute_parallel_safe_chunk(
                    &tool_calls[tool_call_index..],
                    &registry,
                    &policy,
                    &self.config,
                    self.config.hooks.enabled,
                    &available_tools,
                    primary_file.as_deref(),
                    &mut renderer,
                    run_events.as_ref(),
                    cancel_check.as_ref(),
                    &mut observations,
                    &mut tool_events,
                    &mut recent_call_fingerprints,
                    &mut recent_inspection_fingerprints,
                    REPEAT_WINDOW,
                )? {
                    step_attempted_tool = true;
                    tool_call_index += consumed;
                    continue;
                }

                let ToolCallRequest {
                    mut tool_name,
                    mut input,
                } = tool_calls[tool_call_index].clone();
                tool_call_index += 1;
                step_attempted_tool = true;
                check_cancelled(cancel_check.as_ref())?;
                let mut event_input = input.args.clone();
                let mut fingerprint = tool_call_fingerprint(&tool_name, &event_input);
                let mut same_count_in_window = recent_call_fingerprints
                    .iter()
                    .rev()
                    .take(REPEAT_WINDOW)
                    .filter(|fp| **fp == fingerprint)
                    .count();
                if let Some(rewritten) = maybe_rewrite_repeated_mcp_resource_list_call(
                    &tool_name,
                    &event_input,
                    same_count_in_window,
                    &observations,
                    &available_tools,
                ) {
                    tool_name = rewritten.tool_name;
                    input = rewritten.input;
                    event_input = input.args.clone();
                    fingerprint = tool_call_fingerprint(&tool_name, &event_input);
                    same_count_in_window = recent_call_fingerprints
                        .iter()
                        .rev()
                        .take(REPEAT_WINDOW)
                        .filter(|fp| **fp == fingerprint)
                        .count();
                }
                // Fix A: read_file/search_text/list_files have no advancing
                // cursor, so re-issuing the same target with a different
                // max_lines/limit returns a prefix of identical content. The
                // exact-args fingerprint misses this (a different max_lines looks
                // "new"), so also count repeats by inspection target.
                let inspection_target = read_inspection_target(&tool_name, &event_input);
                let inspection_count_in_window = inspection_target
                    .as_ref()
                    .map(|target| {
                        recent_inspection_fingerprints
                            .iter()
                            .rev()
                            .take(REPEAT_WINDOW)
                            .filter(|fp| *fp == target)
                            .count()
                    })
                    .unwrap_or(0);
                let effective_repeat_count = same_count_in_window.max(inspection_count_in_window);

                emit_tool_call(run_events.as_ref(), &tool_name, &event_input);

                // Phase 10c-2: compute fingerprint and check window BEFORE executing.
                recent_call_fingerprints.push(fingerprint.clone());
                // Trim to keep memory bounded over long runs (only the last
                // REPEAT_WINDOW are ever read).
                trim_recent_call_fingerprints(&mut recent_call_fingerprints, REPEAT_WINDOW);
                if let Some(target) = &inspection_target {
                    recent_inspection_fingerprints.push(target.clone());
                    trim_recent_call_fingerprints(
                        &mut recent_inspection_fingerprints,
                        REPEAT_WINDOW,
                    );
                }

                let repeat_threshold = repeat_short_circuit_threshold(&tool_name, &event_input);
                if effective_repeat_count >= repeat_threshold {
                    let stuck_msg = repeat_short_circuit_message(
                        &tool_name,
                        &event_input,
                        effective_repeat_count + 1,
                        REPEAT_WINDOW,
                    );
                    if let Some(renderer) = renderer.as_mut() {
                        renderer.paint_tool_result(
                            crate::ui::stream::ToolResultKind::Failed,
                            &tool_name,
                            "stuck",
                            &stuck_msg,
                        );
                    }
                    let event_name = tool_name.clone();
                    observations.push(Observation::failed(tool_name, stuck_msg.clone()));
                    push_tool_event(
                        &mut tool_events,
                        run_events.as_ref(),
                        ToolEvent {
                            tool_name: event_name,
                            input: event_input,
                            output: stuck_msg,
                            status: crate::model::protocol::ObservationStatus::Failed,
                        },
                    );
                    continue;
                }

                // Phase 10c-2: 2nd identical read-only call: emit a stuck-warning
                // Observation BEFORE running the tool. Avoids burying the warning in the
                // tail of a long tool output that head_trim / Todos summarize would eat,
                // and works for both Ok and Err result paths.
                if effective_repeat_count == 1 && repeat_threshold > 1 {
                    let warning = format!(
                            "⚠ stuck-warning: '{tool_name}' was called on the same target last step. If output is unchanged, try a DIFFERENT approach (apply_patch to make the fix, todo_write to plan, a different path/args, or move to the next step)."
                        );
                    observations.push(Observation::ok("stuck-warning", warning));
                }

                // After an edit attempt the file may change and a corrective
                // re-read is legitimate, so clear the repeat windows. Otherwise
                // Fix A would block the fresh read the model needs to fix a
                // non-matching patch anchor, trapping it between a failed patch
                // and a suppressed re-read.
                if is_edit_tool(&tool_name) {
                    recent_inspection_fingerprints.clear();
                    recent_call_fingerprints.clear();
                }

                match hooks.pre_tool_use(&context.task, &tool_name, &input) {
                    Ok(Some(hook_context)) => {
                        observations.push(Observation::ok(
                            "hook",
                            format!("pre_tool_use: {hook_context}"),
                        ));
                    }
                    Ok(None) => {}
                    Err(error) => {
                        let raw = error.to_string();
                        if let Some(renderer) = renderer.as_mut() {
                            renderer.paint_tool_result(
                                crate::ui::stream::ToolResultKind::Denied,
                                &tool_name,
                                "hook",
                                &raw,
                            );
                        }
                        let event_name = tool_name.clone();
                        observations.push(Observation::failed(
                            tool_name,
                            format!("pre_tool_use hook blocked tool: {raw}"),
                        ));
                        push_tool_event(
                            &mut tool_events,
                            run_events.as_ref(),
                            ToolEvent {
                                tool_name: event_name,
                                input: event_input,
                                output: raw,
                                status: crate::model::protocol::ObservationStatus::Failed,
                            },
                        );
                        continue;
                    }
                }

                let mut execution_policy = policy.clone();
                if let Some(permission) =
                    registry.permission_request_for(&tool_name, &input, &policy)
                {
                    emit_permission_request(
                        run_events.as_ref(),
                        &tool_name,
                        &event_input,
                        &permission.kind,
                        &permission.target,
                    );
                    match hooks.permission_request(
                        &context.task,
                        &tool_name,
                        &input,
                        &permission.kind,
                        &permission.target,
                    ) {
                        Ok(Some(hook_context)) => {
                            observations.push(Observation::ok(
                                "hook",
                                format!("permission_request: {hook_context}"),
                            ));
                        }
                        Ok(None) => {}
                        Err(error) => {
                            let raw = error.to_string();
                            if let Some(renderer) = renderer.as_mut() {
                                renderer.paint_tool_result(
                                    crate::ui::stream::ToolResultKind::Denied,
                                    &tool_name,
                                    "hook",
                                    &raw,
                                );
                            }
                            let event_name = tool_name.clone();
                            observations.push(Observation::failed(
                                tool_name,
                                format!("permission_request hook blocked tool: {raw}"),
                            ));
                            push_tool_event(
                                &mut tool_events,
                                run_events.as_ref(),
                                ToolEvent {
                                    tool_name: event_name,
                                    input: event_input,
                                    output: raw,
                                    status: crate::model::protocol::ObservationStatus::Failed,
                                },
                            );
                            continue;
                        }
                    }

                    if let Some(resolver) = approval_resolver.as_ref() {
                        let approval_request = AgentApprovalRequest {
                            tool_name: tool_name.clone(),
                            input: event_input.clone(),
                            kind: permission.kind.clone(),
                            target: permission.target.clone(),
                        };
                        match resolver.borrow_mut().resolve(&approval_request)? {
                            AgentApprovalDecision::Approved => {
                                execution_policy =
                                    policy.with_auto_approved_permission(&permission.kind);
                            }
                            AgentApprovalDecision::Denied => {
                                let raw = format!(
                                    "permission denied for {}: {}",
                                    permission.kind, permission.target
                                );
                                if let Some(renderer) = renderer.as_mut() {
                                    renderer.paint_tool_result(
                                        crate::ui::stream::ToolResultKind::Denied,
                                        &tool_name,
                                        &permission.kind,
                                        &raw,
                                    );
                                }
                                let event_name = tool_name.clone();
                                observations.push(Observation::failed(tool_name, raw.clone()));
                                push_tool_event(
                                    &mut tool_events,
                                    run_events.as_ref(),
                                    ToolEvent {
                                        tool_name: event_name,
                                        input: event_input,
                                        output: raw,
                                        status: crate::model::protocol::ObservationStatus::Failed,
                                    },
                                );
                                continue;
                            }
                        }
                    }
                }

                if shell_env_hook_applies_to(&tool_name) {
                    match hooks.shell_env(&context.task, &tool_name, &input) {
                        Ok(shell_env) => {
                            let mut applied_keys = Vec::new();
                            for (key, value) in shell_env.vars {
                                input.args.insert(format!("env.{key}"), value);
                                applied_keys.push(key);
                            }
                            if !applied_keys.is_empty() {
                                observations.push(Observation::ok(
                                    "hook",
                                    format!("shell_env applied keys: {}", applied_keys.join(", ")),
                                ));
                            }
                            if !shell_env.notices.is_empty() {
                                observations.push(Observation::ok(
                                    "hook",
                                    format!("shell_env: {}", shell_env.notices.join("; ")),
                                ));
                            }
                        }
                        Err(error) => {
                            observations.push(Observation::ok(
                                "hook",
                                format!("shell_env hook skipped: {error}"),
                            ));
                        }
                    }
                }

                let tool_result = if tool_name == "request_user_input" {
                    if let Some(resolver) = user_input_resolver.as_ref() {
                        match execute_tool_with_cancel(
                            &registry,
                            &tool_name,
                            input.clone(),
                            &execution_policy,
                            cancel_check.as_ref(),
                        ) {
                            Ok(_) => {
                                let request = AgentUserInputRequest {
                                    input: event_input.clone(),
                                };
                                let response = resolver.borrow_mut().resolve(&request)?;
                                Ok(crate::tools::types::ToolOutput {
                                    summary: render_user_input_answers(&response.answers),
                                })
                            }
                            Err(error) => Err(error),
                        }
                    } else {
                        execute_tool_with_cancel(
                            &registry,
                            &tool_name,
                            input,
                            &execution_policy,
                            cancel_check.as_ref(),
                        )
                    }
                } else {
                    execute_tool_with_cancel(
                        &registry,
                        &tool_name,
                        input,
                        &execution_policy,
                        cancel_check.as_ref(),
                    )
                };

                match tool_result {
                    Ok(mut output) => {
                        check_cancelled(cancel_check.as_ref())?;
                        if is_edit_tool(&tool_name) {
                            step_executed_edit = true;
                        }
                        if tool_name == "dispatch_subagent" {
                            if let Some(delegated_task) = event_input.get("task") {
                                if todos
                                    .borrow_mut()
                                    .complete_in_progress_matching_subagent_task(delegated_task)
                                {
                                    output.summary.push_str(
                                        "\nparent todos auto-advanced after subagent completion",
                                    );
                                }
                            }
                        }
                        output.summary =
                            crate::tools::tool_output::maybe_spill_successful_tool_output(
                                &tool_name,
                                &output.summary,
                            );
                        let kind = ObservationKind::from_tool_name(&tool_name);
                        let observation_summary = summarize_for_kind(&output.summary, kind);
                        // CR-1: user sees full body (output.summary), observation/transcript get trim.
                        if let Some(renderer) = renderer.as_mut() {
                            renderer.paint_tool_result(
                                crate::ui::stream::ToolResultKind::Ok,
                                &tool_name,
                                kind.label(),
                                &output.summary,
                            );
                        }
                        let event_name = tool_name.clone();
                        observations.push(Observation::ok(tool_name, observation_summary.clone()));
                        if let Some(recovery_hint) = derive_recovery_hint_after_success(
                            &event_name,
                            &output.summary,
                            &available_tools,
                            primary_file.as_deref(),
                            &observations,
                        ) {
                            observations.push(Observation::ok("recovery_hint", recovery_hint));
                        }
                        if let Some(replan_hint) =
                            derive_replan_hint(&event_name, &output.summary, &observations)
                        {
                            observations.push(Observation::ok("replan_hint", replan_hint));
                        }
                        push_tool_event(
                            &mut tool_events,
                            run_events.as_ref(),
                            ToolEvent {
                                tool_name: event_name,
                                input: event_input,
                                output: output.summary,
                                status: crate::model::protocol::ObservationStatus::Ok,
                            },
                        );
                        push_post_tool_hook_observation(
                            &hooks,
                            &context.task,
                            &tool_events,
                            &mut observations,
                        );
                    }
                    Err(error) => {
                        check_cancelled(cancel_check.as_ref())?;
                        let kind = ObservationKind::from_tool_name(&tool_name);
                        let raw = error.to_string();
                        let observation_summary = summarize_for_kind(&raw, kind);
                        let result_kind = match crate::error::classify(error.as_ref()) {
                            crate::error::AppErrorKind::PolicyDenied => {
                                crate::ui::stream::ToolResultKind::Denied
                            }
                            _ => crate::ui::stream::ToolResultKind::Failed,
                        };
                        // CR-1: user sees full error text, observation/transcript get trim.
                        if let Some(renderer) = renderer.as_mut() {
                            renderer.paint_tool_result(result_kind, &tool_name, kind.label(), &raw);
                        }
                        let event_name = tool_name.clone();
                        observations
                            .push(Observation::failed(tool_name, observation_summary.clone()));
                        if let Some(recovery_hint) = derive_recovery_hint_after_failure(
                            &event_name,
                            &available_tools,
                            primary_file.as_deref(),
                            &observations,
                        ) {
                            observations.push(Observation::ok("recovery_hint", recovery_hint));
                        }
                        if let Some(replan_hint) =
                            derive_replan_hint(&event_name, &observation_summary, &observations)
                        {
                            observations.push(Observation::ok("replan_hint", replan_hint));
                        }
                        push_tool_event(
                            &mut tool_events,
                            run_events.as_ref(),
                            ToolEvent {
                                tool_name: event_name,
                                input: event_input,
                                output: raw,
                                status: crate::model::protocol::ObservationStatus::Failed,
                            },
                        );
                        push_post_tool_hook_observation(
                            &hooks,
                            &context.task,
                            &tool_events,
                            &mut observations,
                        );
                    }
                }
            }

            // Fix B: after sustained inspection with no edit, force a decision.
            // Emitted as a failure observation so compaction never supersedes it.
            if step_executed_edit {
                steps_without_edit = 0;
                emitted_stuck_directive = false;
            } else if step_attempted_tool {
                steps_without_edit += 1;
            }
            if steps_without_edit >= INSPECTION_WITHOUT_EDIT_LIMIT && !emitted_stuck_directive {
                let directive = format!(
                    "⛔ stuck-directive: {steps_without_edit} tool steps have run with no edit. You already have enough context — make the change now with apply_patch (or write_file), or finish and state why no edit is needed. Do NOT read or search the same target again."
                );
                if let Some(renderer) = renderer.as_mut() {
                    renderer.paint_tool_result(
                        crate::ui::stream::ToolResultKind::Failed,
                        "stuck-directive",
                        "stuck",
                        &directive,
                    );
                }
                observations.push(Observation::failed("stuck-directive", directive));
                emitted_stuck_directive = true;
            }
        }

        if emit_progress {
            if let Some(test_command) = suggested_test_command.as_deref() {
                println!();
                println!("Suggested validation command: {test_command}");
            }
        }

        let _ = hooks.session_stop(&context.task, "finish", &last_message)?;

        if persist_session {
            let store = SessionStore::new(self.config.workspace.session_dir());
            let snapshot = SessionSnapshot::new(context.task, profile.name);
            store.save(&snapshot)?;
        }

        Ok(RunResult {
            final_message: last_message,
            tool_events,
            usage: total_usage,
            prompt_layers: prompt_layer_snapshots,
            model_routes: model_route_events,
            tool_repairs: tool_repair_events,
        })
    }
}

struct ReasoningCaptureEvents<'a> {
    inner: &'a mut dyn StreamEvents,
    reasoning: String,
    model_routes: Vec<ModelRouteEvent>,
    tool_repairs: Vec<ToolRepairEvent>,
}

impl<'a> ReasoningCaptureEvents<'a> {
    fn new(inner: &'a mut dyn StreamEvents) -> Self {
        Self {
            inner,
            reasoning: String::new(),
            model_routes: Vec::new(),
            tool_repairs: Vec::new(),
        }
    }

    fn into_parts(self) -> (String, Vec<ModelRouteEvent>, Vec<ToolRepairEvent>) {
        (self.reasoning, self.model_routes, self.tool_repairs)
    }
}

impl StreamEvents for ReasoningCaptureEvents<'_> {
    fn on_reasoning_delta(&mut self, chunk: &str) {
        if !chunk.is_empty() {
            self.reasoning.push_str(chunk);
        }
        self.inner.on_reasoning_delta(chunk);
    }

    fn on_text_delta(&mut self, chunk: &str) {
        self.inner.on_text_delta(chunk);
    }

    fn on_assistant_done(&mut self, full_text: &str) {
        self.inner.on_assistant_done(full_text);
    }

    fn on_model_route(&mut self, preset: &str, model: &str, reason: &str, escalated: bool) {
        self.model_routes.push(ModelRouteEvent {
            preset: preset.to_string(),
            model: model.to_string(),
            reason: reason.to_string(),
            escalated,
        });
        self.inner.on_model_route(preset, model, reason, escalated);
    }

    fn on_model_budget_warning(&mut self, used_microusd: u64, budget_microusd: u64) {
        self.inner
            .on_model_budget_warning(used_microusd, budget_microusd);
    }

    fn on_tool_repair(&mut self, kind: &str, detail: &str) {
        self.tool_repairs.push(ToolRepairEvent {
            kind: kind.to_string(),
            detail: detail.to_string(),
        });
        self.inner.on_tool_repair(kind, detail);
    }

    fn on_tool_call(&mut self, name: &str, input: &BTreeMap<String, String>) {
        self.inner.on_tool_call(name, input);
    }
}

fn recent_step_replay_entry(message: &str, reasoning: &str) -> Option<String> {
    let message = compact_replay_text(message, 120);
    let reasoning = compact_replay_text(reasoning, 160);
    match (message.is_empty(), reasoning.is_empty()) {
        (true, true) => None,
        (false, true) => Some(message),
        (true, false) => Some(format!("reasoning: {reasoning}")),
        (false, false) => Some(format!("reasoning: {reasoning} | assistant: {message}")),
    }
}

fn compact_replay_text(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    let head = normalized.chars().take(max_chars).collect::<String>();
    format!("{head}...")
}

fn emit_tool_call(
    run_events: Option<&SharedAgentRunEvents>,
    tool_name: &str,
    input: &BTreeMap<String, String>,
) {
    if let Some(events) = run_events {
        events.borrow_mut().on_tool_call(tool_name, input);
    }
}

fn emit_permission_request(
    run_events: Option<&SharedAgentRunEvents>,
    tool_name: &str,
    input: &BTreeMap<String, String>,
    kind: &str,
    target: &str,
) {
    if let Some(events) = run_events {
        events
            .borrow_mut()
            .on_permission_request(tool_name, input, kind, target);
    }
}

fn push_tool_event(
    tool_events: &mut Vec<ToolEvent>,
    run_events: Option<&SharedAgentRunEvents>,
    event: ToolEvent,
) {
    if let Some(events) = run_events {
        events.borrow_mut().on_tool_result(&event);
    }
    tool_events.push(event);
}

fn tool_call_fingerprint(tool_name: &str, event_input: &BTreeMap<String, String>) -> String {
    let mut fingerprint = String::new();
    push_fingerprint_field(&mut fingerprint, tool_name);
    for (key, value) in event_input {
        push_fingerprint_field(&mut fingerprint, key);
        push_fingerprint_field(&mut fingerprint, value);
    }
    fingerprint
}

fn push_fingerprint_field(out: &mut String, value: &str) {
    out.push_str(&value.len().to_string());
    out.push(':');
    out.push_str(value);
    out.push(';');
}

/// Identity-only fingerprint for read-only inspection tools.
///
/// read_file/search_text/list_files have no advancing cursor — they only vary
/// by size caps (max_lines, limit, max_results), so re-issuing the same target
/// returns a prefix of identical content. The exact-args fingerprint misses
/// this (a different max_lines looks "new"), so repeat detection also keys on
/// the inspection target returned here. Returns None for tools without a
/// redundant-by-target identity; those rely on the exact fingerprint instead.
fn read_inspection_target(
    tool_name: &str,
    event_input: &BTreeMap<String, String>,
) -> Option<String> {
    match tool_name {
        "read_file" => event_input
            .get("path")
            .map(|path| format!("read_file:{path}")),
        "search_text" | "grep_files" => event_input
            .get("query")
            .or_else(|| event_input.get("pattern"))
            .or_else(|| event_input.get("q"))
            .map(|query| format!("search_text:{query}")),
        "list_files" | "list_dir" => {
            let root = event_input
                .get("root")
                .or_else(|| event_input.get("path"))
                .map(String::as_str)
                .unwrap_or(".");
            Some(format!("list_files:{root}"))
        }
        _ => None,
    }
}

/// Tools that mutate workspace files. A step that runs one of these resets the
/// "inspection without edit" counter that drives the stuck-directive.
fn is_edit_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "apply_patch" | "write_file" | "edit_file" | "fim_edit"
    )
}

fn trim_recent_call_fingerprints(fingerprints: &mut Vec<String>, keep: usize) {
    if fingerprints.len() > keep {
        let drop_n = fingerprints.len() - keep;
        fingerprints.drain(0..drop_n);
    }
}

fn maybe_rewrite_repeated_mcp_resource_list_call(
    tool_name: &str,
    event_input: &BTreeMap<String, String>,
    same_count_in_window: usize,
    observations: &[Observation],
    available_tools: &[String],
) -> Option<ToolCallRequest> {
    if tool_name != "mcp_list_resources"
        || same_count_in_window == 0
        || !available_tools
            .iter()
            .any(|tool| tool == "mcp_read_resource")
    {
        return None;
    }
    let uri = latest_mcp_resource_uri_from_observations(observations)?;
    let server = event_input
        .get("server")
        .cloned()
        .or_else(|| latest_mcp_resource_server_from_observations(observations))?;
    Some(ToolCallRequest {
        tool_name: "mcp_read_resource".to_string(),
        input: crate::tools::types::ToolInput::new()
            .with_arg("server", server)
            .with_arg("uri", uri),
    })
}

fn latest_mcp_resource_uri_from_observations(observations: &[Observation]) -> Option<String> {
    observations
        .iter()
        .rev()
        .filter(|observation| {
            observation.tool_name == "mcp_list_resources" && !observation.is_failure()
        })
        .flat_map(|observation| observation.summary.lines())
        .find_map(|line| {
            let uri = line.trim().strip_prefix("uri:")?.trim();
            if uri.is_empty() {
                None
            } else {
                Some(uri.to_string())
            }
        })
}

fn latest_mcp_resource_server_from_observations(observations: &[Observation]) -> Option<String> {
    observations
        .iter()
        .rev()
        .filter(|observation| {
            observation.tool_name == "mcp_list_resources" && !observation.is_failure()
        })
        .flat_map(|observation| observation.summary.lines())
        .find_map(|line| {
            let trimmed = line.trim();
            let rest = trimmed.strip_prefix("- ")?;
            let (server, _) = rest.split_once(" [")?;
            let server = server.trim();
            if server.is_empty() {
                None
            } else {
                Some(server.to_string())
            }
        })
}

fn repeat_short_circuit_threshold(
    tool_name: &str,
    event_input: &BTreeMap<String, String>,
) -> usize {
    if is_read_only_repeat_tool(tool_name, event_input) {
        2
    } else {
        1
    }
}

fn repeat_short_circuit_message(
    tool_name: &str,
    event_input: &BTreeMap<String, String>,
    count: usize,
    window: usize,
) -> String {
    if is_read_only_repeat_tool(tool_name, event_input) {
        format!(
            "repeated identical tool call detected: '{tool_name}' invoked {count} times in last {window} steps with same args. Break out of stuck loop and try a different path, arguments, or strategy."
        )
    } else {
        format!(
            "repeated identical mutating or side-effecting tool call suppressed before execution: '{tool_name}' was requested {count} times in last {window} steps with same args. Change strategy before retrying to avoid duplicate writes or side effects."
        )
    }
}

fn is_read_only_repeat_tool(tool_name: &str, event_input: &BTreeMap<String, String>) -> bool {
    if tool_name == "mcp_call" {
        return event_input
            .get("tool")
            .is_some_and(|remote_tool| mcp_remote_tool_is_read_only(remote_tool));
    }
    tool_metadata_for_name(tool_name).read_only
}

fn render_user_input_answers(answers: &BTreeMap<String, String>) -> String {
    let answers_json = JsonValue::Object(
        answers
            .iter()
            .map(|(key, value)| (key.clone(), JsonValue::String(value.clone())))
            .collect(),
    );
    let mut summary = String::new();
    summary.push_str("meta.user_input_required=false\n");
    summary.push_str(&format!("meta.answers={}\n", answers.len()));
    summary.push_str("answers_json=");
    summary.push_str(&json_value_to_string(&answers_json));
    summary.push('\n');
    summary.push_str("answers:\n");
    for (key, value) in answers {
        summary.push_str(&format!("- {key}: {value}\n"));
    }
    summary
}

fn model_respond_with_cancel<C: ModelClient>(
    client: &C,
    request: ModelRequest,
    events: &mut dyn StreamEvents,
    cancel_check: Option<&SharedAgentCancelCheck>,
) -> AppResult<(ModelResponse, Option<TokenUsage>)> {
    if let Some(check) = cancel_check {
        let mut guard = check.borrow_mut();
        let mut adapter = AgentCancelAdapter { inner: &mut *guard };
        client.respond_with_cancel(request, events, Some(&mut adapter))
    } else {
        client.respond_with_cancel(request, events, None)
    }
}

fn is_recoverable_model_tool_call_parse_error(error: &(dyn std::error::Error + 'static)) -> bool {
    error
        .to_string()
        .to_ascii_lowercase()
        .contains("tool_call_parse_failed")
}

fn model_tool_call_parse_failure_observation(error: &(dyn std::error::Error + 'static)) -> String {
    format!(
        "{}; previous model response contained malformed tool arguments that could not be repaired. Retry with valid tool JSON or choose a different strategy.",
        error
    )
}

fn execute_tool_with_cancel(
    registry: &crate::tools::registry::ToolRegistry,
    tool_name: &str,
    input: crate::tools::types::ToolInput,
    policy: &ExecutionPolicy,
    cancel_check: Option<&SharedAgentCancelCheck>,
) -> AppResult<crate::tools::types::ToolOutput> {
    if let Some(check) = cancel_check {
        let mut guard = check.borrow_mut();
        let mut adapter = AgentCancelAdapter { inner: &mut *guard };
        registry.execute_with_policy_and_cancel(tool_name, input, policy, Some(&mut adapter))
    } else {
        registry.execute_with_policy_and_cancel(tool_name, input, policy, None)
    }
}

#[derive(Debug, Clone)]
struct PreparedParallelToolCall {
    tool_name: String,
    input: crate::tools::types::ToolInput,
    event_input: BTreeMap<String, String>,
}

fn maybe_execute_parallel_safe_chunk<W: std::io::Write>(
    calls: &[ToolCallRequest],
    registry: &crate::tools::registry::ToolRegistry,
    policy: &ExecutionPolicy,
    config: &crate::config::types::AppConfig,
    hooks_enabled: bool,
    available_tools: &[String],
    primary_file: Option<&str>,
    renderer: &mut Option<crate::ui::stream::TtyRenderer<W>>,
    run_events: Option<&SharedAgentRunEvents>,
    cancel_check: Option<&SharedAgentCancelCheck>,
    observations: &mut Vec<Observation>,
    tool_events: &mut Vec<ToolEvent>,
    recent_call_fingerprints: &mut Vec<String>,
    recent_inspection_fingerprints: &mut Vec<String>,
    repeat_window: usize,
) -> AppResult<Option<usize>> {
    if hooks_enabled || !parallel_dispatch_enabled() || calls.len() < 2 {
        return Ok(None);
    }
    let max_parallel = parallel_dispatch_max();
    if max_parallel < 2 {
        return Ok(None);
    }
    let chunk_len = calls
        .iter()
        .take(max_parallel)
        .take_while(|call| registry.metadata(&call.tool_name).parallel_safe)
        .count();
    if chunk_len < 2 {
        return Ok(None);
    }

    let mut simulated_fingerprints = recent_call_fingerprints.clone();
    let mut simulated_inspection_fingerprints = recent_inspection_fingerprints.clone();
    let mut prepared = Vec::with_capacity(chunk_len);
    for call in &calls[..chunk_len] {
        check_cancelled(cancel_check)?;
        if !policy.allows_tool(&call.tool_name) {
            return Ok(None);
        }
        if registry
            .permission_request_for(&call.tool_name, &call.input, policy)
            .is_some()
        {
            return Ok(None);
        }
        let event_input = call.input.args.clone();
        let fingerprint = tool_call_fingerprint(&call.tool_name, &event_input);
        let same_count_in_window = simulated_fingerprints
            .iter()
            .rev()
            .take(repeat_window)
            .filter(|fp| **fp == fingerprint)
            .count();
        if same_count_in_window > 0 {
            return Ok(None);
        }
        let inspection_target = read_inspection_target(&call.tool_name, &event_input);
        let inspection_count_in_window = inspection_target
            .as_ref()
            .map(|target| {
                simulated_inspection_fingerprints
                    .iter()
                    .rev()
                    .take(repeat_window)
                    .filter(|fp| *fp == target)
                    .count()
            })
            .unwrap_or(0);
        if inspection_count_in_window > 0 {
            return Ok(None);
        }
        simulated_fingerprints.push(fingerprint);
        trim_recent_call_fingerprints(&mut simulated_fingerprints, repeat_window);
        if let Some(target) = inspection_target {
            simulated_inspection_fingerprints.push(target);
            trim_recent_call_fingerprints(&mut simulated_inspection_fingerprints, repeat_window);
        }
        prepared.push(PreparedParallelToolCall {
            tool_name: call.tool_name.clone(),
            input: call.input.clone(),
            event_input,
        });
    }

    *recent_call_fingerprints = simulated_fingerprints;
    *recent_inspection_fingerprints = simulated_inspection_fingerprints;
    for call in &prepared {
        emit_tool_call(run_events, &call.tool_name, &call.event_input);
    }

    let chunk_started = Instant::now();
    let outcomes = execute_prepared_parallel_safe_calls(prepared, policy, config);
    let chunk_elapsed_ms = chunk_started.elapsed().as_millis();
    let chunk_size = outcomes.len();
    for (call, result) in outcomes {
        check_cancelled(cancel_check)?;
        match result {
            Ok(mut output) => {
                output.summary = crate::tools::tool_output::maybe_spill_successful_tool_output(
                    &call.tool_name,
                    &output.summary,
                );
                let kind = ObservationKind::from_tool_name(&call.tool_name);
                let observation_summary = summarize_for_kind(&output.summary, kind);
                if let Some(renderer) = renderer.as_mut() {
                    renderer.paint_tool_result(
                        crate::ui::stream::ToolResultKind::Ok,
                        &call.tool_name,
                        kind.label(),
                        &output.summary,
                    );
                }
                observations.push(Observation::ok(
                    call.tool_name.clone(),
                    observation_summary.clone(),
                ));
                if let Some(recovery_hint) = derive_recovery_hint_after_success(
                    &call.tool_name,
                    &output.summary,
                    available_tools,
                    primary_file,
                    observations,
                ) {
                    observations.push(Observation::ok("recovery_hint", recovery_hint));
                }
                if let Some(replan_hint) =
                    derive_replan_hint(&call.tool_name, &output.summary, observations)
                {
                    observations.push(Observation::ok("replan_hint", replan_hint));
                }
                push_tool_event(
                    tool_events,
                    run_events,
                    ToolEvent {
                        tool_name: call.tool_name,
                        input: call.event_input,
                        output: append_parallel_dispatch_metadata(
                            output.summary,
                            chunk_size,
                            chunk_elapsed_ms,
                        ),
                        status: crate::model::protocol::ObservationStatus::Ok,
                    },
                );
            }
            Err(raw) => {
                let kind = ObservationKind::from_tool_name(&call.tool_name);
                let observation_summary = summarize_for_kind(&raw, kind);
                if let Some(renderer) = renderer.as_mut() {
                    renderer.paint_tool_result(
                        crate::ui::stream::ToolResultKind::Failed,
                        &call.tool_name,
                        kind.label(),
                        &raw,
                    );
                }
                observations.push(Observation::failed(
                    call.tool_name.clone(),
                    observation_summary.clone(),
                ));
                if let Some(recovery_hint) = derive_recovery_hint_after_failure(
                    &call.tool_name,
                    available_tools,
                    primary_file,
                    observations,
                ) {
                    observations.push(Observation::ok("recovery_hint", recovery_hint));
                }
                if let Some(replan_hint) =
                    derive_replan_hint(&call.tool_name, &observation_summary, observations)
                {
                    observations.push(Observation::ok("replan_hint", replan_hint));
                }
                push_tool_event(
                    tool_events,
                    run_events,
                    ToolEvent {
                        tool_name: call.tool_name,
                        input: call.event_input,
                        output: append_parallel_dispatch_metadata(
                            raw,
                            chunk_size,
                            chunk_elapsed_ms,
                        ),
                        status: crate::model::protocol::ObservationStatus::Failed,
                    },
                );
            }
        }
    }

    Ok(Some(chunk_len))
}

fn append_parallel_dispatch_metadata(
    mut output: String,
    chunk_size: usize,
    elapsed_ms: u128,
) -> String {
    output.push_str("\nmeta.parallel_dispatch=true");
    output.push_str(&format!("\nmeta.parallel_chunk_size={chunk_size}"));
    output.push_str(&format!("\nmeta.parallel_elapsed_ms={elapsed_ms}"));
    output
}

fn execute_prepared_parallel_safe_calls(
    calls: Vec<PreparedParallelToolCall>,
    policy: &ExecutionPolicy,
    config: &crate::config::types::AppConfig,
) -> Vec<(
    PreparedParallelToolCall,
    Result<crate::tools::types::ToolOutput, String>,
)> {
    thread::scope(|scope| {
        let mut handles = Vec::with_capacity(calls.len());
        for call in calls {
            let fallback = call.clone();
            let worker_policy = policy.clone();
            let worker_config = config.clone();
            let handle = scope.spawn(move || {
                parallel_test_probe(&call.input);
                let result = crate::tools::registry::execute_parallel_safe_tool(
                    &call.tool_name,
                    call.input.clone(),
                    &worker_policy,
                    &worker_config,
                )
                .map_err(|error| error.to_string());
                (call, result)
            });
            handles.push((fallback, handle));
        }
        handles
            .into_iter()
            .map(|(fallback, handle)| match handle.join() {
                Ok(result) => result,
                Err(_) => (fallback, Err("parallel tool worker panicked".to_string())),
            })
            .collect()
    })
}

fn parallel_dispatch_enabled() -> bool {
    !matches!(
        std::env::var("DSCODE_TOOL_DISPATCH")
            .ok()
            .as_deref()
            .map(str::trim),
        Some("serial") | Some("off") | Some("disabled")
    )
}

fn parallel_dispatch_max() -> usize {
    std::env::var("DSCODE_PARALLEL_MAX")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(4)
        .min(16)
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default)]
struct ParallelProbeStats {
    active: usize,
    max_active: usize,
}

#[cfg(test)]
static PARALLEL_TEST_PROBES: OnceLock<Mutex<BTreeMap<String, ParallelProbeStats>>> =
    OnceLock::new();

#[cfg(test)]
fn parallel_test_probes() -> &'static Mutex<BTreeMap<String, ParallelProbeStats>> {
    PARALLEL_TEST_PROBES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

#[cfg(test)]
fn parallel_test_probe(input: &crate::tools::types::ToolInput) {
    let Some(key) = input
        .get("_parallel_test_probe")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    let key = if key == "true" { "default" } else { key }.to_string();
    {
        let mut probes = parallel_test_probes().lock().unwrap();
        let stats = probes.entry(key.clone()).or_default();
        stats.active += 1;
        stats.max_active = stats.max_active.max(stats.active);
    }
    let delay_ms = input
        .get("_parallel_test_delay_ms")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(50);
    thread::sleep(Duration::from_millis(delay_ms));
    let mut probes = parallel_test_probes().lock().unwrap();
    let stats = probes.entry(key).or_default();
    stats.active = stats.active.saturating_sub(1);
}

#[cfg(not(test))]
fn parallel_test_probe(_input: &crate::tools::types::ToolInput) {}

#[cfg(test)]
fn reset_parallel_test_probe(key: &str) {
    parallel_test_probes()
        .lock()
        .unwrap()
        .insert(key.to_string(), ParallelProbeStats::default());
}

#[cfg(test)]
fn max_parallel_test_probe(key: &str) -> usize {
    parallel_test_probes()
        .lock()
        .unwrap()
        .get(key)
        .map(|stats| stats.max_active)
        .unwrap_or_default()
}

fn shell_env_hook_applies_to(tool_name: &str) -> bool {
    matches!(tool_name, "exec_shell" | "run_shell" | "task_shell_start")
}

fn check_cancelled(cancel_check: Option<&SharedAgentCancelCheck>) -> AppResult<()> {
    if let Some(check) = cancel_check {
        let mut guard = check.borrow_mut();
        if AgentCancelCheck::is_cancelled(&mut *guard)? {
            return Err(app_error("agent run cancelled"));
        }
    }
    Ok(())
}

fn push_post_tool_hook_observation(
    hooks: &crate::core::hooks::HookRunner,
    task: &str,
    tool_events: &[ToolEvent],
    observations: &mut Vec<Observation>,
) {
    let Some(event) = tool_events.last() else {
        return;
    };
    match hooks.post_tool_use(
        task,
        &event.tool_name,
        &event.input,
        event.status,
        &event.output,
    ) {
        Ok(Some(hook_context)) => {
            observations.push(Observation::ok(
                "hook",
                format!("post_tool_use: {hook_context}"),
            ));
        }
        Ok(None) => {}
        Err(error) => {
            observations.push(Observation::failed(
                "hook",
                format!("post_tool_use hook failed: {error}"),
            ));
        }
    }
}

fn derive_recovery_hint_after_success(
    tool_name: &str,
    output: &str,
    available_tools: &[String],
    primary_file: Option<&str>,
    observations: &[Observation],
) -> Option<String> {
    if tool_name == "search_text" && output.starts_with("No matches for `") {
        return format_recovery_hint(
            "search_text",
            preferred_listing_or_search_tool(available_tools)?,
            "search_text returned no matches, inspect the repository layout or broaden the lookup before retrying the query",
            None,
            None,
        );
    }

    if tool_name == "run_shell" && shell_exit_code(output).is_some_and(|code| code != 0) {
        if let Some(plan) =
            shell_recovery_directive(output, available_tools, primary_file, observations)
        {
            return format_recovery_hint(
                "run_shell",
                plan.next,
                &plan.reason,
                plan.query.as_deref(),
                plan.path.as_deref(),
            );
        }
    }

    None
}

fn derive_recovery_hint_after_failure(
    tool_name: &str,
    available_tools: &[String],
    primary_file: Option<&str>,
    observations: &[Observation],
) -> Option<String> {
    if latest_failure_is_unknown_tool(observations) {
        return format_recovery_hint(
            tool_name,
            preferred_tool_discovery_tool(available_tools)?,
            "model requested an unknown tool; search available tool definitions before retrying or choose one listed in Available tools",
            Some(tool_name),
            None,
        );
    }

    if is_mcp_tool_name(tool_name)
        && available_tools.iter().any(|tool| tool == "mcp_list_tools")
        && observations
            .last()
            .is_some_and(|observation| mcp_failure_is_policy_denial(&observation.summary))
    {
        return format_recovery_hint(
            tool_name,
            "mcp_list_tools",
            "MCP policy denied the remote tool call; list configured MCP tools before retrying or explain the policy blocker",
            None,
            None,
        );
    }

    match tool_name {
        "apply_patch" | "write_file" | "edit_file" | "fim_edit" => format_recovery_hint(
            tool_name,
            "read_file",
            "the edit did not apply: the find text must match the file byte-for-byte. Read the target once, then retry apply_patch with the SMALLEST unique anchor (1-3 lines copied exactly, no line-number prefixes) instead of reproducing a whole function or block",
            None,
            None,
        ),
        "read_file" => format_recovery_hint(
            "read_file",
            preferred_search_or_listing_tool(available_tools)?,
            "read_file failed, locate the correct file path before retrying the read",
            None,
            None,
        ),
        "dispatch_subagent" | "dispatch_subagents" => format_recovery_hint(
            "dispatch_subagent",
            preferred_search_or_listing_tool(available_tools)?,
            "subagent dispatch failed, continue locally with a direct inspection step",
            None,
            None,
        ),
        "run_shell" => format_recovery_hint(
            "run_shell",
            preferred_shell_recovery_tool(available_tools, primary_file, observations)?,
            "run_shell failed before completing, inspect the relevant code or diff before retrying the command",
            None,
            None,
        ),
        _ => None,
    }
}

fn latest_failure_is_unknown_tool(observations: &[Observation]) -> bool {
    observations.last().is_some_and(|observation| {
        observation.is_failure()
            && observation
                .summary
                .to_ascii_lowercase()
                .contains("unknown tool")
    })
}

fn is_mcp_tool_name(tool_name: &str) -> bool {
    tool_name == "mcp_call" || tool_name.starts_with(crate::tools::mcp::MCP_DYNAMIC_TOOL_PREFIX)
}

fn mcp_failure_is_policy_denial(summary: &str) -> bool {
    let lower = summary.to_ascii_lowercase();
    lower.contains("mcp tool call blocked by policy allowlist")
        || lower.contains("policy allowlist")
        || lower.contains("mcp tool call declined")
        || lower.contains("permission denied for mcp")
}

struct RecoveryDirective {
    next: &'static str,
    reason: String,
    query: Option<String>,
    path: Option<String>,
}

fn derive_replan_hint(
    tool_name: &str,
    output: &str,
    observations: &[Observation],
) -> Option<String> {
    if tool_name == "dispatch_subagent"
        && child_outcome(output).is_some_and(|outcome| outcome == "blocked")
    {
        return Some(
            "reason=subagent blocker; action=replan parent todo list around the blocker"
                .to_string(),
        );
    }

    if tool_name == "dispatch_subagents" && parallel_child_blocked(output) {
        return Some(
            "reason=subagent blocker; action=replan parent todo list around the blocker"
                .to_string(),
        );
    }

    if tool_name == "recovery_hint" {
        return None;
    }

    let recent_recovery_hints = observations
        .iter()
        .rev()
        .take(6)
        .filter(|observation| observation.tool_name == "recovery_hint")
        .count();
    if recent_recovery_hints >= 2 {
        return Some(
            "reason=multiple recovery hints in recent steps; action=replan the remaining todo list before continuing".to_string(),
        );
    }

    None
}

fn parallel_child_blocked(output: &str) -> bool {
    output
        .lines()
        .any(|line| line.starts_with("meta.parallel_child_") && line.contains("_outcome=blocked"))
}

fn child_outcome(summary: &str) -> Option<&str> {
    summary
        .lines()
        .find_map(|line| line.strip_prefix("meta.child_outcome="))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            summary
                .lines()
                .find_map(|line| line.strip_prefix("child outcome: "))
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
}

fn preferred_search_or_listing_tool(available_tools: &[String]) -> Option<&'static str> {
    if available_tools.iter().any(|tool| tool == "search_text") {
        Some("search_text")
    } else if available_tools.iter().any(|tool| tool == "list_files") {
        Some("list_files")
    } else {
        None
    }
}

fn preferred_listing_or_search_tool(available_tools: &[String]) -> Option<&'static str> {
    if available_tools.iter().any(|tool| tool == "list_files") {
        Some("list_files")
    } else if available_tools.iter().any(|tool| tool == "search_text") {
        Some("search_text")
    } else {
        None
    }
}

fn preferred_tool_discovery_tool(available_tools: &[String]) -> Option<&'static str> {
    if available_tools
        .iter()
        .any(|tool| tool == "tool_search_tool_bm25")
    {
        Some("tool_search_tool_bm25")
    } else if available_tools
        .iter()
        .any(|tool| tool == "tool_search_tool_regex")
    {
        Some("tool_search_tool_regex")
    } else {
        None
    }
}

fn preferred_shell_recovery_tool(
    available_tools: &[String],
    primary_file: Option<&str>,
    observations: &[Observation],
) -> Option<&'static str> {
    let has_apply_patch_success = observations
        .iter()
        .any(|observation| observation.tool_name == "apply_patch" && !observation.is_failure());
    if has_apply_patch_success && available_tools.iter().any(|tool| tool == "git_diff") {
        return Some("git_diff");
    }
    if primary_file.is_some() && available_tools.iter().any(|tool| tool == "read_file") {
        return Some("read_file");
    }
    preferred_search_or_listing_tool(available_tools)
}

fn format_recovery_hint(
    after: &str,
    next: &str,
    reason: &str,
    query: Option<&str>,
    path: Option<&str>,
) -> Option<String> {
    let mut parts = vec![format!("after={after}"), format!("next={next}")];
    if let Some(query) = query.filter(|value| !value.is_empty()) {
        parts.push(format!("query={query}"));
    }
    if let Some(path) = path.filter(|value| !value.is_empty()) {
        parts.push(format!("path={path}"));
    }
    parts.push(format!("reason={reason}"));
    Some(parts.join("; "))
}

fn shell_exit_code(output: &str) -> Option<i32> {
    output
        .lines()
        .find_map(|line| line.strip_prefix("meta.exit_code="))
        .or_else(|| {
            output
                .lines()
                .find_map(|line| line.strip_prefix("exit_code: "))
        })
        .and_then(|raw| raw.trim().parse::<i32>().ok())
}

fn shell_meta_value<'a>(output: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("meta.{key}=");
    output
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(str::trim)
}

fn shell_failure_reason(output: &str) -> String {
    let failure_kind = shell_meta_value(output, "failure_kind").unwrap_or("command_failure");
    let stderr_summary = shell_meta_value(output, "stderr_summary");
    let failed_tests = shell_meta_value(output, "failed_tests");

    match failure_kind {
        "test_failure" => {
            if let Some(failed_tests) = failed_tests.filter(|value| !value.is_empty()) {
                format!(
                    "run_shell reported failing tests ({failed_tests}), inspect the relevant code or diff before retrying the command"
                )
            } else if let Some(stderr_summary) = stderr_summary {
                format!(
                    "run_shell reported a test failure ({stderr_summary}), inspect the relevant code or diff before retrying the command"
                )
            } else {
                "run_shell reported a test failure, inspect the relevant code or diff before retrying the command"
                    .to_string()
            }
        }
        "lint_failure" => {
            if let Some(stderr_summary) = stderr_summary {
                format!(
                    "run_shell reported a lint failure ({stderr_summary}), inspect the relevant code or diff before retrying the command"
                )
            } else {
                "run_shell reported a lint failure, inspect the relevant code or diff before retrying the command"
                    .to_string()
            }
        }
        "build_failure" => {
            if let Some(stderr_summary) = stderr_summary {
                format!(
                    "run_shell reported a build failure ({stderr_summary}), inspect the relevant code or diff before retrying the command"
                )
            } else {
                "run_shell reported a build failure, inspect the relevant code or diff before retrying the command"
                    .to_string()
            }
        }
        _ => {
            if let Some(stderr_summary) = stderr_summary {
                format!(
                    "run_shell exited non-zero ({stderr_summary}), inspect the relevant code or diff before retrying the command"
                )
            } else {
                "run_shell exited non-zero, inspect the relevant code or diff before retrying the command"
                    .to_string()
            }
        }
    }
}

fn shell_recovery_directive(
    output: &str,
    available_tools: &[String],
    primary_file: Option<&str>,
    observations: &[Observation],
) -> Option<RecoveryDirective> {
    let failure_kind = shell_meta_value(output, "failure_kind").unwrap_or("command_failure");
    let failed_tests = shell_meta_value(output, "failed_tests");
    let stderr_summary = shell_meta_value(output, "stderr_summary");
    let reason = shell_failure_reason(output);
    let has_apply_patch_success = observations
        .iter()
        .any(|observation| observation.tool_name == "apply_patch" && !observation.is_failure());

    match failure_kind {
        "test_failure" => {
            if let Some(path) = failed_test_path(failed_tests)
                .filter(|path| is_javascript_test_path(path))
                .filter(|_| available_tools.iter().any(|tool| tool == "read_file"))
            {
                return Some(RecoveryDirective {
                    next: "read_file",
                    reason,
                    query: None,
                    path: Some(path),
                });
            }
            if has_apply_patch_success && available_tools.iter().any(|tool| tool == "git_diff") {
                return Some(RecoveryDirective {
                    next: "git_diff",
                    reason,
                    query: None,
                    path: None,
                });
            }
            if let Some(path) = failed_test_path(failed_tests)
                .filter(|_| available_tools.iter().any(|tool| tool == "read_file"))
            {
                return Some(RecoveryDirective {
                    next: "read_file",
                    reason,
                    query: None,
                    path: Some(path),
                });
            }
            if let Some(primary_file) =
                primary_file.filter(|_| available_tools.iter().any(|tool| tool == "read_file"))
            {
                return Some(RecoveryDirective {
                    next: "read_file",
                    reason,
                    query: None,
                    path: Some(primary_file.to_string()),
                });
            }
        }
        "lint_failure" | "build_failure" => {
            if let Some(query) = stderr_summary
                .and_then(derive_search_query_like)
                .filter(|_| available_tools.iter().any(|tool| tool == "search_text"))
            {
                return Some(RecoveryDirective {
                    next: "search_text",
                    reason,
                    query: Some(query),
                    path: None,
                });
            }
            if let Some(primary_file) =
                primary_file.filter(|_| available_tools.iter().any(|tool| tool == "read_file"))
            {
                return Some(RecoveryDirective {
                    next: "read_file",
                    reason,
                    query: None,
                    path: Some(primary_file.to_string()),
                });
            }
        }
        _ => {}
    }

    Some(RecoveryDirective {
        next: preferred_shell_recovery_tool(available_tools, primary_file, observations)?,
        reason,
        query: None,
        path: None,
    })
}

fn derive_search_query_like(text: &str) -> Option<String> {
    first_quoted_segment(text)
        .or_else(|| identifier_like_token_like(text))
        .or_else(|| {
            text.split_whitespace()
                .map(|word| {
                    word.trim_matches(|ch: char| {
                        !ch.is_ascii_alphanumeric() && ch != '_' && ch != ':' && ch != '-'
                    })
                })
                .find(|word| word.len() >= 3 && word.chars().any(|ch| ch.is_ascii_alphanumeric()))
                .map(str::to_string)
        })
}

fn first_quoted_segment(text: &str) -> Option<String> {
    for marker in ['`', '"', '\''] {
        let mut parts = text.split(marker);
        let _ = parts.next();
        if let Some(inner) = parts.next().map(str::trim).filter(|part| !part.is_empty()) {
            return Some(inner.to_string());
        }
    }
    None
}

fn identifier_like_token_like(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|word| {
            word.trim_matches(|ch: char| {
                !ch.is_ascii_alphanumeric() && ch != '_' && ch != ':' && ch != '-'
            })
        })
        .find(|word| {
            !word.is_empty()
                && (word.contains('_')
                    || word.contains("::")
                    || word.chars().any(|ch| ch.is_ascii_uppercase()))
        })
        .map(str::to_string)
}

fn failed_test_path(failed_tests: Option<&str>) -> Option<String> {
    let first = failed_tests?
        .split(',')
        .next()
        .map(str::trim)
        .filter(|part| !part.is_empty())?;
    let candidate = first.split("::").next().unwrap_or(first).trim();
    if candidate.contains('/') || candidate.ends_with(".py") || candidate.ends_with(".rs") {
        Some(candidate.to_string())
    } else {
        None
    }
}

fn is_javascript_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    (lower.ends_with(".js")
        || lower.ends_with(".jsx")
        || lower.ends_with(".ts")
        || lower.ends_with(".tsx"))
        && (lower.contains("/test") || lower.contains(".test.") || lower.contains(".spec."))
}

fn primary_file(profile: &crate::language::profile::LanguageProfile) -> Option<&str> {
    profile.file_priority.iter().find_map(|path| {
        let candidate = path.trim_end_matches('/');
        if std::path::Path::new(candidate).is_file() {
            Some(candidate)
        } else {
            None
        }
    })
}

const TODO_NUDGE: &str = "\n\nYou have access to a todo_write tool. Use it proactively when the request:\n- involves three or more distinct steps,\n- spans multiple files or non-trivial refactoring,\n- requires running tests or shell commands as part of completion.\n\nEach todo has fields: content (imperative, e.g. \"Run tests\"), activeForm (present continuous, e.g. \"Running tests\"), status (\"pending\" | \"in_progress\" | \"completed\").\n\nMark exactly one todo as in_progress at a time. Update the list (mark completed, add discovered tasks) before moving to the next step. Skip todo_write only for trivial single-step requests.";
const SUBAGENT_NUDGE: &str = "\n\n[sub-agent delegation]\nYou may call `dispatch_subagent` for one independent subtask, or `dispatch_subagents` when the user explicitly asks for parallel work or when multiple independent workstreams can run concurrently.\n- Only dispatch self-contained workstreams with concrete tasks and disjoint write scopes.\n- Prefer dispatch after a todo plan exists, or when the split is already obvious.\n- Nested dispatch is bounded; use it only when the child has its own clearly separable subtask.\n- Do NOT dispatch trivial reads, tiny edits, or work you can finish directly in one step.\n- Treat child results as summarized observations, read back child-edited files before relying on patches, then continue the parent plan.";
const EXPLICIT_PLANNING_BOOTSTRAP_NUDGE: &str = "\n\n[explicit-planning mode]\nThis task is large enough that you MUST create and follow a concrete plan.\n- If no todo plan exists yet, your NEXT turn MUST call todo_write with 3-7 concrete steps before repository inspection, edits, or test runs.\n- Keep exactly one todo in_progress at a time.\n- After a plan exists, execute the current in_progress step instead of starting over.\n- Do NOT rewrite the whole plan unless new evidence changes the approach.\n- Your assistant message should say which plan step you are executing now.";
const EXPLICIT_PLAN_EXECUTION_NUDGE: &str = "\n\n[plan execution]\nA todo plan already exists.\n- Continue from the current in_progress step.\n- Update todo_write only when a step changes status or new work is discovered.\n- Do NOT recreate the plan from scratch while execution is already in progress.";

/// Phase 10c-3: research-bootstrap nudge. Prepended to system prompt when the
/// workspace is empty AND the task text contains research keywords. Without
/// this, dogfood with v4-pro showed agents oscillating between mkdir +
/// todo_write for 30 steps without ever issuing a gh/curl call. Strong-style
/// directive that matches the empirically-observed failure mode.
const RESEARCH_BOOTSTRAP_NUDGE: &str = "\n\n[research-bootstrap mode]\nThe workspace is INTENTIONALLY EMPTY. You are doing research, not editing files.\n- Step 1 MUST be a REAL research call through `run_shell`, using `gh search ...` or `curl -sSL ...`.\n- DO NOT start with todo_write, mkdir, list_files, or any setup-only shell command.\n- DO NOT call mkdir, list_files, or run_shell with setup commands — the workspace is empty by design.\n- DO NOT repeat the same setup tool call. Each step should make NEW progress (a new gh query, a new curl URL, or a todo_write update after concrete research results exist).\n- After the first research result lands, use todo_write to track follow-up steps if the task is multi-step.\n- After research is complete, use apply_patch to write findings to a markdown file.";

fn should_apply_research_bootstrap(
    task: &str,
    workspace_root: &Path,
    available_tools: &[String],
) -> bool {
    if !task_looks_like_research(task) {
        return false;
    }
    if !workspace_is_bootstrap_empty(workspace_root) {
        return false;
    }
    available_tools.iter().any(|tool| tool == "run_shell")
}

fn task_looks_like_research(task: &str) -> bool {
    let lower = task.to_lowercase();
    let keywords = [
        "research",
        "investigate",
        "调研",
        "explore",
        "find on github",
        "gh search",
        "gh repo",
        "curl",
        "search github",
        "look up",
    ];
    keywords.iter().any(|kw| lower.contains(kw))
}

fn workspace_is_bootstrap_empty(workspace_root: &Path) -> bool {
    std::fs::read_dir(workspace_root)
        .map(|entries| {
            entries.filter_map(|e| e.ok()).all(|entry| {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                name_str.starts_with('.') || name_str == ".dscode"
            })
        })
        .unwrap_or(false)
}

#[cfg(test)]
fn build_system_prompt(skill_name: Option<&SkillSpec>) -> String {
    build_system_prompt_with_flags(skill_name, false, false, false, false)
}

/// DeepSeek may batch tool calls when a task mentions multiple subtopics. The
/// runtime can parallelize independent read-only batches, but side-effecting
/// work remains a serial barrier.
const TOOL_DISPATCH_NUDGE: &str = "\n\nYou may emit multiple tool calls in one turn only when every call is independent and read-only, such as list_files, read_file, search_text, git_status, or git_diff. Keep writes, shell commands, approvals, user input, side-effect MCP calls, and dependent calls serial: one tool call per turn.";

fn should_use_explicit_planning(
    task: &str,
    skill: Option<&SkillSpec>,
    available_tools: &[String],
) -> bool {
    if !available_tools.iter().any(|tool| tool == "todo_write") {
        return false;
    }

    let lower = task.to_lowercase();
    if crate::model::deepseek::task_has_direct_edit_request(task) {
        return false;
    }

    if skill.map(|s| s.suggested_steps.len() >= 3).unwrap_or(false) {
        return true;
    }

    let complexity_markers = [
        " and ",
        " then ",
        " across ",
        " multiple ",
        " end-to-end",
        " investigate",
        " research",
        "improve",
        "enhance",
        "stabilize",
        "hardening",
        "optimize",
        " better",
        " more like ",
        "close the gap",
        "gap closure",
        "production-ready",
        "production ready",
        "product-ready",
        "product ready",
        "productize",
        "productionize",
        "ship-ready",
        "ship ready",
        "daily coding",
        "daily use",
        " implement",
        " refactor",
        " debug",
        " review",
        " write ",
        " update ",
        " fix ",
        " verify ",
        " test ",
        " build ",
    ];

    complexity_markers
        .iter()
        .any(|marker| lower.contains(marker))
        || task.split_whitespace().count() >= 10
}

fn build_system_prompt_with_flags(
    skill_name: Option<&SkillSpec>,
    research_bootstrap: bool,
    planning_mode: bool,
    has_plan: bool,
    subagent_available: bool,
) -> String {
    let mut prompt = String::from(
        "You are the offline planning layer for DeepSeekCode. Prefer repository inspection before edits.",
    );
    prompt.push_str(TOOL_DISPATCH_NUDGE);
    // Note: TOOL_DISPATCH_NUDGE starts with explicit "\n\n" so order with
    // skill.system_append (added below) is well-defined regardless of trailing
    // punctuation in the base prompt.
    if let Some(skill) = skill_name {
        prompt.push_str(&format!(" Active skill: {}.", skill.name));
        if !skill.description.is_empty() {
            prompt.push_str(&format!(" Skill description: {}.", skill.description));
        }
        if !skill.references.is_empty() {
            prompt.push_str(" Skill references:");
            for reference in &skill.references {
                prompt.push_str(&format!(" [{reference}]"));
            }
            prompt.push('.');
        }
        if !skill.system_append.is_empty() {
            prompt.push(' ');
            prompt.push_str(skill.system_append.trim());
        }
    }
    if research_bootstrap {
        prompt.push_str(RESEARCH_BOOTSTRAP_NUDGE);
    }
    if planning_mode {
        if has_plan {
            prompt.push_str(EXPLICIT_PLAN_EXECUTION_NUDGE);
        } else {
            prompt.push_str(EXPLICIT_PLANNING_BOOTSTRAP_NUDGE);
        }
    }
    prompt.push_str(TODO_NUDGE);
    if subagent_available {
        prompt.push_str(SUBAGENT_NUDGE);
    }
    prompt
}

fn build_system_prompt_with_workspace_instructions(
    skill_name: Option<&SkillSpec>,
    research_bootstrap: bool,
    planning_mode: bool,
    has_plan: bool,
    subagent_available: bool,
    workspace_instructions: &[crate::core::instructions::InstructionFile],
    user_memory: Option<&crate::core::memory::PersistentMemory>,
) -> String {
    let mut prompt = build_system_prompt_with_flags(
        skill_name,
        research_bootstrap,
        planning_mode,
        has_plan,
        subagent_available,
    );
    if let Some(instructions) =
        crate::core::instructions::render_workspace_instructions(workspace_instructions)
    {
        prompt.push_str("\n\n");
        prompt.push_str(&instructions);
    }
    if let Some(memory) = user_memory {
        prompt.push_str("\n\n");
        prompt.push_str(&crate::core::memory::render_user_memory(memory));
    }
    prompt
}

fn append_translation_output_instruction(prompt: &mut String, target_language: &str) {
    let target_language = target_language.trim();
    if target_language.is_empty() {
        return;
    }
    prompt.push_str("\n\n");
    prompt.push_str(&translation_output_instruction(target_language));
}

fn translation_output_instruction(target_language: &str) -> String {
    format!(
        "## Language Output Requirement\n\
When responding to the user, write natural-language prose in {target_language}. \
Preserve code blocks, identifiers, command names, file paths, URLs, API names, \
function names, and user-requested English text as-is. Keep Markdown structure \
intact and do not mention this translation instruction unless the user asks."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_system_prompt_includes_todo_nudge() {
        let prompt = super::build_system_prompt(None);
        assert!(prompt.contains("todo_write"));
        assert!(prompt.contains("in_progress"));
        assert!(prompt.contains("Skip todo_write only for trivial"));
    }

    #[test]
    fn build_system_prompt_includes_workspace_instructions() {
        let instructions = [crate::core::instructions::InstructionFile {
            path: std::path::PathBuf::from("AGENTS.md"),
            content: "Run cargo test before committing.".to_string(),
            truncated: false,
        }];
        let prompt = super::build_system_prompt_with_workspace_instructions(
            None,
            false,
            false,
            false,
            false,
            &instructions,
            None,
        );

        assert!(prompt.contains("Workspace instructions"));
        assert!(prompt.contains("AGENTS.md"));
        assert!(prompt.contains("Run cargo test before committing."));
    }

    #[test]
    fn build_system_prompt_includes_user_memory() {
        let memory = crate::core::memory::PersistentMemory {
            path: std::path::PathBuf::from("memory.md"),
            content: "- prefer cargo test".to_string(),
            truncated: false,
        };
        let prompt = super::build_system_prompt_with_workspace_instructions(
            None,
            false,
            false,
            false,
            false,
            &[],
            Some(&memory),
        );

        assert!(prompt.contains("User memory"));
        assert!(prompt.contains("memory.md"));
        assert!(prompt.contains("prefer cargo test"));
    }

    #[test]
    fn preview_system_prompt_includes_workspace_context() {
        let dir = unique_tmp("system_prompt_preview");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "Run cargo test before committing.").unwrap();
        let memory_path = dir.join("memory.md");
        std::fs::write(&memory_path, "prefer narrow diffs").unwrap();
        let mut config = AppConfig::default();
        config.workspace.user_instructions_file =
            dir.join("missing-user-agents.md").display().to_string();
        config.workspace.user_skills_dir = dir.join("missing-skills").display().to_string();
        config.memory.enabled = true;
        config.memory.memory_path = memory_path.display().to_string();

        let preview = preview_system_prompt_for_workspace(
            &config,
            &dir,
            Some("inspect this project"),
            false,
            0,
        )
        .unwrap();

        assert_eq!(preview.workspace, dir.canonicalize().unwrap());
        assert_eq!(preview.profile_name, "generic");
        assert_eq!(preview.task.as_deref(), Some("inspect this project"));
        assert!(preview.prompt.contains("Workspace instructions"));
        assert!(preview.prompt.contains("Run cargo test before committing."));
        assert!(preview.prompt.contains("User memory"));
        assert!(preview.prompt.contains("prefer narrow diffs"));
        assert_eq!(preview.workspace_instruction_paths.len(), 1);
        assert_eq!(
            preview.user_memory_path.as_deref(),
            Some(memory_path.as_path())
        );
        assert!(!preview.user_memory_truncated);
        assert!(!preview.available_tools.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn build_system_prompt_places_nudge_after_skill_append() {
        use crate::skills::schema::{SkillPolicy, SkillSpec};
        // SkillPolicy has no Default impl in this codebase; construct explicitly.
        let skill = SkillSpec {
            name: "demo".to_string(),
            description: "demo skill".to_string(),
            allowed_tools: Vec::new(),
            system_append: "ZZZ_SKILL_HINT".to_string(),
            suggested_steps: Vec::new(),
            triggers: Vec::new(),
            initial_todos: Vec::new(),
            references: Vec::new(),
            policy: SkillPolicy {
                require_write_confirmation: false,
                require_shell_confirmation: false,
                shell_allowlist: Vec::new(),
            },
        };
        let prompt = super::build_system_prompt(Some(&skill));
        let skill_pos = prompt.find("ZZZ_SKILL_HINT").expect("skill hint present");
        let nudge_pos = prompt.find("todo_write").expect("nudge present");
        assert!(nudge_pos > skill_pos, "nudge must come after skill_append");
    }

    #[test]
    fn research_bootstrap_keyword_match_detects_research_in_task() {
        let prompt = super::build_system_prompt_with_flags(None, true, false, false, false);
        assert!(prompt.contains("research-bootstrap mode"));
        assert!(prompt.contains("INTENTIONALLY EMPTY"));
        assert!(prompt.contains("gh search"));
        assert!(prompt.contains("Step 1 MUST be a REAL research call"));
        assert!(prompt.contains("DO NOT start with todo_write"));
        assert!(prompt.contains("DO NOT call mkdir"));
    }

    #[test]
    fn research_bootstrap_disabled_omits_nudge() {
        let prompt = super::build_system_prompt_with_flags(None, false, false, false, false);
        assert!(!prompt.contains("research-bootstrap mode"));
        assert!(!prompt.contains("INTENTIONALLY EMPTY"));
        // TODO_NUDGE still applies (always on)
        assert!(prompt.contains("todo_write"));
    }

    #[test]
    fn explicit_planning_prompt_requires_todo_plan_before_execution() {
        let prompt = super::build_system_prompt_with_flags(None, false, true, false, false);
        assert!(prompt.contains("explicit-planning mode"));
        assert!(prompt.contains("NEXT turn MUST call todo_write"));
        assert!(prompt.contains("execute"));
    }

    #[test]
    fn explicit_planning_prompt_switches_to_execution_once_plan_exists() {
        let prompt = super::build_system_prompt_with_flags(None, false, true, true, false);
        assert!(prompt.contains("plan execution"));
        assert!(prompt.contains("Continue from the current in_progress step"));
        assert!(!prompt.contains("NEXT turn MUST call todo_write"));
    }

    #[test]
    fn subagent_prompt_nudge_only_appears_when_tool_available() {
        let prompt = super::build_system_prompt_with_flags(None, false, false, false, true);
        assert!(prompt.contains("sub-agent delegation"));
        assert!(prompt.contains("dispatch_subagent"));

        let without = super::build_system_prompt_with_flags(None, false, false, false, false);
        assert!(!without.contains("sub-agent delegation"));
    }

    #[test]
    fn workspace_is_bootstrap_empty_ignores_hidden_entries() {
        let dir = unique_tmp("bootstrap_hidden_only");
        std::fs::create_dir_all(dir.join(".dscode")).unwrap();
        std::fs::write(dir.join(".gitkeep"), "").unwrap();
        assert!(super::workspace_is_bootstrap_empty(&dir));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn workspace_is_bootstrap_empty_rejects_visible_entries() {
        let dir = unique_tmp("bootstrap_visible_file");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.md"), "hello").unwrap();
        assert!(!super::workspace_is_bootstrap_empty(&dir));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn research_bootstrap_requires_keyword_empty_workspace_and_run_shell() {
        let dir = unique_tmp("bootstrap_research");
        std::fs::create_dir_all(dir.join(".dscode")).unwrap();

        let tools = vec!["run_shell".to_string(), "todo_write".to_string()];
        assert!(super::should_apply_research_bootstrap(
            "research the ACP protocol on github",
            &dir,
            &tools,
        ));

        let no_shell = vec!["todo_write".to_string()];
        assert!(!super::should_apply_research_bootstrap(
            "research the ACP protocol on github",
            &dir,
            &no_shell,
        ));

        std::fs::write(dir.join("README.md"), "not empty").unwrap();
        assert!(!super::should_apply_research_bootstrap(
            "research the ACP protocol on github",
            &dir,
            &tools,
        ));

        let _ = std::fs::remove_dir_all(dir);
    }

    fn unique_tmp(label: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("dscode_loop_runtime_test_{label}_{nanos}"))
    }

    #[test]
    fn task_looks_like_research_matches_expected_keywords() {
        assert!(!super::task_looks_like_research(""));
        assert!(!super::task_looks_like_research("rename foo to bar"));
        assert!(super::task_looks_like_research(
            "research the ACP protocol on github"
        ));
        assert!(super::task_looks_like_research("帮我调研这个项目"));
    }

    #[test]
    fn shell_exit_code_reads_structured_metadata_first() {
        let output = "meta.command_kind=test\nmeta.exit_code=101\nmeta.result=failed\nexit_code: 0";
        assert_eq!(super::shell_exit_code(output), Some(101));
    }

    #[test]
    fn shell_failure_reason_mentions_failed_tests_when_present() {
        let output = "meta.command_kind=test\nmeta.exit_code=101\nmeta.result=failed\nmeta.failure_kind=test_failure\nmeta.failed_tests=parser::rejects_bad_input\nmeta.stderr_summary=test failed\nexit_code: 101";
        let reason = super::shell_failure_reason(output);
        assert!(reason.contains("parser::rejects_bad_input"));
        assert!(reason.contains("failing tests"));
    }

    #[test]
    fn derive_replan_hint_triggers_after_multiple_recovery_hints() {
        let observations = vec![
            Observation::ok("search_text", "No matches for `x`."),
            Observation::ok(
                "recovery_hint",
                "after=search_text; next=list_files; reason=first recovery",
            ),
            Observation::failed("read_file", "No such file"),
            Observation::ok(
                "recovery_hint",
                "after=read_file; next=search_text; reason=second recovery",
            ),
        ];
        let hint = super::derive_replan_hint("read_file", "No such file", &observations)
            .expect("expected replan hint");
        assert!(hint.contains("multiple recovery hints"));
    }

    #[test]
    fn derive_recovery_hint_for_unknown_tool_uses_tool_search() {
        let observations = vec![Observation::failed(
            "apply_file_patch",
            "unknown tool: apply_file_patch",
        )];
        let tools = vec![
            "read_file".to_string(),
            "tool_search_tool_regex".to_string(),
            "tool_search_tool_bm25".to_string(),
        ];
        let hint = super::derive_recovery_hint_after_failure(
            "apply_file_patch",
            &tools,
            None,
            &observations,
        )
        .expect("expected unknown-tool recovery hint");

        assert!(hint.contains("next=tool_search_tool_bm25"));
        assert!(hint.contains("query=apply_file_patch"));
        assert!(hint.contains("unknown tool"));
    }

    #[test]
    fn derive_recovery_hint_for_unknown_tool_requires_discovery_tool() {
        let observations = vec![Observation::failed(
            "apply_file_patch",
            "unknown tool: apply_file_patch",
        )];
        let tools = vec!["read_file".to_string(), "list_files".to_string()];

        assert!(super::derive_recovery_hint_after_failure(
            "apply_file_patch",
            &tools,
            None,
            &observations,
        )
        .is_none());
    }

    #[test]
    fn derive_replan_hint_triggers_for_blocked_subagent_summary() {
        let observations = vec![Observation::ok(
            "dispatch_subagent",
            "meta.child_outcome=blocked\nchild outcome: blocked",
        )];
        let hint = super::derive_replan_hint(
            "dispatch_subagent",
            "meta.child_outcome=blocked\nsubagent finished task `x`\nchild outcome: blocked",
            &observations,
        )
        .expect("expected subagent blocker replan hint");
        assert!(hint.contains("subagent blocker"));
    }

    #[test]
    fn derive_replan_hint_triggers_for_blocked_parallel_subagent_summary() {
        let hint = super::derive_replan_hint(
            "dispatch_subagents",
            "meta.parallel_child_1_outcome=ok\nmeta.parallel_child_2_outcome=blocked",
            &[],
        )
        .expect("expected parallel subagent blocker replan hint");
        assert!(hint.contains("subagent blocker"));
    }

    #[test]
    fn shell_recovery_directive_uses_read_file_for_failed_test_path() {
        let tools = vec!["read_file".to_string(), "search_text".to_string()];
        let output = "meta.command_kind=test\nmeta.exit_code=101\nmeta.result=failed\nmeta.failure_kind=test_failure\nmeta.failed_tests=src/cli/app.rs::cli_from_argv_routes_benchmark_subcommand\nmeta.stderr_summary=test failed\nexit_code: 101";
        let plan = super::shell_recovery_directive(output, &tools, None, &[])
            .expect("expected recovery directive");
        assert_eq!(plan.next, "read_file");
        assert_eq!(plan.path.as_deref(), Some("src/cli/app.rs"));
    }

    #[test]
    fn shell_recovery_directive_prefers_js_test_file_after_failed_validation() {
        let tools = vec!["read_file".to_string(), "git_diff".to_string()];
        let observations = vec![Observation::ok("apply_patch", "patched src/math.js")];
        let output = "meta.command_kind=test\nmeta.exit_code=1\nmeta.result=failed\nmeta.failure_kind=test_failure\nmeta.failed_tests=test/math.test.js\nmeta.stderr_summary=test failed\nexit_code: 1";
        let plan =
            super::shell_recovery_directive(output, &tools, Some("src/math.js"), &observations)
                .expect("expected recovery directive");
        assert_eq!(plan.next, "read_file");
        assert_eq!(plan.path.as_deref(), Some("test/math.test.js"));
    }

    #[test]
    fn shell_recovery_directive_uses_search_text_for_lint_failure_query() {
        let tools = vec!["search_text".to_string(), "read_file".to_string()];
        let output = "meta.command_kind=lint\nmeta.exit_code=1\nmeta.result=failed\nmeta.failure_kind=lint_failure\nmeta.stderr_summary=cannot find value `dispatch_subagent` in this scope\nexit_code: 1";
        let plan = super::shell_recovery_directive(output, &tools, None, &[])
            .expect("expected recovery directive");
        assert_eq!(plan.next, "search_text");
        assert_eq!(plan.query.as_deref(), Some("dispatch_subagent"));
    }

    #[test]
    fn explicit_planning_heuristic_skips_simple_replace_task() {
        let tools = vec!["todo_write".to_string()];
        assert!(!super::should_use_explicit_planning(
            "replace \"a\" with \"b\" in src/lib.rs",
            None,
            &tools,
        ));
    }

    #[test]
    fn explicit_planning_heuristic_skips_pr_replace_task_when_edit_request_is_clear() {
        let tools = vec!["todo_write".to_string()];
        assert!(!super::should_use_explicit_planning(
            "Address PR #44 review feedback: replace `a - b` with `a + b` in src/lib.rs and validate with cargo test.",
            None,
            &tools,
        ));
    }

    #[test]
    fn explicit_planning_heuristic_triggers_for_complex_task_or_skill_steps() {
        let tools = vec!["todo_write".to_string()];
        assert!(super::should_use_explicit_planning(
            "implement the new auth flow and verify the tests still pass",
            None,
            &tools,
        ));

        use crate::skills::schema::{SkillPolicy, SkillSpec};
        let skill = SkillSpec {
            name: "demo".to_string(),
            description: "demo skill".to_string(),
            allowed_tools: Vec::new(),
            system_append: String::new(),
            suggested_steps: vec!["one".to_string(), "two".to_string(), "three".to_string()],
            triggers: Vec::new(),
            initial_todos: Vec::new(),
            references: Vec::new(),
            policy: SkillPolicy {
                require_write_confirmation: false,
                require_shell_confirmation: false,
                shell_allowlist: Vec::new(),
            },
        };
        assert!(super::should_use_explicit_planning(
            "short task",
            Some(&skill),
            &tools
        ));
    }

    #[test]
    fn explicit_planning_heuristic_triggers_for_ambiguous_improvement_tasks() {
        let tools = vec!["todo_write".to_string()];
        assert!(super::should_use_explicit_planning(
            "improve benchmark reliability",
            None,
            &tools,
        ));
        assert!(super::should_use_explicit_planning(
            "make the CLI onboarding better",
            None,
            &tools,
        ));
        assert!(super::should_use_explicit_planning(
            "make DeepSeekCode more like Claude Code",
            None,
            &tools,
        ));
        assert!(super::should_use_explicit_planning(
            "close the product gap for PR review",
            None,
            &tools,
        ));
        assert!(super::should_use_explicit_planning(
            "make the CLI production-ready",
            None,
            &tools,
        ));
        assert!(super::should_use_explicit_planning(
            "productionize DeepSeekCode for daily coding work",
            None,
            &tools,
        ));
        assert!(super::should_use_explicit_planning(
            "make this ship-ready for daily use",
            None,
            &tools,
        ));
    }

    #[test]
    fn build_system_prompt_includes_skill_references_when_present() {
        use crate::skills::schema::{SkillPolicy, SkillSpec};
        let skill = SkillSpec {
            name: "demo".to_string(),
            description: "demo skill".to_string(),
            allowed_tools: Vec::new(),
            system_append: String::new(),
            suggested_steps: Vec::new(),
            triggers: Vec::new(),
            initial_todos: Vec::new(),
            references: vec!["docs/guide.md".to_string(), "README.md".to_string()],
            policy: SkillPolicy {
                require_write_confirmation: false,
                require_shell_confirmation: false,
                shell_allowlist: Vec::new(),
            },
        };
        let prompt = super::build_system_prompt(Some(&skill));
        assert!(prompt.contains("Skill references: [docs/guide.md] [README.md]."));
    }

    #[test]
    fn agent_loop_options_default_provides_empty_todo_list() {
        let opts = AgentLoopOptions::default();
        assert_eq!(opts.steps, 4);
        assert!(opts.todos.borrow().is_empty());
    }
}

#[cfg(test)]
mod cr1_regression_test {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use crate::core::context::TaskContext;
    use crate::core::todos::{TodoList, TodoStatus};
    use crate::model::client::ModelClient;
    use crate::model::protocol::{
        ModelAction, ModelRequest, ModelResponse, TokenUsage, ToolCallRequest,
    };
    use crate::tools::types::ToolInput;
    use crate::ui::stream::StreamEvents;

    struct ScriptedClient {
        calls: RefCell<u32>,
    }

    impl ModelClient for ScriptedClient {
        fn respond(
            &self,
            _input: ModelRequest,
            _events: &mut dyn StreamEvents,
        ) -> crate::error::AppResult<(ModelResponse, Option<TokenUsage>)> {
            let n = *self.calls.borrow();
            *self.calls.borrow_mut() = n + 1;
            let action = if n == 0 {
                let mut input = ToolInput::new();
                let items = r#"[{"content":"A","activeForm":"Aing","status":"pending"},{"content":"B","activeForm":"Bing","status":"in_progress"},{"content":"C","activeForm":"Cing","status":"completed"}]"#;
                input.args.insert("items".to_string(), items.to_string());
                ModelAction::CallTool {
                    tool_name: "todo_write".to_string(),
                    input,
                }
            } else {
                ModelAction::Finish
            };
            Ok((
                ModelResponse {
                    message: "scripted".to_string(),
                    action,
                },
                None,
            ))
        }
    }

    struct ScriptedReplyClient {
        replies: RefCell<Vec<String>>,
        captured_recent_steps: RefCell<Vec<Vec<String>>>,
    }

    impl ModelClient for ScriptedReplyClient {
        fn respond(
            &self,
            input: ModelRequest,
            _events: &mut dyn StreamEvents,
        ) -> crate::error::AppResult<(ModelResponse, Option<TokenUsage>)> {
            self.captured_recent_steps
                .borrow_mut()
                .push(input.recent_steps.clone());
            let n = self.captured_recent_steps.borrow().len() - 1;
            let action = if n < 2 {
                let mut tin = ToolInput::new();
                tin.args.insert("root".to_string(), ".".to_string());
                tin.args.insert("max_depth".to_string(), "1".to_string());
                tin.args.insert("limit".to_string(), "5".to_string());
                ModelAction::CallTool {
                    tool_name: "list_files".to_string(),
                    input: tin,
                }
            } else {
                ModelAction::Finish
            };
            let message = self
                .replies
                .borrow()
                .get(n)
                .cloned()
                .unwrap_or_else(|| "done".to_string());
            Ok((ModelResponse { message, action }, None))
        }
    }

    struct SystemPromptCapturingClient {
        captured_system_prompts: RefCell<Vec<String>>,
    }

    impl ModelClient for SystemPromptCapturingClient {
        fn respond(
            &self,
            input: ModelRequest,
            _events: &mut dyn StreamEvents,
        ) -> crate::error::AppResult<(ModelResponse, Option<TokenUsage>)> {
            self.captured_system_prompts
                .borrow_mut()
                .push(input.system_prompt);
            Ok((
                ModelResponse {
                    message: "done".to_string(),
                    action: ModelAction::Finish,
                },
                None,
            ))
        }
    }

    #[test]
    fn run_with_client_adds_translation_instruction_to_system_prompt() {
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("answer in my UI language".to_string(), None)
            .with_translation_target_language("Simplified Chinese");
        let client = SystemPromptCapturingClient {
            captured_system_prompts: RefCell::new(Vec::new()),
        };

        agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 1,
                    emit_progress: false,
                    persist_session: false,
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();

        let captured = client.captured_system_prompts.borrow();
        assert_eq!(captured.len(), 1);
        assert!(captured[0].contains("## Language Output Requirement"));
        assert!(captured[0].contains("Simplified Chinese"));
        assert!(captured[0].contains("Preserve code blocks"));
    }

    #[test]
    fn run_with_client_replays_recent_assistant_steps_into_each_request() {
        // Phase 10c-1 regression: dscode run multi-step loops without seeing prior
        // assistant messages, causing "I'll start by..." infinite loops. Verify the
        // ModelRequest.recent_steps field carries prior messages forward.
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("dummy".to_string(), None);
        let todos = Rc::new(RefCell::new(TodoList::default()));
        let client = ScriptedReplyClient {
            replies: RefCell::new(vec![
                "step ONE: looking at files".to_string(),
                "step TWO: read first one".to_string(),
                "step THREE: finishing".to_string(),
            ]),
            captured_recent_steps: RefCell::new(Vec::new()),
        };
        let _ = agent.run_with_client(
            context,
            AgentLoopOptions {
                steps: 3,
                initial_observations: Vec::new(),
                todos,
                ..AgentLoopOptions::default()
            },
            &client,
        );

        let captured = client.captured_recent_steps.borrow();
        assert_eq!(captured.len(), 3, "should have called respond 3 times");
        // First call: no prior steps yet.
        assert!(
            captured[0].is_empty(),
            "step 1 should see empty recent_steps"
        );
        // Second call: should see step 1's message.
        assert_eq!(captured[1].len(), 1);
        assert!(captured[1][0].contains("step ONE"));
        // Third call: should see steps 1 + 2.
        assert_eq!(captured[2].len(), 2);
        assert!(captured[2][0].contains("step ONE"));
        assert!(captured[2][1].contains("step TWO"));
    }

    #[test]
    fn run_with_client_replays_initial_recent_steps_on_first_request() {
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("dummy".to_string(), None);
        let client = ScriptedReplyClient {
            replies: RefCell::new(vec!["done".to_string()]),
            captured_recent_steps: RefCell::new(Vec::new()),
        };
        let _ = agent.run_with_client(
            context,
            AgentLoopOptions {
                steps: 1,
                emit_progress: false,
                initial_recent_steps: vec![
                    "older persisted reasoning".to_string(),
                    "latest persisted reasoning".to_string(),
                ],
                ..AgentLoopOptions::default()
            },
            &client,
        );

        let captured = client.captured_recent_steps.borrow();
        assert_eq!(captured.len(), 1);
        assert_eq!(
            captured[0],
            vec![
                "older persisted reasoning".to_string(),
                "latest persisted reasoning".to_string()
            ]
        );
    }

    struct ScriptedReasoningClient {
        captured_recent_steps: RefCell<Vec<Vec<String>>>,
    }

    impl ModelClient for ScriptedReasoningClient {
        fn respond(
            &self,
            input: ModelRequest,
            events: &mut dyn StreamEvents,
        ) -> crate::error::AppResult<(ModelResponse, Option<TokenUsage>)> {
            self.captured_recent_steps
                .borrow_mut()
                .push(input.recent_steps.clone());
            let n = self.captured_recent_steps.borrow().len() - 1;
            events.on_reasoning_delta(&format!("thinking through step {n}"));
            let action = if n == 0 {
                let mut tin = ToolInput::new();
                tin.args.insert("root".to_string(), ".".to_string());
                tin.args.insert("max_depth".to_string(), "1".to_string());
                tin.args.insert("limit".to_string(), "5".to_string());
                ModelAction::CallTool {
                    tool_name: "list_files".to_string(),
                    input: tin,
                }
            } else {
                ModelAction::Finish
            };
            Ok((
                ModelResponse {
                    message: format!("assistant message {n}"),
                    action,
                },
                None,
            ))
        }
    }

    struct BudgetUsageClient {
        calls: RefCell<u32>,
    }

    impl ModelClient for BudgetUsageClient {
        fn respond(
            &self,
            _input: ModelRequest,
            _events: &mut dyn StreamEvents,
        ) -> crate::error::AppResult<(ModelResponse, Option<TokenUsage>)> {
            let n = *self.calls.borrow();
            *self.calls.borrow_mut() = n + 1;
            let action = if n == 0 {
                ModelAction::CallTool {
                    tool_name: "list_files".to_string(),
                    input: ToolInput::new()
                        .with_arg("root", ".")
                        .with_arg("max_depth", "1")
                        .with_arg("limit", "1"),
                }
            } else {
                ModelAction::Finish
            };
            let mut usage = TokenUsage::with_prompt_cache(1000, 1000, 0, 1000);
            usage.model = Some("deepseek-v4-flash".to_string());
            Ok((
                ModelResponse {
                    message: "budget step".to_string(),
                    action,
                },
                Some(usage),
            ))
        }
    }

    #[derive(Default)]
    struct CapturingStreamEvents {
        routes: Vec<ModelRouteEvent>,
        repairs: Vec<ToolRepairEvent>,
        budget_warnings: Vec<(u64, u64)>,
    }

    impl StreamEvents for CapturingStreamEvents {
        fn on_text_delta(&mut self, _chunk: &str) {}

        fn on_assistant_done(&mut self, _full_text: &str) {}

        fn on_model_route(&mut self, preset: &str, model: &str, reason: &str, escalated: bool) {
            self.routes.push(ModelRouteEvent {
                preset: preset.to_string(),
                model: model.to_string(),
                reason: reason.to_string(),
                escalated,
            });
        }

        fn on_model_budget_warning(&mut self, used_microusd: u64, budget_microusd: u64) {
            self.budget_warnings.push((used_microusd, budget_microusd));
        }

        fn on_tool_repair(&mut self, kind: &str, detail: &str) {
            self.repairs.push(ToolRepairEvent {
                kind: kind.to_string(),
                detail: detail.to_string(),
            });
        }

        fn on_tool_call(&mut self, _name: &str, _input: &BTreeMap<String, String>) {}
    }

    #[test]
    fn reasoning_capture_forwards_model_policy_events_and_captures_repairs() {
        let mut sink = CapturingStreamEvents::default();
        let mut capture = ReasoningCaptureEvents::new(&mut sink);

        capture.on_model_route("auto", "deepseek-v4-pro", "repeated repair signals", true);
        capture.on_model_budget_warning(800, 1000);
        capture.on_tool_repair("truncated-json", "repaired truncated tool arguments JSON");
        let (_reasoning, routes, repairs) = capture.into_parts();

        assert_eq!(
            routes,
            vec![ModelRouteEvent {
                preset: "auto".to_string(),
                model: "deepseek-v4-pro".to_string(),
                reason: "repeated repair signals".to_string(),
                escalated: true,
            }]
        );
        assert_eq!(sink.routes, routes);
        assert_eq!(
            repairs,
            vec![ToolRepairEvent {
                kind: "truncated-json".to_string(),
                detail: "repaired truncated tool arguments JSON".to_string(),
            }]
        );
        assert_eq!(sink.repairs, repairs);
        assert_eq!(sink.budget_warnings, vec![(800, 1000)]);
    }

    #[test]
    fn run_with_client_refuses_next_turn_after_session_budget_is_exhausted() {
        let mut cfg = crate::config::types::AppConfig::default();
        cfg.model.session_budget_microusd = 1;
        let agent = AgentLoop::new(cfg);
        let client = BudgetUsageClient {
            calls: RefCell::new(0),
        };

        let error = agent
            .run_with_client(
                TaskContext::new("list one file then continue".to_string(), None),
                AgentLoopOptions {
                    steps: 2,
                    emit_progress: false,
                    persist_session: false,
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap_err();

        assert!(error.to_string().contains("session budget exhausted"));
        assert_eq!(*client.calls.borrow(), 1);
    }

    #[test]
    fn run_with_client_refuses_first_turn_when_persisted_budget_is_exhausted() {
        let mut cfg = crate::config::types::AppConfig::default();
        cfg.model.session_budget_microusd = 0;
        let agent = AgentLoop::new(cfg);
        let client = BudgetUsageClient {
            calls: RefCell::new(0),
        };

        let error = agent
            .run_with_client(
                TaskContext::new("do not call the model".to_string(), None),
                AgentLoopOptions {
                    steps: 1,
                    emit_progress: false,
                    persist_session: false,
                    session_budget: Some(AgentSessionBudget {
                        budget_microusd: 10,
                        used_microusd: 10,
                    }),
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap_err();

        assert!(error.to_string().contains("session budget exhausted"));
        assert_eq!(*client.calls.borrow(), 0);
    }

    #[test]
    fn run_with_client_replays_recent_reasoning_into_next_request() {
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("dummy".to_string(), None);
        let client = ScriptedReasoningClient {
            captured_recent_steps: RefCell::new(Vec::new()),
        };

        let _ = agent.run_with_client(
            context,
            AgentLoopOptions {
                steps: 2,
                emit_progress: false,
                ..AgentLoopOptions::default()
            },
            &client,
        );

        let captured = client.captured_recent_steps.borrow();
        assert_eq!(captured.len(), 2);
        assert!(captured[0].is_empty());
        assert_eq!(captured[1].len(), 1);
        assert!(captured[1][0].contains("reasoning: thinking through step 0"));
        assert!(captured[1][0].contains("assistant: assistant message 0"));
    }

    struct CapturingRunEvents {
        entries: Rc<RefCell<Vec<String>>>,
    }

    impl AgentRunEvents for CapturingRunEvents {
        fn on_tool_call(&mut self, tool_name: &str, _input: &BTreeMap<String, String>) {
            self.entries.borrow_mut().push(format!("call:{tool_name}"));
        }

        fn on_permission_request(
            &mut self,
            tool_name: &str,
            _input: &BTreeMap<String, String>,
            kind: &str,
            target: &str,
        ) {
            self.entries
                .borrow_mut()
                .push(format!("permission:{tool_name}:{kind}:{target}"));
        }

        fn on_tool_result(&mut self, event: &ToolEvent) {
            self.entries.borrow_mut().push(format!(
                "result:{}:{}",
                event.tool_name,
                match event.status {
                    crate::model::protocol::ObservationStatus::Ok => "ok",
                    crate::model::protocol::ObservationStatus::Failed => "failed",
                }
            ));
        }
    }

    struct CountingCancelCheck {
        calls: usize,
        cancel_after: usize,
    }

    impl AgentCancelCheck for CountingCancelCheck {
        fn is_cancelled(&mut self) -> AppResult<bool> {
            self.calls += 1;
            Ok(self.calls >= self.cancel_after)
        }
    }

    #[test]
    fn run_with_client_stops_when_cancel_check_trips() {
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("dummy".to_string(), None);
        let client = ScriptedReplyClient {
            replies: RefCell::new(vec!["step one".to_string(), "step two".to_string()]),
            captured_recent_steps: RefCell::new(Vec::new()),
        };
        let cancel_check: SharedAgentCancelCheck = Rc::new(RefCell::new(CountingCancelCheck {
            calls: 0,
            cancel_after: 2,
        }));

        let error = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 3,
                    emit_progress: false,
                    cancel_check: Some(cancel_check),
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap_err();

        assert!(error.to_string().contains("agent run cancelled"));
        assert_eq!(client.captured_recent_steps.borrow().len(), 1);
    }

    #[test]
    fn run_with_client_emits_live_tool_call_and_result_events() {
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("dummy".to_string(), None);
        let todos = Rc::new(RefCell::new(TodoList::default()));
        let entries = Rc::new(RefCell::new(Vec::new()));
        let sink: SharedAgentRunEvents = Rc::new(RefCell::new(CapturingRunEvents {
            entries: entries.clone(),
        }));
        let client = ScriptedReplyClient {
            replies: RefCell::new(vec!["step ONE: looking at files".to_string()]),
            captured_recent_steps: RefCell::new(Vec::new()),
        };

        agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 1,
                    initial_observations: Vec::new(),
                    todos,
                    emit_progress: false,
                    run_events: Some(sink),
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();

        assert_eq!(
            entries.borrow().as_slice(),
            ["call:list_files", "result:list_files:ok"]
        );
    }

    struct FixedApprovalResolver {
        decision: AgentApprovalDecision,
        seen: Rc<RefCell<Vec<AgentApprovalRequest>>>,
    }

    impl AgentApprovalResolver for FixedApprovalResolver {
        fn resolve(
            &mut self,
            request: &AgentApprovalRequest,
        ) -> crate::error::AppResult<AgentApprovalDecision> {
            self.seen.borrow_mut().push(request.clone());
            Ok(self.decision)
        }
    }

    struct FixedUserInputResolver {
        answers: BTreeMap<String, String>,
        seen: Rc<RefCell<Vec<AgentUserInputRequest>>>,
    }

    impl AgentUserInputResolver for FixedUserInputResolver {
        fn resolve(
            &mut self,
            request: &AgentUserInputRequest,
        ) -> crate::error::AppResult<AgentUserInputResponse> {
            self.seen.borrow_mut().push(request.clone());
            Ok(AgentUserInputResponse {
                answers: self.answers.clone(),
            })
        }
    }

    #[test]
    fn run_with_client_uses_user_input_resolver_for_request_user_input() {
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("dummy".to_string(), None);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let answers = BTreeMap::from([("mode".to_string(), "Plan".to_string())]);
        let resolver: SharedAgentUserInputResolver =
            Rc::new(RefCell::new(FixedUserInputResolver {
                answers,
                seen: seen.clone(),
            }));
        let questions = r#"[{"header":"Mode","id":"mode","question":"Which mode?","options":[{"label":"Plan","description":"Plan first."},{"label":"Apply","description":"Implement directly."}]}]"#;
        let client = ScriptedActionsClient {
            captured_observations: RefCell::new(Vec::new()),
            actions: vec![
                ModelAction::CallTool {
                    tool_name: "request_user_input".to_string(),
                    input: ToolInput::new().with_arg("questions", questions),
                },
                ModelAction::Finish,
            ],
            calls: RefCell::new(0),
        };

        let result = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 2,
                    initial_observations: Vec::new(),
                    todos: Rc::new(RefCell::new(TodoList::default())),
                    emit_progress: false,
                    user_input_resolver: Some(resolver),
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();

        assert_eq!(seen.borrow().len(), 1);
        assert!(seen.borrow()[0].input.contains_key("questions"));
        assert_eq!(result.tool_events.len(), 1);
        assert_eq!(
            result.tool_events[0].status,
            crate::model::protocol::ObservationStatus::Ok
        );
        assert!(result.tool_events[0]
            .output
            .contains("meta.user_input_required=false"));
        assert!(result.tool_events[0]
            .output
            .contains(r#"answers_json={"mode":"Plan"}"#));
    }

    #[test]
    fn run_with_client_uses_approval_resolver_for_permissioned_tools() {
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("dummy".to_string(), None);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let resolver: SharedAgentApprovalResolver = Rc::new(RefCell::new(FixedApprovalResolver {
            decision: AgentApprovalDecision::Approved,
            seen: seen.clone(),
        }));
        let client = ScriptedActionsClient {
            captured_observations: RefCell::new(Vec::new()),
            actions: vec![
                ModelAction::CallTool {
                    tool_name: "run_shell".to_string(),
                    input: ToolInput::new()
                        .with_arg("command", "pwd")
                        .with_arg("cwd", "."),
                },
                ModelAction::Finish,
            ],
            calls: RefCell::new(0),
        };

        let result = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 2,
                    initial_observations: Vec::new(),
                    todos: Rc::new(RefCell::new(TodoList::default())),
                    emit_progress: false,
                    approval_resolver: Some(resolver),
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();

        assert_eq!(seen.borrow().len(), 1);
        assert_eq!(seen.borrow()[0].kind, "shell");
        assert_eq!(seen.borrow()[0].target, "pwd");
        assert_eq!(result.tool_events.len(), 1);
        assert!(result.tool_events[0].output.contains("exit_code: 0"));
        assert_eq!(
            result.tool_events[0].status,
            crate::model::protocol::ObservationStatus::Ok
        );
    }

    #[test]
    fn run_with_client_stops_permissioned_tool_when_resolver_denies() {
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("dummy".to_string(), None);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let resolver: SharedAgentApprovalResolver = Rc::new(RefCell::new(FixedApprovalResolver {
            decision: AgentApprovalDecision::Denied,
            seen: seen.clone(),
        }));
        let client = ScriptedActionsClient {
            captured_observations: RefCell::new(Vec::new()),
            actions: vec![
                ModelAction::CallTool {
                    tool_name: "run_shell".to_string(),
                    input: ToolInput::new()
                        .with_arg("command", "pwd")
                        .with_arg("cwd", "."),
                },
                ModelAction::Finish,
            ],
            calls: RefCell::new(0),
        };

        let result = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 2,
                    initial_observations: Vec::new(),
                    todos: Rc::new(RefCell::new(TodoList::default())),
                    emit_progress: false,
                    approval_resolver: Some(resolver),
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();

        assert_eq!(seen.borrow().len(), 1);
        assert_eq!(result.tool_events.len(), 1);
        assert_eq!(
            result.tool_events[0].status,
            crate::model::protocol::ObservationStatus::Failed
        );
        assert!(result.tool_events[0]
            .output
            .contains("permission denied for shell: pwd"));
        assert!(!result.tool_events[0].output.contains("exit_code: 0"));
    }

    /// Phase 10c-2: scripted client emits N identical list_files calls in a row.
    /// Used to verify repeat-call detection windowing.
    struct RepeatScriptedClient {
        max_calls: usize,
        calls: RefCell<usize>,
    }

    impl ModelClient for RepeatScriptedClient {
        fn respond(
            &self,
            _input: ModelRequest,
            _events: &mut dyn StreamEvents,
        ) -> crate::error::AppResult<(ModelResponse, Option<TokenUsage>)> {
            let n = *self.calls.borrow();
            *self.calls.borrow_mut() = n + 1;
            let action = if n < self.max_calls {
                let mut tin = ToolInput::new();
                tin.args.insert("root".to_string(), "/empty".to_string());
                tin.args.insert("max_depth".to_string(), "1".to_string());
                tin.args.insert("limit".to_string(), "5".to_string());
                ModelAction::CallTool {
                    tool_name: "list_files".to_string(),
                    input: tin,
                }
            } else {
                ModelAction::Finish
            };
            Ok((
                ModelResponse {
                    message: format!("step {n}"),
                    action,
                },
                None,
            ))
        }
    }

    #[test]
    fn repeat_detection_first_call_passes_through_clean() {
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("dummy".to_string(), None);
        let client = RepeatScriptedClient {
            max_calls: 1,
            calls: RefCell::new(0),
        };
        let result = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 2,
                    initial_observations: Vec::new(),
                    todos: Rc::new(RefCell::new(TodoList::default())),
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();
        assert_eq!(result.tool_events.len(), 1);
        assert!(
            !result.tool_events[0].output.contains("stuck-warning"),
            "first call must NOT have stuck-warning"
        );
        // It's an OK status (list_files ran, even if /empty doesn't exist — registry returns
        // ToolFailure or empty listing depending on platform).
    }

    #[test]
    fn repeat_detection_second_identical_call_does_not_short_circuit() {
        // 2nd identical call should NOT trigger the short-circuit (only the 3rd does).
        // The stuck-warning is now injected as a separate Observation rather than
        // appended to output.summary (codex review: warning was being eaten by
        // head_trim / Todos summarize when buried in the tail).
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("dummy".to_string(), None);
        let client = RepeatScriptedClient {
            max_calls: 2,
            calls: RefCell::new(0),
        };
        let result = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 3,
                    initial_observations: Vec::new(),
                    todos: Rc::new(RefCell::new(TodoList::default())),
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();
        assert_eq!(result.tool_events.len(), 2, "expected 2 tool events");
        let second = &result.tool_events[1].output;
        assert!(
            !second.contains("repeated identical tool call detected"),
            "2nd call must NOT short-circuit (only the 3rd does); output: {second}"
        );
    }

    #[test]
    fn tool_call_fingerprint_is_delimiter_collision_safe() {
        let left = BTreeMap::from([("a".to_string(), "b|c=d".to_string())]);
        let right = BTreeMap::from([
            ("a".to_string(), "b".to_string()),
            ("c".to_string(), "d".to_string()),
        ]);

        assert_ne!(
            super::tool_call_fingerprint("run_shell", &left),
            super::tool_call_fingerprint("run_shell", &right)
        );
    }

    /// Mock client that captures the `observations` field of every ModelRequest it sees.
    /// Used to verify side effects on the observation stream (e.g., stuck-warning).
    struct ObservationCapturingClient {
        captured_observations: RefCell<Vec<Vec<crate::model::protocol::Observation>>>,
        max_calls: usize,
        calls: RefCell<usize>,
    }

    impl ModelClient for ObservationCapturingClient {
        fn respond(
            &self,
            input: ModelRequest,
            _events: &mut dyn StreamEvents,
        ) -> crate::error::AppResult<(ModelResponse, Option<TokenUsage>)> {
            self.captured_observations
                .borrow_mut()
                .push(input.observations.clone());
            let n = *self.calls.borrow();
            *self.calls.borrow_mut() = n + 1;
            let action = if n < self.max_calls {
                let mut tin = ToolInput::new();
                tin.args.insert("root".to_string(), "/empty".to_string());
                tin.args.insert("max_depth".to_string(), "1".to_string());
                tin.args.insert("limit".to_string(), "5".to_string());
                ModelAction::CallTool {
                    tool_name: "list_files".to_string(),
                    input: tin,
                }
            } else {
                ModelAction::Finish
            };
            Ok((
                ModelResponse {
                    message: format!("step {n}"),
                    action,
                },
                None,
            ))
        }
    }

    struct RecoverableModelErrorClient {
        captured_observations: RefCell<Vec<Vec<crate::model::protocol::Observation>>>,
        calls: RefCell<usize>,
    }

    impl ModelClient for RecoverableModelErrorClient {
        fn respond(
            &self,
            input: ModelRequest,
            _events: &mut dyn StreamEvents,
        ) -> crate::error::AppResult<(ModelResponse, Option<TokenUsage>)> {
            self.captured_observations
                .borrow_mut()
                .push(input.observations.clone());
            let call = *self.calls.borrow();
            *self.calls.borrow_mut() = call + 1;
            if call == 0 {
                return Err(crate::error::tool_failure(
                    "tool_call_parse_failed: expected JSON object",
                ));
            }
            Ok((
                ModelResponse {
                    message: "recovered after model observation".to_string(),
                    action: ModelAction::Finish,
                },
                None,
            ))
        }
    }

    #[test]
    fn run_with_client_recovers_tool_call_parse_failure_as_model_observation() {
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("dummy".to_string(), None);
        let client = RecoverableModelErrorClient {
            captured_observations: RefCell::new(Vec::new()),
            calls: RefCell::new(0),
        };

        let result = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 2,
                    initial_observations: Vec::new(),
                    todos: Rc::new(RefCell::new(TodoList::default())),
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();

        assert_eq!(result.final_message, "recovered after model observation");
        let captures = client.captured_observations.borrow();
        let step2_obs = captures
            .get(1)
            .expect("second model call should receive the failed model observation");
        let model_observation = step2_obs
            .iter()
            .find(|observation| observation.tool_name == "model")
            .expect("expected model parse failure observation");
        assert!(model_observation.is_failure());
        assert!(model_observation.summary.contains("tool_call_parse_failed"));
        assert!(model_observation
            .summary
            .contains("malformed tool arguments"));
    }

    #[test]
    fn repeat_detection_emits_stuck_warning_observation_on_second_identical_call() {
        // After 2nd identical call, the next ModelRequest must include a stuck-warning
        // Observation in its observations field (not buried in the tool's summary).
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("dummy".to_string(), None);
        let client = ObservationCapturingClient {
            captured_observations: RefCell::new(Vec::new()),
            max_calls: 3,
            calls: RefCell::new(0),
        };
        let _ = agent.run_with_client(
            context,
            AgentLoopOptions {
                steps: 4,
                initial_observations: Vec::new(),
                todos: Rc::new(RefCell::new(TodoList::default())),
                ..AgentLoopOptions::default()
            },
            &client,
        );
        let captures = client.captured_observations.borrow();
        // After step 1 (1st list_files), step 2 sees observations including the result —
        // no warning yet. After step 2 (2nd identical), step 3's request observations
        // should include the stuck-warning entry (tool_name == "stuck-warning").
        let step3_obs = captures
            .get(2)
            .expect("at least 3 model calls (step 1 + 2 + 3 setup)");
        let has_warning = step3_obs
            .iter()
            .any(|o| o.tool_name == "stuck-warning" && o.summary.contains("stuck-warning"));
        assert!(
            has_warning,
            "step 3 request should see a stuck-warning Observation: {:?}",
            step3_obs
                .iter()
                .map(|o| (&o.tool_name, &o.summary))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn repeat_detection_third_identical_call_short_circuits_as_failure() {
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("dummy".to_string(), None);
        let client = RepeatScriptedClient {
            max_calls: 5, // emit identical calls forever; loop budget will end it
            calls: RefCell::new(0),
        };
        let result = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 4,
                    initial_observations: Vec::new(),
                    todos: Rc::new(RefCell::new(TodoList::default())),
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();
        // Step 1: list_files. Step 2: list_files (warning). Step 3: list_files (short-circuit).
        // Step 4: list_files (short-circuit).
        assert!(result.tool_events.len() >= 3, "expected ≥3 tool events");
        let third = &result.tool_events[2].output;
        assert!(
            third.contains("repeated identical tool call detected"),
            "3rd call must short-circuit: {third}"
        );
        assert!(matches!(
            result.tool_events[2].status,
            crate::model::protocol::ObservationStatus::Failed
        ));
    }

    #[test]
    fn read_inspection_target_ignores_size_caps_and_skips_non_inspection_tools() {
        let mut read = BTreeMap::new();
        read.insert("path".to_string(), "src/lib.rs".to_string());
        read.insert("max_lines".to_string(), "200".to_string());
        let mut read_smaller = BTreeMap::new();
        read_smaller.insert("path".to_string(), "src/lib.rs".to_string());
        read_smaller.insert("max_lines".to_string(), "40".to_string());
        // Same path, different size cap -> identical inspection target.
        assert_eq!(
            super::read_inspection_target("read_file", &read),
            super::read_inspection_target("read_file", &read_smaller)
        );
        assert_eq!(
            super::read_inspection_target("read_file", &read).as_deref(),
            Some("read_file:src/lib.rs")
        );
        // apply_patch is not a redundant-by-target inspection tool.
        assert_eq!(super::read_inspection_target("apply_patch", &read), None);
        assert!(super::is_edit_tool("apply_patch"));
        assert!(!super::is_edit_tool("read_file"));
    }

    /// Emits `read_file` against the SAME path but a DIFFERENT `max_lines` each
    /// step, so the exact-args fingerprint differs every time. Without Fix A the
    /// model could re-read forever; with it the redundant re-read short-circuits.
    struct VaryingReadScriptedClient {
        max_calls: usize,
        calls: RefCell<usize>,
    }

    impl ModelClient for VaryingReadScriptedClient {
        fn respond(
            &self,
            _input: ModelRequest,
            _events: &mut dyn StreamEvents,
        ) -> crate::error::AppResult<(ModelResponse, Option<TokenUsage>)> {
            let n = *self.calls.borrow();
            *self.calls.borrow_mut() = n + 1;
            let action = if n < self.max_calls {
                let mut tin = ToolInput::new();
                tin.args
                    .insert("path".to_string(), "src/lib.rs".to_string());
                let max_lines = match n {
                    0 => "200",
                    1 => "100",
                    _ => "40",
                };
                tin.args
                    .insert("max_lines".to_string(), max_lines.to_string());
                ModelAction::CallTool {
                    tool_name: "read_file".to_string(),
                    input: tin,
                }
            } else {
                ModelAction::Finish
            };
            Ok((
                ModelResponse {
                    message: format!("step {n}"),
                    action,
                },
                None,
            ))
        }
    }

    #[test]
    fn fix_a_redundant_reread_with_varied_size_cap_short_circuits() {
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("dummy".to_string(), None);
        let client = VaryingReadScriptedClient {
            max_calls: 5,
            calls: RefCell::new(0),
        };
        let result = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 4,
                    initial_observations: Vec::new(),
                    todos: Rc::new(RefCell::new(TodoList::default())),
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();
        // step 1 + 2 read (different max_lines), step 3 must short-circuit even
        // though the exact args differ, because the inspection target repeats.
        assert!(result.tool_events.len() >= 3, "expected >=3 tool events");
        let third = &result.tool_events[2].output;
        assert!(
            third.contains("repeated identical tool call detected"),
            "3rd same-target read must short-circuit despite varied max_lines: {third}"
        );
        assert!(matches!(
            result.tool_events[2].status,
            crate::model::protocol::ObservationStatus::Failed
        ));
    }

    /// Reads a DIFFERENT path each step (so Fix A never triggers) and never
    /// edits, modeling the "understands but won't act" loop.
    struct InspectionNoEditClient {
        captured_observations: RefCell<Vec<Vec<crate::model::protocol::Observation>>>,
        max_calls: usize,
        calls: RefCell<usize>,
    }

    impl ModelClient for InspectionNoEditClient {
        fn respond(
            &self,
            input: ModelRequest,
            _events: &mut dyn StreamEvents,
        ) -> crate::error::AppResult<(ModelResponse, Option<TokenUsage>)> {
            self.captured_observations
                .borrow_mut()
                .push(input.observations.clone());
            let n = *self.calls.borrow();
            *self.calls.borrow_mut() = n + 1;
            let action = if n < self.max_calls {
                let mut tin = ToolInput::new();
                tin.args
                    .insert("path".to_string(), format!("src/does_not_exist_{n}.rs"));
                ModelAction::CallTool {
                    tool_name: "read_file".to_string(),
                    input: tin,
                }
            } else {
                ModelAction::Finish
            };
            Ok((
                ModelResponse {
                    message: format!("step {n}"),
                    action,
                },
                None,
            ))
        }
    }

    #[test]
    fn fix_b_sustained_inspection_without_edit_emits_stuck_directive() {
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("dummy".to_string(), None);
        let client = InspectionNoEditClient {
            captured_observations: RefCell::new(Vec::new()),
            max_calls: 8,
            calls: RefCell::new(0),
        };
        let _ = agent.run_with_client(
            context,
            AgentLoopOptions {
                steps: 8,
                initial_observations: Vec::new(),
                todos: Rc::new(RefCell::new(TodoList::default())),
                ..AgentLoopOptions::default()
            },
            &client,
        );
        let captures = client.captured_observations.borrow();
        let saw_directive = captures.iter().any(|obs| {
            obs.iter()
                .any(|o| o.tool_name == "stuck-directive" && o.is_failure())
        });
        assert!(
            saw_directive,
            "after sustained inspection with no edit, a later request must carry a \
             stuck-directive failure observation"
        );
    }

    #[test]
    fn repeat_detection_classifies_known_read_only_and_unknown_tools() {
        let empty = BTreeMap::new();
        assert_eq!(
            super::repeat_short_circuit_threshold("list_files", &empty),
            2
        );
        assert_eq!(
            super::repeat_short_circuit_threshold("read_file", &empty),
            2
        );
        assert_eq!(super::repeat_short_circuit_threshold("todo_add", &empty), 1);
        assert_eq!(
            super::repeat_short_circuit_threshold("write_file", &empty),
            1
        );
        assert_eq!(
            super::repeat_short_circuit_threshold("mcp__stdio-self__read_file", &empty),
            2
        );
        assert_eq!(
            super::repeat_short_circuit_threshold("mcp__fake__write", &empty),
            1
        );
        assert_eq!(
            super::repeat_short_circuit_threshold(
                "mcp_call",
                &BTreeMap::from([("tool".to_string(), "read_file".to_string())])
            ),
            2
        );
        assert_eq!(
            super::repeat_short_circuit_threshold(
                "mcp_call",
                &BTreeMap::from([("tool".to_string(), "write_file".to_string())])
            ),
            1
        );
    }

    #[test]
    fn repeated_mcp_resource_list_rewrites_to_read_resource_when_uri_is_known() {
        let observations = vec![crate::model::protocol::Observation::ok(
            "mcp_list_resources",
            "MCP remote resources:\n- stdio-self [stdio]: 1 resource(s)\n  - workspace (application/json): Current workspace\n    uri: file:///tmp/deepseek-workspace\n",
        )];
        let rewritten = super::maybe_rewrite_repeated_mcp_resource_list_call(
            "mcp_list_resources",
            &BTreeMap::from([("server".to_string(), "stdio-self".to_string())]),
            1,
            &observations,
            &[
                "mcp_list_resources".to_string(),
                "mcp_read_resource".to_string(),
            ],
        )
        .expect("repeated resource listing should be rewritten");

        assert_eq!(rewritten.tool_name, "mcp_read_resource");
        assert_eq!(rewritten.input.get("server"), Some("stdio-self"));
        assert_eq!(
            rewritten.input.get("uri"),
            Some("file:///tmp/deepseek-workspace")
        );
    }

    struct ScriptedActionsClient {
        captured_observations: RefCell<Vec<Vec<crate::model::protocol::Observation>>>,
        actions: Vec<ModelAction>,
        calls: RefCell<usize>,
    }

    impl ModelClient for ScriptedActionsClient {
        fn respond(
            &self,
            input: ModelRequest,
            _events: &mut dyn StreamEvents,
        ) -> crate::error::AppResult<(ModelResponse, Option<TokenUsage>)> {
            self.captured_observations
                .borrow_mut()
                .push(input.observations.clone());
            let index = *self.calls.borrow();
            *self.calls.borrow_mut() = index + 1;
            let action = self
                .actions
                .get(index)
                .cloned()
                .unwrap_or(ModelAction::Finish);
            Ok((
                ModelResponse {
                    message: format!("scripted step {index}"),
                    action,
                },
                None,
            ))
        }
    }

    #[test]
    fn run_with_client_executes_batched_tool_calls_in_one_model_turn() {
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("inspect workspace".to_string(), None);
        let client = ScriptedActionsClient {
            captured_observations: RefCell::new(Vec::new()),
            actions: vec![
                ModelAction::CallTools(vec![
                    ToolCallRequest {
                        tool_name: "list_files".to_string(),
                        input: ToolInput::new()
                            .with_arg("root", ".")
                            .with_arg("depth", "1"),
                    },
                    ToolCallRequest {
                        tool_name: "read_file".to_string(),
                        input: ToolInput::new().with_arg("path", "Cargo.toml"),
                    },
                ]),
                ModelAction::Finish,
            ],
            calls: RefCell::new(0),
        };

        let result = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 2,
                    emit_progress: false,
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();

        assert_eq!(result.tool_events.len(), 2);
        assert_eq!(result.tool_events[0].tool_name, "list_files");
        assert_eq!(result.tool_events[1].tool_name, "read_file");
        assert!(matches!(
            result.tool_events[0].status,
            crate::model::protocol::ObservationStatus::Ok
        ));
        assert!(matches!(
            result.tool_events[1].status,
            crate::model::protocol::ObservationStatus::Ok
        ));

        let captures = client.captured_observations.borrow();
        let step2_obs = captures.get(1).expect("expected second model request");
        assert!(step2_obs
            .iter()
            .any(|observation| observation.tool_name == "list_files"));
        assert!(step2_obs
            .iter()
            .any(|observation| observation.tool_name == "read_file"));
    }

    struct EnvRestore {
        _guard: MutexGuard<'static, ()>,
        values: Vec<(&'static str, Option<String>)>,
    }

    fn parallel_dispatch_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    impl EnvRestore {
        fn set(values: &[(&'static str, &'static str)]) -> Self {
            let guard = parallel_dispatch_env_lock().lock().unwrap();
            let restore = Self {
                _guard: guard,
                values: values
                    .iter()
                    .map(|(key, _)| (*key, std::env::var(key).ok()))
                    .collect(),
            };
            for (key, value) in values {
                std::env::set_var(key, value);
            }
            restore
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            for (key, value) in self.values.drain(..) {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    #[test]
    fn run_with_client_parallelizes_same_turn_read_only_tool_chunk() {
        let _env = EnvRestore::set(&[
            ("DSCODE_TOOL_DISPATCH", "auto"),
            ("DSCODE_PARALLEL_MAX", "4"),
        ]);
        let probe = "parallel_read_chunk";
        reset_parallel_test_probe(probe);
        let root = unique_tmp("parallel_read_chunk");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "pub fn a() {}\n").unwrap();
        std::fs::write(root.join("src/b.rs"), "pub fn b() {}\n").unwrap();
        let root_arg = root.display().to_string();

        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("inspect two independent listings".to_string(), None);
        let client = ScriptedActionsClient {
            captured_observations: RefCell::new(Vec::new()),
            actions: vec![
                ModelAction::CallTools(vec![
                    ToolCallRequest {
                        tool_name: "list_files".to_string(),
                        input: ToolInput::new()
                            .with_arg("root", &root_arg)
                            .with_arg("max_depth", "1")
                            .with_arg("_parallel_test_probe", probe)
                            .with_arg("_parallel_test_delay_ms", "80"),
                    },
                    ToolCallRequest {
                        tool_name: "list_files".to_string(),
                        input: ToolInput::new()
                            .with_arg("root", root.join("src").display().to_string())
                            .with_arg("max_depth", "1")
                            .with_arg("_parallel_test_probe", probe)
                            .with_arg("_parallel_test_delay_ms", "80"),
                    },
                ]),
                ModelAction::Finish,
            ],
            calls: RefCell::new(0),
        };

        let result = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 2,
                    emit_progress: false,
                    persist_session: false,
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();

        assert_eq!(result.tool_events.len(), 2);
        assert_eq!(result.tool_events[0].tool_name, "list_files");
        assert_eq!(result.tool_events[1].tool_name, "list_files");
        for event in &result.tool_events {
            assert!(event.output.contains("meta.parallel_dispatch=true"));
            assert!(event.output.contains("meta.parallel_chunk_size=2"));
            assert!(event.output.contains("meta.parallel_elapsed_ms="));
        }
        assert!(
            max_parallel_test_probe(probe) >= 2,
            "expected at least two read-only tools in flight, max active was {}",
            max_parallel_test_probe(probe)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn run_with_client_respects_parallel_dispatch_serial_env() {
        let _env = EnvRestore::set(&[
            ("DSCODE_TOOL_DISPATCH", "serial"),
            ("DSCODE_PARALLEL_MAX", "4"),
        ]);
        let probe = "parallel_serial_env";
        reset_parallel_test_probe(probe);
        let root = unique_tmp("parallel_serial_env");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("README.md"), "hello\n").unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
        let root_arg = root.display().to_string();

        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("inspect two independent listings".to_string(), None);
        let client = ScriptedActionsClient {
            captured_observations: RefCell::new(Vec::new()),
            actions: vec![
                ModelAction::CallTools(vec![
                    ToolCallRequest {
                        tool_name: "list_files".to_string(),
                        input: ToolInput::new()
                            .with_arg("root", &root_arg)
                            .with_arg("max_depth", "1")
                            .with_arg("_parallel_test_probe", probe)
                            .with_arg("_parallel_test_delay_ms", "80"),
                    },
                    ToolCallRequest {
                        tool_name: "read_file".to_string(),
                        input: ToolInput::new()
                            .with_arg("path", root.join("README.md").display().to_string())
                            .with_arg("_parallel_test_probe", probe)
                            .with_arg("_parallel_test_delay_ms", "80"),
                    },
                ]),
                ModelAction::Finish,
            ],
            calls: RefCell::new(0),
        };

        let result = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 2,
                    emit_progress: false,
                    persist_session: false,
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();

        assert_eq!(result.tool_events.len(), 2);
        assert!(result
            .tool_events
            .iter()
            .all(|event| !event.output.contains("meta.parallel_dispatch=true")));
        assert_eq!(max_parallel_test_probe(probe), 0);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn run_with_client_downgrades_parallel_chunk_for_repeated_inspection_target() {
        let _env = EnvRestore::set(&[
            ("DSCODE_TOOL_DISPATCH", "auto"),
            ("DSCODE_PARALLEL_MAX", "4"),
        ]);
        let probe = "parallel_repeat_inspection_target";
        reset_parallel_test_probe(probe);
        let root = unique_tmp("parallel-repeat-inspection-target");
        std::fs::create_dir_all(&root).unwrap();
        let readme = root.join("README.md");
        std::fs::write(&readme, "one\ntwo\nthree\n").unwrap();
        let readme_arg = readme.display().to_string();

        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("inspect same file twice".to_string(), None);
        let client = ScriptedActionsClient {
            captured_observations: RefCell::new(Vec::new()),
            actions: vec![
                ModelAction::CallTools(vec![
                    ToolCallRequest {
                        tool_name: "read_file".to_string(),
                        input: ToolInput::new()
                            .with_arg("path", &readme_arg)
                            .with_arg("max_lines", "1")
                            .with_arg("_parallel_test_probe", probe)
                            .with_arg("_parallel_test_delay_ms", "80"),
                    },
                    ToolCallRequest {
                        tool_name: "read_file".to_string(),
                        input: ToolInput::new()
                            .with_arg("path", &readme_arg)
                            .with_arg("max_lines", "2")
                            .with_arg("_parallel_test_probe", probe)
                            .with_arg("_parallel_test_delay_ms", "80"),
                    },
                ]),
                ModelAction::Finish,
            ],
            calls: RefCell::new(0),
        };

        let result = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 2,
                    emit_progress: false,
                    persist_session: false,
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();

        assert_eq!(result.tool_events.len(), 2);
        assert!(result
            .tool_events
            .iter()
            .all(|event| !event.output.contains("meta.parallel_dispatch=true")));
        assert_eq!(
            max_parallel_test_probe(probe),
            0,
            "same inspection target with different caps should fall back to serial repeat handling"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn run_with_client_downgrades_parallel_chunk_for_policy_blocked_tool() {
        let _env = EnvRestore::set(&[
            ("DSCODE_TOOL_DISPATCH", "auto"),
            ("DSCODE_PARALLEL_MAX", "4"),
        ]);
        let probe = "parallel_policy_blocked";
        reset_parallel_test_probe(probe);
        let root = unique_tmp("parallel-policy-blocked");
        let skills = root.join("skills");
        std::fs::create_dir_all(&skills).unwrap();
        std::fs::write(
            skills.join("read-only.toml"),
            r#"
name = "read-only"
description = "Only read one file"
allowed_tools = ["read_file"]

[policy]
require_write_confirmation = false
require_shell_confirmation = false
shell_allowlist = []
"#,
        )
        .unwrap();
        let file = root.join("README.md");
        std::fs::write(&file, "hello\n").unwrap();
        let file_arg = file.display().to_string();

        let mut cfg = crate::config::types::AppConfig::default();
        cfg.workspace.user_skills_dir = skills.display().to_string();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new(
            "inspect with a restricted skill".to_string(),
            Some("read-only".to_string()),
        );
        let client = ScriptedActionsClient {
            captured_observations: RefCell::new(Vec::new()),
            actions: vec![
                ModelAction::CallTools(vec![
                    ToolCallRequest {
                        tool_name: "read_file".to_string(),
                        input: ToolInput::new()
                            .with_arg("path", &file_arg)
                            .with_arg("_parallel_test_probe", probe)
                            .with_arg("_parallel_test_delay_ms", "80"),
                    },
                    ToolCallRequest {
                        tool_name: "git_status".to_string(),
                        input: ToolInput::new()
                            .with_arg("_parallel_test_probe", probe)
                            .with_arg("_parallel_test_delay_ms", "80"),
                    },
                ]),
                ModelAction::Finish,
            ],
            calls: RefCell::new(0),
        };

        let result = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 2,
                    emit_progress: false,
                    persist_session: false,
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();

        assert_eq!(result.tool_events.len(), 2);
        assert!(result
            .tool_events
            .iter()
            .all(|event| !event.output.contains("meta.parallel_dispatch=true")));
        assert!(matches!(
            result.tool_events[1].status,
            crate::model::protocol::ObservationStatus::Failed
        ));
        assert!(result.tool_events[1]
            .output
            .contains("tool blocked by policy: git_status"));
        assert_eq!(
            max_parallel_test_probe(probe),
            0,
            "policy-blocked read chunks must fall back to serial policy handling"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn run_with_client_caps_parallel_safe_chunk_by_env() {
        let _env = EnvRestore::set(&[
            ("DSCODE_TOOL_DISPATCH", "auto"),
            ("DSCODE_PARALLEL_MAX", "2"),
        ]);
        let probe = "parallel_max_cap";
        reset_parallel_test_probe(probe);
        let root = unique_tmp("parallel_max_cap");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("README.md"), "hello\n").unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
        let root_arg = root.display().to_string();

        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("inspect three independent reads".to_string(), None);
        let client = ScriptedActionsClient {
            captured_observations: RefCell::new(Vec::new()),
            actions: vec![
                ModelAction::CallTools(vec![
                    ToolCallRequest {
                        tool_name: "list_files".to_string(),
                        input: ToolInput::new()
                            .with_arg("root", &root_arg)
                            .with_arg("max_depth", "1")
                            .with_arg("_parallel_test_probe", probe)
                            .with_arg("_parallel_test_delay_ms", "80"),
                    },
                    ToolCallRequest {
                        tool_name: "read_file".to_string(),
                        input: ToolInput::new()
                            .with_arg("path", root.join("README.md").display().to_string())
                            .with_arg("_parallel_test_probe", probe)
                            .with_arg("_parallel_test_delay_ms", "80"),
                    },
                    ToolCallRequest {
                        tool_name: "read_file".to_string(),
                        input: ToolInput::new()
                            .with_arg("path", root.join("src/lib.rs").display().to_string())
                            .with_arg("_parallel_test_probe", probe)
                            .with_arg("_parallel_test_delay_ms", "80"),
                    },
                ]),
                ModelAction::Finish,
            ],
            calls: RefCell::new(0),
        };

        let result = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 2,
                    emit_progress: false,
                    persist_session: false,
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();

        assert_eq!(result.tool_events.len(), 3);
        assert!(result.tool_events[0]
            .output
            .contains("meta.parallel_chunk_size=2"));
        assert!(result.tool_events[1]
            .output
            .contains("meta.parallel_chunk_size=2"));
        assert!(!result.tool_events[2]
            .output
            .contains("meta.parallel_dispatch=true"));
        assert_eq!(max_parallel_test_probe(probe), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn run_with_client_parallelizes_extended_local_read_tool_chunk() {
        let _env = EnvRestore::set(&[
            ("DSCODE_TOOL_DISPATCH", "auto"),
            ("DSCODE_PARALLEL_MAX", "4"),
        ]);
        let probe = "parallel_extended_read_chunk";
        reset_parallel_test_probe(probe);
        let root = unique_tmp("parallel_extended_read_chunk");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn add() {}\n").unwrap();
        let root_arg = root.display().to_string();

        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("inspect project map and file names".to_string(), None);
        let client = ScriptedActionsClient {
            captured_observations: RefCell::new(Vec::new()),
            actions: vec![
                ModelAction::CallTools(vec![
                    ToolCallRequest {
                        tool_name: "project_map".to_string(),
                        input: ToolInput::new()
                            .with_arg("path", &root_arg)
                            .with_arg("_parallel_test_probe", probe)
                            .with_arg("_parallel_test_delay_ms", "80"),
                    },
                    ToolCallRequest {
                        tool_name: "file_search".to_string(),
                        input: ToolInput::new()
                            .with_arg("path", &root_arg)
                            .with_arg("query", "lib")
                            .with_arg("_parallel_test_probe", probe)
                            .with_arg("_parallel_test_delay_ms", "80"),
                    },
                ]),
                ModelAction::Finish,
            ],
            calls: RefCell::new(0),
        };

        let result = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 2,
                    emit_progress: false,
                    persist_session: false,
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();

        assert_eq!(result.tool_events.len(), 2);
        assert_eq!(result.tool_events[0].tool_name, "project_map");
        assert_eq!(result.tool_events[1].tool_name, "file_search");
        assert!(
            max_parallel_test_probe(probe) >= 2,
            "expected extended read-only tools in flight, max active was {}",
            max_parallel_test_probe(probe)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn run_with_client_parallelizes_runtime_query_tool_chunk() {
        let _env = EnvRestore::set(&[
            ("DSCODE_TOOL_DISPATCH", "auto"),
            ("DSCODE_PARALLEL_MAX", "4"),
        ]);
        let probe = "parallel_runtime_query_chunk";
        reset_parallel_test_probe(probe);
        let root = unique_tmp("parallel_runtime_query_chunk");
        let mut cfg = crate::config::types::AppConfig::default();
        cfg.workspace.config_dir = root.join(".dscode").display().to_string();
        let store = crate::core::runtime::RuntimeStore::new(root.join(".dscode/runtime"));
        let session = store
            .create_session("Runtime".to_string(), ".".to_string())
            .unwrap();
        let thread = store
            .create_thread_for_session(
                &session.id,
                "Runtime".to_string(),
                ".".to_string(),
                "deepseek-v4-flash".to_string(),
                "agent".to_string(),
            )
            .unwrap();
        let task = store
            .create_task(
                Some(&session.id),
                Some(&thread.id),
                None,
                "agent".to_string(),
                "pending".to_string(),
                "inspect runtime".to_string(),
            )
            .unwrap();

        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("inspect runtime records".to_string(), None);
        let client = ScriptedActionsClient {
            captured_observations: RefCell::new(Vec::new()),
            actions: vec![
                ModelAction::CallTools(vec![
                    ToolCallRequest {
                        tool_name: "task_list".to_string(),
                        input: ToolInput::new()
                            .with_arg("thread_id", &thread.id)
                            .with_arg("_parallel_test_probe", probe)
                            .with_arg("_parallel_test_delay_ms", "80"),
                    },
                    ToolCallRequest {
                        tool_name: "task_read".to_string(),
                        input: ToolInput::new()
                            .with_arg("id", &task.id)
                            .with_arg("_parallel_test_probe", probe)
                            .with_arg("_parallel_test_delay_ms", "80"),
                    },
                ]),
                ModelAction::Finish,
            ],
            calls: RefCell::new(0),
        };

        let result = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 2,
                    emit_progress: false,
                    persist_session: false,
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();

        assert_eq!(result.tool_events.len(), 2);
        assert_eq!(result.tool_events[0].tool_name, "task_list");
        assert_eq!(result.tool_events[1].tool_name, "task_read");
        assert!(
            max_parallel_test_probe(probe) >= 2,
            "expected runtime query tools in flight, max active was {}",
            max_parallel_test_probe(probe)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn run_with_client_keeps_mixed_read_write_batch_serial() {
        let _env = EnvRestore::set(&[
            ("DSCODE_TOOL_DISPATCH", "auto"),
            ("DSCODE_PARALLEL_MAX", "4"),
        ]);
        let probe = "parallel_read_write_barrier";
        reset_parallel_test_probe(probe);
        let root = unique_tmp("parallel_read_write_barrier");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("README.md"), "hello\n").unwrap();
        let todos = Rc::new(RefCell::new(TodoList::default()));
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("read then mutate todo".to_string(), None);
        let client = ScriptedActionsClient {
            captured_observations: RefCell::new(Vec::new()),
            actions: vec![
                ModelAction::CallTools(vec![
                    ToolCallRequest {
                        tool_name: "list_files".to_string(),
                        input: ToolInput::new()
                            .with_arg("root", root.display().to_string())
                            .with_arg("max_depth", "1")
                            .with_arg("_parallel_test_probe", probe)
                            .with_arg("_parallel_test_delay_ms", "80"),
                    },
                    ToolCallRequest {
                        tool_name: "todo_add".to_string(),
                        input: ToolInput::new()
                            .with_arg("content", "Run tests")
                            .with_arg("status", "pending"),
                    },
                ]),
                ModelAction::Finish,
            ],
            calls: RefCell::new(0),
        };

        let result = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 2,
                    emit_progress: false,
                    persist_session: false,
                    todos: todos.clone(),
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();

        assert_eq!(result.tool_events.len(), 2);
        assert_eq!(result.tool_events[0].tool_name, "list_files");
        assert_eq!(result.tool_events[1].tool_name, "todo_add");
        assert_eq!(max_parallel_test_probe(probe), 0);
        assert_eq!(todos.borrow().items.len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn run_with_client_checks_cancellation_before_parallel_read_chunk() {
        let _env = EnvRestore::set(&[
            ("DSCODE_TOOL_DISPATCH", "auto"),
            ("DSCODE_PARALLEL_MAX", "4"),
        ]);
        let probe = "parallel_cancel";
        reset_parallel_test_probe(probe);
        let root = unique_tmp("parallel_cancel");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("README.md"), "hello\n").unwrap();
        let client = ScriptedActionsClient {
            captured_observations: RefCell::new(Vec::new()),
            actions: vec![ModelAction::CallTools(vec![
                ToolCallRequest {
                    tool_name: "list_files".to_string(),
                    input: ToolInput::new()
                        .with_arg("root", root.display().to_string())
                        .with_arg("max_depth", "1")
                        .with_arg("_parallel_test_probe", probe),
                },
                ToolCallRequest {
                    tool_name: "read_file".to_string(),
                    input: ToolInput::new()
                        .with_arg("path", root.join("README.md").display().to_string())
                        .with_arg("_parallel_test_probe", probe),
                },
            ])],
            calls: RefCell::new(0),
        };
        let cancel_check: SharedAgentCancelCheck = Rc::new(RefCell::new(CountingCancelCheck {
            calls: 0,
            cancel_after: 3,
        }));
        let agent = AgentLoop::new(crate::config::types::AppConfig::default());

        let error = agent
            .run_with_client(
                TaskContext::new("inspect then cancel".to_string(), None),
                AgentLoopOptions {
                    steps: 1,
                    emit_progress: false,
                    persist_session: false,
                    cancel_check: Some(cancel_check),
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap_err();

        assert!(error.to_string().contains("agent run cancelled"));
        assert_eq!(max_parallel_test_probe(probe), 0);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn repeat_detection_second_identical_mutating_call_short_circuits_before_execution() {
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("update todos".to_string(), None);
        let todos = Rc::new(RefCell::new(TodoList::default()));
        let action = ModelAction::CallTool {
            tool_name: "todo_add".to_string(),
            input: ToolInput::new()
                .with_arg("content", "Run tests")
                .with_arg("status", "pending"),
        };
        let client = ScriptedActionsClient {
            captured_observations: RefCell::new(Vec::new()),
            actions: vec![action.clone(), action, ModelAction::Finish],
            calls: RefCell::new(0),
        };

        let result = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 3,
                    emit_progress: false,
                    todos: todos.clone(),
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();

        assert_eq!(result.tool_events.len(), 2);
        assert!(matches!(
            result.tool_events[0].status,
            crate::model::protocol::ObservationStatus::Ok
        ));
        assert!(matches!(
            result.tool_events[1].status,
            crate::model::protocol::ObservationStatus::Failed
        ));
        assert!(
            result.tool_events[1]
                .output
                .contains("repeated identical mutating or side-effecting tool call suppressed"),
            "2nd mutating repeat should be suppressed before execution: {}",
            result.tool_events[1].output
        );
        assert_eq!(
            todos.borrow().items.len(),
            1,
            "the second identical todo_add must not execute"
        );
    }

    #[test]
    fn run_with_client_cancels_in_flight_shell_tool() {
        let mut cfg = crate::config::types::AppConfig::default();
        cfg.approval.require_shell_confirmation = false;
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("run cancellable shell".to_string(), None);
        let client = ScriptedActionsClient {
            captured_observations: RefCell::new(Vec::new()),
            actions: vec![ModelAction::CallTool {
                tool_name: "run_shell".to_string(),
                input: ToolInput::new()
                    .with_arg("command", "tail -f /dev/null")
                    .with_arg("cwd", "."),
            }],
            calls: RefCell::new(0),
        };
        let cancel_check: SharedAgentCancelCheck = Rc::new(RefCell::new(CountingCancelCheck {
            calls: 0,
            cancel_after: 4,
        }));

        let started = Instant::now();
        let error = agent
            .run_with_client(
                context,
                AgentLoopOptions {
                    steps: 1,
                    emit_progress: false,
                    cancel_check: Some(cancel_check),
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap_err();

        assert!(error.to_string().contains("agent run cancelled"));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "agent loop should abort the shell process promptly"
        );
        assert_eq!(*client.calls.borrow(), 1);
    }

    #[test]
    fn run_with_client_injects_recovery_hint_after_search_text_returns_no_matches() {
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("find a definitely missing symbol".to_string(), None);
        let dir = unique_tmp("empty_search");
        fs::create_dir_all(&dir).unwrap();
        let client = ScriptedActionsClient {
            captured_observations: RefCell::new(Vec::new()),
            actions: vec![
                ModelAction::CallTool {
                    tool_name: "search_text".to_string(),
                    input: ToolInput::new()
                        .with_arg("root", dir.to_string_lossy().to_string())
                        .with_arg("query", "missing_symbol_that_should_not_exist")
                        .with_arg("limit", "5"),
                },
                ModelAction::Finish,
            ],
            calls: RefCell::new(0),
        };

        let _ = agent.run_with_client(
            context,
            AgentLoopOptions {
                steps: 2,
                initial_observations: Vec::new(),
                todos: Rc::new(RefCell::new(TodoList::default())),
                ..AgentLoopOptions::default()
            },
            &client,
        );

        let captures = client.captured_observations.borrow();
        let step2_obs = captures.get(1).expect("expected second model request");
        let has_hint = step2_obs.iter().any(|observation| {
            observation.tool_name == "recovery_hint"
                && observation.summary.contains("after=search_text")
                && observation.summary.contains("next=list_files")
        });
        assert!(has_hint, "expected recovery_hint after empty search result");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn run_with_client_injects_recovery_hint_after_failed_read_file() {
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("inspect a missing file".to_string(), None);
        let client = ScriptedActionsClient {
            captured_observations: RefCell::new(Vec::new()),
            actions: vec![
                ModelAction::CallTool {
                    tool_name: "read_file".to_string(),
                    input: ToolInput::new().with_arg("path", "definitely-missing-file.rs"),
                },
                ModelAction::Finish,
            ],
            calls: RefCell::new(0),
        };

        let _ = agent.run_with_client(
            context,
            AgentLoopOptions {
                steps: 2,
                initial_observations: Vec::new(),
                todos: Rc::new(RefCell::new(TodoList::default())),
                ..AgentLoopOptions::default()
            },
            &client,
        );

        let captures = client.captured_observations.borrow();
        let step2_obs = captures.get(1).expect("expected second model request");
        let has_hint = step2_obs.iter().any(|observation| {
            observation.tool_name == "recovery_hint"
                && observation.summary.contains("after=read_file")
                && observation.summary.contains("next=search_text")
        });
        assert!(has_hint, "expected recovery_hint after failed read_file");
    }

    #[test]
    fn run_with_client_records_raw_tool_output_for_benchmark_and_dogfood() {
        // Regression guard: ToolEvent.output should keep the raw tool body for
        // benchmark and dogfood assertions, even though observations still use
        // summarize_for_kind(...) for prompt compaction.
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let context = TaskContext::new("dummy".to_string(), None);
        let todos = Rc::new(RefCell::new(TodoList::default()));
        let options = AgentLoopOptions {
            steps: 2,
            initial_observations: Vec::new(),
            todos: todos.clone(),
            ..AgentLoopOptions::default()
        };
        let client = ScriptedClient {
            calls: RefCell::new(0),
        };

        let result = agent.run_with_client(context, options, &client).unwrap();

        // The TodoList was actually mutated (proving the registry got the same Rc):
        let inner = todos.borrow();
        assert_eq!(inner.items.len(), 3);
        assert_eq!(inner.items[1].status, TodoStatus::InProgress);
        drop(inner);

        // The ToolEvent.output must keep the raw todo_write body:
        assert_eq!(result.tool_events.len(), 1);
        let observed = &result.tool_events[0].output;
        assert_eq!(
            observed.lines().count(),
            4,
            "raw output expected: {observed}"
        );
        assert!(observed.starts_with("3 todos"), "observed: {observed}");
        assert!(
            observed.contains("[in_progress]  Bing"),
            "observed: {observed}"
        );
    }

    struct TodoCapturingClient {
        captured_todos: RefCell<Vec<Vec<crate::core::todos::Todo>>>,
    }

    impl ModelClient for TodoCapturingClient {
        fn respond(
            &self,
            input: ModelRequest,
            _events: &mut dyn StreamEvents,
        ) -> crate::error::AppResult<(ModelResponse, Option<TokenUsage>)> {
            self.captured_todos.borrow_mut().push(input.todos.clone());
            Ok((
                ModelResponse {
                    message: "done".to_string(),
                    action: ModelAction::Finish,
                },
                None,
            ))
        }
    }

    fn unique_tmp(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("dscode_loop_runtime_skill_test_{label}_{nanos}"))
    }

    #[test]
    fn run_with_client_seeds_skill_initial_todos_into_first_request() {
        let dir = unique_tmp("skill_seed");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("seeded.toml"),
            r#"
name = "seeded"
description = "seed test"
allowed_tools = ["todo_write", "list_files"]
triggers = ["seed"]

[[initial_todos]]
content = "Inspect the repo"
active_form = "Inspecting the repo"
status = "in_progress"

[[initial_todos]]
content = "Summarize findings"
active_form = "Summarizing findings"
status = "pending"

[policy]
require_write_confirmation = false
require_shell_confirmation = false
shell_allowlist = []
"#,
        )
        .unwrap();

        let mut cfg = crate::config::types::AppConfig::default();
        cfg.workspace.user_skills_dir = dir.to_string_lossy().to_string();
        let agent = AgentLoop::new(cfg);
        let client = TodoCapturingClient {
            captured_todos: RefCell::new(Vec::new()),
        };

        let _ = agent.run_with_client(
            TaskContext::new("seed todos".to_string(), Some("seeded".to_string())),
            AgentLoopOptions {
                steps: 1,
                initial_observations: Vec::new(),
                todos: Rc::new(RefCell::new(TodoList::default())),
                ..AgentLoopOptions::default()
            },
            &client,
        );

        let captured = client.captured_todos.borrow();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].len(), 2);
        assert_eq!(captured[0][0].content, "Inspect the repo");
        assert_eq!(captured[0][0].status, TodoStatus::InProgress);
        assert_eq!(captured[0][1].content, "Summarize findings");
        assert_eq!(captured[0][1].status, TodoStatus::Pending);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn run_with_client_auto_selects_skill_from_triggers() {
        let dir = unique_tmp("skill_auto");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("write-tests.toml"),
            r#"
name = "write-tests"
description = "auto select test"
allowed_tools = ["todo_write", "list_files"]
triggers = ["write tests", "coverage"]

[[initial_todos]]
content = "Write the first failing test"
active_form = "Writing the first failing test"
status = "in_progress"

[policy]
require_write_confirmation = false
require_shell_confirmation = false
shell_allowlist = []
"#,
        )
        .unwrap();

        let mut cfg = crate::config::types::AppConfig::default();
        cfg.workspace.user_skills_dir = dir.to_string_lossy().to_string();
        let agent = AgentLoop::new(cfg);
        let client = TodoCapturingClient {
            captured_todos: RefCell::new(Vec::new()),
        };

        let _ = agent.run_with_client(
            TaskContext::new("please write tests for the parser".to_string(), None),
            AgentLoopOptions {
                steps: 1,
                initial_observations: Vec::new(),
                todos: Rc::new(RefCell::new(TodoList::default())),
                ..AgentLoopOptions::default()
            },
            &client,
        );

        let captured = client.captured_todos.borrow();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].len(), 1);
        assert_eq!(captured[0][0].content, "Write the first failing test");
        assert_eq!(captured[0][0].status, TodoStatus::InProgress);

        let _ = fs::remove_dir_all(dir);
    }

    struct DispatchingClient {
        calls: RefCell<usize>,
    }

    impl ModelClient for DispatchingClient {
        fn respond(
            &self,
            _input: ModelRequest,
            _events: &mut dyn StreamEvents,
        ) -> crate::error::AppResult<(ModelResponse, Option<TokenUsage>)> {
            let n = *self.calls.borrow();
            *self.calls.borrow_mut() = n + 1;
            let action = if n == 0 {
                ModelAction::CallTool {
                    tool_name: "dispatch_subagent".to_string(),
                    input: ToolInput::new()
                        .with_arg("task", "inspect repository layout")
                        .with_arg("steps", "2"),
                }
            } else {
                ModelAction::Finish
            };
            Ok((
                ModelResponse {
                    message: format!("dispatch step {n}"),
                    action,
                },
                None,
            ))
        }
    }

    #[cfg(unix)]
    struct HookBlockingClient {
        calls: RefCell<usize>,
    }

    #[cfg(unix)]
    impl ModelClient for HookBlockingClient {
        fn respond(
            &self,
            _input: ModelRequest,
            _events: &mut dyn StreamEvents,
        ) -> crate::error::AppResult<(ModelResponse, Option<TokenUsage>)> {
            let n = *self.calls.borrow();
            *self.calls.borrow_mut() = n + 1;
            let action = if n == 0 {
                ModelAction::CallTool {
                    tool_name: "list_files".to_string(),
                    input: ToolInput::new()
                        .with_arg("root", ".")
                        .with_arg("max_depth", "1"),
                }
            } else {
                ModelAction::Finish
            };
            Ok((
                ModelResponse {
                    message: format!("hook step {n}"),
                    action,
                },
                None,
            ))
        }
    }

    #[cfg(unix)]
    struct ShellEnvHookClient {
        calls: RefCell<usize>,
    }

    #[cfg(unix)]
    impl ModelClient for ShellEnvHookClient {
        fn respond(
            &self,
            _input: ModelRequest,
            _events: &mut dyn StreamEvents,
        ) -> crate::error::AppResult<(ModelResponse, Option<TokenUsage>)> {
            let n = *self.calls.borrow();
            *self.calls.borrow_mut() = n + 1;
            let action = if n == 0 {
                ModelAction::CallTool {
                    tool_name: "run_shell".to_string(),
                    input: ToolInput::new().with_arg("command", "echo $DSCODE_SHELL_ENV_SECRET"),
                }
            } else {
                ModelAction::Finish
            };
            Ok((
                ModelResponse {
                    message: format!("shell env step {n}"),
                    action,
                },
                None,
            ))
        }
    }

    #[test]
    #[cfg(unix)]
    fn run_with_client_blocks_tool_when_pre_tool_hook_denies() {
        use std::os::unix::fs::PermissionsExt;

        let root = unique_tmp("pre_tool_hook");
        let hook_dir = root.join("hooks/pre_tool_use");
        fs::create_dir_all(&hook_dir).unwrap();
        let hook_path = hook_dir.join("10-block");
        fs::write(
            &hook_path,
            "#!/bin/sh\nprintf 'blocked by pre hook' >&2\nexit 7\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&hook_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&hook_path, permissions).unwrap();

        let mut cfg = crate::config::types::AppConfig::default();
        cfg.hooks.enabled = true;
        cfg.hooks.project_dir = root.join("hooks").display().to_string();
        let agent = AgentLoop::new(cfg);
        let client = HookBlockingClient {
            calls: RefCell::new(0),
        };

        let result = agent
            .run_with_client(
                TaskContext::new("inspect with hook".to_string(), None),
                AgentLoopOptions {
                    steps: 2,
                    emit_progress: false,
                    persist_session: false,
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();

        assert_eq!(result.tool_events.len(), 1);
        let event = &result.tool_events[0];
        assert_eq!(event.tool_name, "list_files");
        assert_eq!(
            event.status,
            crate::model::protocol::ObservationStatus::Failed
        );
        assert!(event.output.contains("blocked by pre hook"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    #[cfg(unix)]
    fn run_with_client_applies_shell_env_hook_without_recording_secret_input() {
        use std::os::unix::fs::PermissionsExt;

        let root = unique_tmp("shell_env_hook");
        let hook_dir = root.join("hooks/shell_env");
        fs::create_dir_all(&hook_dir).unwrap();
        let hook_path = hook_dir.join("10-env");
        fs::write(
            &hook_path,
            "#!/bin/sh\nprintf 'DSCODE_SHELL_ENV_SECRET=hook-secret\\n'\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&hook_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&hook_path, permissions).unwrap();

        let mut cfg = crate::config::types::AppConfig::default();
        cfg.approval.require_shell_confirmation = false;
        cfg.hooks.enabled = true;
        cfg.hooks.project_dir = root.join("hooks").display().to_string();
        let agent = AgentLoop::new(cfg);
        let client = ShellEnvHookClient {
            calls: RefCell::new(0),
        };

        let result = agent
            .run_with_client(
                TaskContext::new("shell env hook".to_string(), None),
                AgentLoopOptions {
                    steps: 2,
                    emit_progress: false,
                    persist_session: false,
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();

        assert_eq!(result.tool_events.len(), 1);
        let event = &result.tool_events[0];
        assert_eq!(event.tool_name, "run_shell");
        assert!(event.output.contains("hook-secret"), "{}", event.output);
        assert_eq!(
            event.input.get("command").map(String::as_str),
            Some("echo $DSCODE_SHELL_ENV_SECRET")
        );
        assert!(
            !event
                .input
                .values()
                .any(|value| value.contains("hook-secret")),
            "{:?}",
            event.input
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn run_with_client_executes_dispatch_subagent_with_isolated_child_loop() {
        let cfg = crate::config::types::AppConfig::default();
        let agent = AgentLoop::new(cfg);
        let client = DispatchingClient {
            calls: RefCell::new(0),
        };
        let todos = Rc::new(RefCell::new(TodoList::default()));
        todos.borrow_mut().replace(vec![
            crate::core::todos::Todo {
                content: "Inspect repository layout".to_string(),
                active_form: "Inspecting repository layout".to_string(),
                status: TodoStatus::InProgress,
            },
            crate::core::todos::Todo {
                content: "Implement the requested changes".to_string(),
                active_form: "Implementing the requested changes".to_string(),
                status: TodoStatus::Pending,
            },
        ]);

        let result = agent
            .run_with_client(
                TaskContext::new("delegate repository inspection".to_string(), None),
                AgentLoopOptions {
                    steps: 2,
                    initial_observations: Vec::new(),
                    todos: todos.clone(),
                    ..AgentLoopOptions::default()
                },
                &client,
            )
            .unwrap();

        assert_eq!(result.tool_events.len(), 1);
        let event = &result.tool_events[0];
        assert_eq!(event.tool_name, "dispatch_subagent");
        assert!(event.output.contains("subagent finished task"));
        assert!(event.output.contains("child tool calls:"));
        assert!(event.output.contains("parent todos auto-advanced"));

        let todos = todos.borrow();
        assert_eq!(todos.items[0].status, TodoStatus::Completed);
        assert_eq!(todos.items[1].status, TodoStatus::InProgress);
    }
}
