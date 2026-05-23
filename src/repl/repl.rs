use crate::config::types::AppConfig;
use crate::error::AppResult;
use crate::repl::transcript::Transcript;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

pub const DEFAULT_BUDGET: usize = 20;

#[derive(Debug)]
pub enum ControlFlow {
    Continue,
    Quit,
}

#[derive(Debug)]
pub struct Repl {
    pub config: AppConfig,
    pub transcript: Transcript,
    pub budget: usize,
    pub skill: Option<String>,
    pub tokens_prompt: u64,
    pub tokens_completion: u64,
    pub todos: std::rc::Rc<std::cell::RefCell<crate::core::todos::TodoList>>,
    pub last_rollback_snapshot_id: Option<String>,
}

impl Repl {
    pub fn new(config: AppConfig, skill: Option<String>) -> Self {
        Self {
            config,
            transcript: Transcript::default(),
            budget: DEFAULT_BUDGET,
            skill,
            tokens_prompt: 0,
            tokens_completion: 0,
            todos: std::rc::Rc::new(std::cell::RefCell::new(
                crate::core::todos::TodoList::default(),
            )),
            last_rollback_snapshot_id: None,
        }
    }

    pub fn run(&mut self) -> AppResult<()> {
        use std::io::{self, IsTerminal};
        if !io::stdin().is_terminal() {
            let bin = invoked_binary_name();
            return Err(crate::error::policy_denied(
                format!(
                    "{bin} interactive mode requires a TTY; use `{bin} run \"task\"` for one-shot tasks"
                ),
        ));
        }
        self.run_interactive(&mut io::stderr())
    }

    fn run_interactive<W: std::io::Write>(&mut self, prompt_sink: &mut W) -> AppResult<()> {
        let mut editor = ReplLineEditor::default();
        let turn_cancel = ReplTurnCancel::install()?;
        loop {
            let Some(line) = read_interactive_line(&mut editor, prompt_sink, &self.config)? else {
                return Ok(());
            };
            turn_cancel.reset();
            let outcome = self.handle_line_with_cancel(&line, Some(turn_cancel.shared_check()));
            let was_cancelled = outcome.as_ref().err().is_some_and(|error| {
                turn_cancel.was_requested() && is_agent_run_cancelled_error(error.as_ref())
            });
            turn_cancel.reset();
            match outcome {
                Ok(ControlFlow::Continue) => continue,
                Ok(ControlFlow::Quit) => return Ok(()),
                Err(_error) if was_cancelled => {
                    writeln!(prompt_sink, "cancelled current turn")?;
                    prompt_sink.flush()?;
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub fn run_with_reader<R: std::io::BufRead, W: std::io::Write>(
        &mut self,
        reader: &mut R,
        prompt_sink: &mut W,
    ) -> AppResult<()> {
        let mut buffer = String::new();
        loop {
            let _ = write!(prompt_sink, "> ");
            let _ = prompt_sink.flush();
            buffer.clear();
            let bytes = reader.read_line(&mut buffer)?;
            if bytes == 0 {
                return Ok(());
            }
            let line = buffer.trim_end_matches('\n').trim_end_matches('\r');
            match self.handle_line(line)? {
                ControlFlow::Continue => continue,
                ControlFlow::Quit => return Ok(()),
            }
        }
    }

    pub fn handle_line(&mut self, line: &str) -> AppResult<ControlFlow> {
        self.handle_line_with_cancel(line, None)
    }

    fn handle_line_with_cancel(
        &mut self,
        line: &str,
        cancel_check: Option<crate::core::loop_runtime::SharedAgentCancelCheck>,
    ) -> AppResult<ControlFlow> {
        if line.trim().is_empty() {
            return Ok(ControlFlow::Continue);
        }
        match crate::repl::slash::try_handle_slash(self, line)? {
            crate::repl::slash::SlashOutcome::Quit => return Ok(ControlFlow::Quit),
            crate::repl::slash::SlashOutcome::Continue => return Ok(ControlFlow::Continue),
            crate::repl::slash::SlashOutcome::Submit(prompt) => {
                return self.dispatch_prompt_with_cancel(prompt, cancel_check);
            }
            crate::repl::slash::SlashOutcome::NotASlash => {}
        }

        self.dispatch_prompt_with_cancel(line.to_string(), cancel_check)
    }

    fn dispatch_prompt_with_cancel(
        &mut self,
        prompt: String,
        cancel_check: Option<crate::core::loop_runtime::SharedAgentCancelCheck>,
    ) -> AppResult<ControlFlow> {
        if shared_cancel_requested(cancel_check.as_ref())? {
            return Err(crate::error::app_error("agent run cancelled"));
        }
        let transcript_len = self.transcript.turns.len();
        let previous_snapshot_id = self.last_rollback_snapshot_id.clone();
        let snapshot_id = self.create_turn_snapshot(&prompt);
        if snapshot_id.is_some() {
            self.last_rollback_snapshot_id = snapshot_id.clone();
        }
        self.transcript.push_user(&prompt);
        let prompt = self.transcript.render_for_prompt();
        let context = crate::core::context::TaskContext::new(prompt, self.skill.clone());
        let runtime = crate::core::loop_runtime::AgentLoop::new(self.config.clone());
        let result = runtime.run_with(
            context,
            crate::core::loop_runtime::AgentLoopOptions {
                steps: self.budget,
                initial_observations: Vec::new(),
                todos: self.todos.clone(),
                cancel_check,
                ..crate::core::loop_runtime::AgentLoopOptions::default()
            },
        );
        let result = match result {
            Ok(result) => result,
            Err(error) if is_agent_run_cancelled_error(error.as_ref()) => {
                self.transcript.turns.truncate(transcript_len);
                self.last_rollback_snapshot_id = previous_snapshot_id;
                return Err(error);
            }
            Err(error) => return Err(error),
        };

        self.tokens_prompt += result.usage.prompt;
        self.tokens_completion += result.usage.completion;
        let had_tool_events = !result.tool_events.is_empty();
        for event in result.tool_events {
            self.transcript
                .push_tool(event.tool_name, event.input, event.output, event.status);
        }
        if !result.final_message.is_empty() {
            self.transcript.push_assistant(result.final_message);
        }
        if had_tool_events {
            if let Some(snapshot_id) = snapshot_id {
                println!("rollback snapshot: {snapshot_id} (/revert_turn last --apply)");
            }
        }
        Ok(ControlFlow::Continue)
    }

    fn create_turn_snapshot(&self, prompt: &str) -> Option<String> {
        let cwd = std::env::current_dir().ok()?;
        let store = crate::core::rollback::RollbackStore::new(
            std::path::PathBuf::from(&self.config.workspace.config_dir).join("rollback"),
        );
        store
            .create_snapshot(&cwd, repl_turn_snapshot_label(prompt))
            .ok()
            .map(|snapshot| snapshot.id)
    }
}

#[derive(Clone)]
struct ReplTurnCancel {
    requested: Arc<AtomicBool>,
}

impl ReplTurnCancel {
    fn install() -> AppResult<Self> {
        Ok(Self {
            requested: repl_sigint_cancel_flag()?,
        })
    }

    #[cfg(test)]
    fn new_for_tests() -> Self {
        Self {
            requested: Arc::new(AtomicBool::new(false)),
        }
    }

    #[cfg(test)]
    fn request(&self) {
        self.requested.store(true, Ordering::SeqCst);
    }

    fn reset(&self) {
        self.requested.store(false, Ordering::SeqCst);
    }

    fn was_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    fn shared_check(&self) -> crate::core::loop_runtime::SharedAgentCancelCheck {
        std::rc::Rc::new(std::cell::RefCell::new(self.clone()))
    }
}

impl crate::core::loop_runtime::AgentCancelCheck for ReplTurnCancel {
    fn is_cancelled(&mut self) -> AppResult<bool> {
        Ok(self.was_requested())
    }
}

fn repl_sigint_cancel_flag() -> AppResult<Arc<AtomicBool>> {
    static FLAG: OnceLock<Arc<AtomicBool>> = OnceLock::new();
    static INSTALL: OnceLock<Result<(), String>> = OnceLock::new();

    let flag = FLAG
        .get_or_init(|| Arc::new(AtomicBool::new(false)))
        .clone();
    let install_result = INSTALL.get_or_init(|| install_repl_sigint_handler(flag.clone()));
    if let Err(error) = install_result {
        return Err(crate::error::app_error(format!(
            "failed to install REPL Ctrl+C cancellation handler: {error}"
        )));
    }
    Ok(flag)
}

fn install_repl_sigint_handler(flag: Arc<AtomicBool>) -> Result<(), String> {
    signal_hook::flag::register_conditional_default(signal_hook::consts::SIGINT, flag.clone())
        .map_err(|error| error.to_string())?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, flag)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn shared_cancel_requested(
    cancel_check: Option<&crate::core::loop_runtime::SharedAgentCancelCheck>,
) -> AppResult<bool> {
    let Some(cancel_check) = cancel_check else {
        return Ok(false);
    };
    crate::core::loop_runtime::AgentCancelCheck::is_cancelled(&mut *cancel_check.borrow_mut())
}

fn is_agent_run_cancelled_error(error: &(dyn std::error::Error + 'static)) -> bool {
    error.to_string().contains("agent run cancelled")
}

#[derive(Debug, Default)]
struct ReplLineEditor {
    buffer: String,
    cursor: usize,
    history: Vec<String>,
    history_index: Option<usize>,
    draft: String,
}

#[derive(Debug, PartialEq, Eq)]
enum LineEditorOutcome {
    Continue,
    Redraw,
    Submit(String),
    Eof,
}

impl ReplLineEditor {
    fn start_line(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
        self.history_index = None;
        self.draft.clear();
    }

    fn handle_key(&mut self, event: KeyEvent) -> LineEditorOutcome {
        match (event.code, event.modifiers) {
            (KeyCode::Enter | KeyCode::Char('\n') | KeyCode::Char('\r'), _) => {
                let line = self.buffer.clone();
                self.remember_submitted_line(&line);
                LineEditorOutcome::Submit(line)
            }
            (KeyCode::Char('j') | KeyCode::Char('m'), KeyModifiers::CONTROL) => {
                let line = self.buffer.clone();
                self.remember_submitted_line(&line);
                LineEditorOutcome::Submit(line)
            }
            (KeyCode::Char('d'), KeyModifiers::CONTROL) if self.buffer.is_empty() => {
                LineEditorOutcome::Eof
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => LineEditorOutcome::Eof,
            (KeyCode::Char('a'), KeyModifiers::CONTROL) | (KeyCode::Home, _) => {
                self.cursor = 0;
                LineEditorOutcome::Redraw
            }
            (KeyCode::Char('e'), KeyModifiers::CONTROL) | (KeyCode::End, _) => {
                self.cursor = self.buffer.len();
                LineEditorOutcome::Redraw
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                self.detach_history_for_edit();
                self.buffer.clear();
                self.cursor = 0;
                LineEditorOutcome::Redraw
            }
            (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                self.detach_history_for_edit();
                self.buffer.truncate(self.cursor);
                LineEditorOutcome::Redraw
            }
            (KeyCode::Char('w'), KeyModifiers::CONTROL) => {
                self.detach_history_for_edit();
                self.delete_previous_word();
                LineEditorOutcome::Redraw
            }
            (KeyCode::Backspace, _) => {
                self.detach_history_for_edit();
                self.delete_before_cursor();
                LineEditorOutcome::Redraw
            }
            (KeyCode::Delete, _) => {
                self.detach_history_for_edit();
                self.delete_at_cursor();
                LineEditorOutcome::Redraw
            }
            (KeyCode::Left, _) => {
                self.move_left();
                LineEditorOutcome::Redraw
            }
            (KeyCode::Right, _) => {
                self.move_right();
                LineEditorOutcome::Redraw
            }
            (KeyCode::Up, _) => {
                self.history_previous();
                LineEditorOutcome::Redraw
            }
            (KeyCode::Down, _) => {
                self.history_next();
                LineEditorOutcome::Redraw
            }
            (KeyCode::Esc, _) => {
                self.detach_history_for_edit();
                self.buffer.clear();
                self.cursor = 0;
                LineEditorOutcome::Redraw
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.contains(KeyModifiers::CONTROL)
                    && !modifiers.contains(KeyModifiers::ALT) =>
            {
                self.detach_history_for_edit();
                self.buffer.insert(self.cursor, ch);
                self.cursor += ch.len_utf8();
                LineEditorOutcome::Redraw
            }
            _ => LineEditorOutcome::Continue,
        }
    }

    fn remember_submitted_line(&mut self, line: &str) {
        let line = line.trim_end_matches(['\n', '\r']);
        if line.trim().is_empty() {
            return;
        }
        if self.history.last().is_some_and(|previous| previous == line) {
            return;
        }
        self.history.push(line.to_string());
    }

    fn history_previous(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next_index = match self.history_index {
            Some(index) if index > 0 => index - 1,
            Some(index) => index,
            None => {
                self.draft = self.buffer.clone();
                self.history.len() - 1
            }
        };
        self.history_index = Some(next_index);
        self.buffer = self.history[next_index].clone();
        self.cursor = self.buffer.len();
    }

    fn history_next(&mut self) {
        let Some(index) = self.history_index else {
            return;
        };
        if index + 1 < self.history.len() {
            let next_index = index + 1;
            self.history_index = Some(next_index);
            self.buffer = self.history[next_index].clone();
        } else {
            self.history_index = None;
            self.buffer = self.draft.clone();
            self.draft.clear();
        }
        self.cursor = self.buffer.len();
    }

    fn detach_history_for_edit(&mut self) {
        if self.history_index.is_some() {
            self.history_index = None;
            self.draft.clear();
        }
    }

    fn move_left(&mut self) {
        self.cursor = previous_char_boundary(&self.buffer, self.cursor);
    }

    fn move_right(&mut self) {
        self.cursor = next_char_boundary(&self.buffer, self.cursor);
    }

    fn delete_before_cursor(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let previous = previous_char_boundary(&self.buffer, self.cursor);
        self.buffer.drain(previous..self.cursor);
        self.cursor = previous;
    }

    fn delete_at_cursor(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let next = next_char_boundary(&self.buffer, self.cursor);
        self.buffer.drain(self.cursor..next);
    }

    fn delete_previous_word(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut index = self.cursor;
        while index > 0 {
            let previous = previous_char_boundary(&self.buffer, index);
            let ch = self.buffer[previous..index].chars().next().unwrap_or(' ');
            if !ch.is_whitespace() {
                break;
            }
            index = previous;
        }
        while index > 0 {
            let previous = previous_char_boundary(&self.buffer, index);
            let ch = self.buffer[previous..index].chars().next().unwrap_or(' ');
            if ch.is_whitespace() {
                break;
            }
            index = previous;
        }
        self.buffer.drain(index..self.cursor);
        self.cursor = index;
    }
}

fn read_interactive_line<W: std::io::Write>(
    editor: &mut ReplLineEditor,
    prompt_sink: &mut W,
    config: &AppConfig,
) -> AppResult<Option<String>> {
    let _raw = RawModeGuard::enable()?;
    editor.start_line();
    write!(prompt_sink, "> ")?;
    prompt_sink.flush()?;
    loop {
        let Event::Key(key) = event::read()? else {
            continue;
        };
        let keys = if is_escape_key(&key) {
            read_escape_sequence_keys()?
        } else {
            vec![key]
        };
        for key in keys {
            if is_tab_key(&key) {
                match editor.complete(config)? {
                    ReplCompletionOutcome::None => {}
                    ReplCompletionOutcome::Applied => redraw_interactive_line(prompt_sink, editor)?,
                    ReplCompletionOutcome::Suggestions(suggestions) => {
                        write!(prompt_sink, "\r\n{}\r\n", suggestions.join("  "))?;
                        redraw_interactive_line(prompt_sink, editor)?;
                    }
                }
                continue;
            }
            match editor.handle_key(key) {
                LineEditorOutcome::Continue => {}
                LineEditorOutcome::Redraw => redraw_interactive_line(prompt_sink, editor)?,
                LineEditorOutcome::Submit(line) => {
                    write!(prompt_sink, "\r\n")?;
                    prompt_sink.flush()?;
                    return Ok(Some(line));
                }
                LineEditorOutcome::Eof => {
                    write!(prompt_sink, "\r\n")?;
                    prompt_sink.flush()?;
                    return Ok(None);
                }
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ReplCompletionOutcome {
    None,
    Applied,
    Suggestions(Vec<String>),
}

impl ReplLineEditor {
    fn complete(&mut self, config: &AppConfig) -> AppResult<ReplCompletionOutcome> {
        let Some(completion) =
            crate::repl::slash::complete_repl_input(config, &self.buffer, self.cursor)?
        else {
            return Ok(ReplCompletionOutcome::None);
        };
        match completion {
            crate::repl::slash::ReplCompletion::Applied {
                start,
                end,
                replacement,
            } => {
                self.detach_history_for_edit();
                self.buffer.replace_range(start..end, &replacement);
                self.cursor = start + replacement.len();
                Ok(ReplCompletionOutcome::Applied)
            }
            crate::repl::slash::ReplCompletion::Suggestions(suggestions) => {
                Ok(ReplCompletionOutcome::Suggestions(suggestions))
            }
        }
    }
}

fn is_escape_key(event: &KeyEvent) -> bool {
    matches!(&event.code, KeyCode::Esc | KeyCode::Char('\u{1b}'))
        && event.modifiers == KeyModifiers::NONE
}

fn is_tab_key(event: &KeyEvent) -> bool {
    matches!(event.code, KeyCode::Tab | KeyCode::Char('\t'))
        || (event.code == KeyCode::Char('i') && event.modifiers == KeyModifiers::CONTROL)
}

fn read_escape_sequence_keys() -> std::io::Result<Vec<KeyEvent>> {
    let escape = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    let Some(first) = poll_next_key_event(Duration::from_millis(20))? else {
        return Ok(vec![escape]);
    };
    let Some(first_char) = plain_char_key(&first) else {
        return Ok(vec![escape, first]);
    };
    if first_char != '[' && first_char != 'O' {
        return Ok(vec![escape, first]);
    }

    let Some(second) = poll_next_key_event(Duration::from_millis(20))? else {
        return Ok(vec![escape, first]);
    };
    let Some(second_char) = plain_char_key(&second) else {
        return Ok(vec![escape, first, second]);
    };

    let mut sequence = String::with_capacity(3);
    sequence.push(first_char);
    sequence.push(second_char);
    if let Some(code) = key_code_from_ansi_sequence(&sequence) {
        return Ok(vec![KeyEvent::new(code, KeyModifiers::NONE)]);
    }

    if matches!(second_char, '1' | '3' | '4' | '7' | '8') {
        let Some(third) = poll_next_key_event(Duration::from_millis(20))? else {
            return Ok(vec![escape, first, second]);
        };
        let Some(third_char) = plain_char_key(&third) else {
            return Ok(vec![escape, first, second, third]);
        };
        sequence.push(third_char);
        if let Some(code) = key_code_from_ansi_sequence(&sequence) {
            return Ok(vec![KeyEvent::new(code, KeyModifiers::NONE)]);
        }
        return Ok(vec![escape, first, second, third]);
    }

    Ok(vec![escape, first, second])
}

fn poll_next_key_event(timeout: Duration) -> std::io::Result<Option<KeyEvent>> {
    if !event::poll(timeout)? {
        return Ok(None);
    }
    loop {
        if let Event::Key(key) = event::read()? {
            return Ok(Some(key));
        }
        if !event::poll(Duration::from_millis(0))? {
            return Ok(None);
        }
    }
}

fn plain_char_key(event: &KeyEvent) -> Option<char> {
    if event.modifiers != KeyModifiers::NONE {
        return None;
    }
    match event.code {
        KeyCode::Char(ch) => Some(ch),
        _ => None,
    }
}

fn key_code_from_ansi_sequence(sequence: &str) -> Option<KeyCode> {
    match sequence {
        "[A" => Some(KeyCode::Up),
        "[B" => Some(KeyCode::Down),
        "[C" => Some(KeyCode::Right),
        "[D" => Some(KeyCode::Left),
        "[H" | "[1~" | "[7~" | "OH" => Some(KeyCode::Home),
        "[F" | "[4~" | "[8~" | "OF" => Some(KeyCode::End),
        "[3~" => Some(KeyCode::Delete),
        _ => None,
    }
}

fn redraw_interactive_line<W: std::io::Write>(
    prompt_sink: &mut W,
    editor: &ReplLineEditor,
) -> std::io::Result<()> {
    write!(prompt_sink, "\r\x1b[2K> {}", editor.buffer)?;
    let chars_right = editor.buffer[editor.cursor..].chars().count();
    if chars_right > 0 {
        write!(prompt_sink, "\x1b[{chars_right}D")?;
    }
    prompt_sink.flush()
}

struct RawModeGuard;

impl RawModeGuard {
    fn enable() -> std::io::Result<Self> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

fn previous_char_boundary(value: &str, index: usize) -> usize {
    value[..index]
        .char_indices()
        .last()
        .map_or(0, |(pos, _)| pos)
}

fn next_char_boundary(value: &str, index: usize) -> usize {
    if index >= value.len() {
        return value.len();
    }
    index
        + value[index..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(0)
}

fn repl_turn_snapshot_label(prompt: &str) -> String {
    let mut summary = prompt
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(80)
        .collect::<String>();
    if summary.is_empty() {
        summary = "empty prompt".to_string();
    }
    format!("REPL turn before: {summary}")
}

fn invoked_binary_name() -> String {
    std::env::args()
        .next()
        .and_then(|path| {
            std::path::Path::new(&path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "deepseek".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::AppConfig;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-repl-{label}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn run_git(cwd: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn new_starts_with_default_budget_and_empty_transcript() {
        let r = Repl::new(AppConfig::default(), None);
        assert_eq!(r.budget, DEFAULT_BUDGET);
        assert!(r.transcript.turns.is_empty());
        assert_eq!(r.tokens_prompt, 0);
        assert_eq!(r.tokens_completion, 0);
        assert!(r.skill.is_none());
        assert!(r.last_rollback_snapshot_id.is_none());
    }

    #[test]
    fn new_keeps_skill_when_provided() {
        let r = Repl::new(AppConfig::default(), Some("pr-review".to_string()));
        assert_eq!(r.skill.as_deref(), Some("pr-review"));
    }

    #[test]
    fn handle_line_returns_continue_for_empty_input() {
        let mut r = Repl::new(AppConfig::default(), None);
        let cf = r.handle_line("").unwrap();
        assert!(matches!(cf, ControlFlow::Continue));
        assert!(r.transcript.turns.is_empty());
    }

    #[test]
    fn handle_line_returns_continue_for_whitespace() {
        let mut r = Repl::new(AppConfig::default(), None);
        assert!(matches!(
            r.handle_line("   \t  ").unwrap(),
            ControlFlow::Continue,
        ));
        assert!(r.transcript.turns.is_empty());
    }

    #[test]
    fn handle_line_routes_help_slash_to_continue() {
        let mut r = Repl::new(AppConfig::default(), None);
        let cf = r.handle_line("/help").unwrap();
        assert!(matches!(cf, ControlFlow::Continue));
    }

    #[test]
    fn handle_line_routes_quit_slash_to_quit_control_flow() {
        let mut r = Repl::new(AppConfig::default(), None);
        let cf = r.handle_line("/quit").unwrap();
        assert!(matches!(cf, ControlFlow::Quit));
    }

    #[test]
    fn run_with_reader_processes_slash_commands_and_quits() {
        use std::io::Cursor;
        let mut input = Cursor::new(b"/help\n/quit\n".to_vec());
        let mut output = Vec::new();
        let mut r = Repl::new(AppConfig::default(), None);
        r.run_with_reader(&mut input, &mut output).unwrap();
        let prompt = String::from_utf8(output).unwrap();
        assert!(prompt.contains("> "));
    }

    #[test]
    fn repl_turn_cancel_check_reports_request_and_reset() {
        let cancel = ReplTurnCancel::new_for_tests();
        let check = cancel.shared_check();
        assert!(!shared_cancel_requested(Some(&check)).unwrap());

        cancel.request();
        assert!(shared_cancel_requested(Some(&check)).unwrap());
        assert!(cancel.was_requested());

        cancel.reset();
        assert!(!shared_cancel_requested(Some(&check)).unwrap());
        assert!(!cancel.was_requested());
    }

    #[test]
    fn handle_line_with_cancel_does_not_record_cancelled_turn() {
        let mut config = AppConfig::default();
        config.model.api_key_env = "DEEPSEEK_TEST_MISSING_KEY_FOR_REPL_CANCEL".to_string();
        let mut repl = Repl::new(config, None);
        let cancel = ReplTurnCancel::new_for_tests();
        cancel.request();

        let error = repl
            .handle_line_with_cancel("write a test", Some(cancel.shared_check()))
            .unwrap_err();

        assert!(is_agent_run_cancelled_error(error.as_ref()));
        assert!(repl.transcript.turns.is_empty());
        assert!(repl.last_rollback_snapshot_id.is_none());
    }

    #[test]
    fn line_editor_browses_history_and_restores_draft() {
        let mut editor = ReplLineEditor::default();
        editor.remember_submitted_line("first task");
        editor.remember_submitted_line("second task");
        editor.start_line();
        for ch in "draft".chars() {
            assert_eq!(
                editor.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)),
                LineEditorOutcome::Redraw
            );
        }

        assert_eq!(
            editor.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            LineEditorOutcome::Redraw
        );
        assert_eq!(editor.buffer, "second task");
        assert_eq!(
            editor.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            LineEditorOutcome::Redraw
        );
        assert_eq!(editor.buffer, "first task");
        assert_eq!(
            editor.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            LineEditorOutcome::Redraw
        );
        assert_eq!(editor.buffer, "second task");
        assert_eq!(
            editor.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            LineEditorOutcome::Redraw
        );
        assert_eq!(editor.buffer, "draft");
        assert_eq!(editor.history_index, None);
    }

    #[test]
    fn line_editor_records_submitted_lines_without_blank_or_duplicate_entries() {
        let mut editor = ReplLineEditor::default();
        editor.remember_submitted_line("build");
        editor.remember_submitted_line("build");
        editor.remember_submitted_line("  ");
        editor.remember_submitted_line("test");

        assert_eq!(
            editor.history,
            vec!["build".to_string(), "test".to_string()]
        );
    }

    #[test]
    fn line_editor_tab_completes_slash_commands_and_load_sessions() {
        let mut editor = ReplLineEditor::default();
        editor.start_line();
        editor.buffer = "/he".to_string();
        editor.cursor = editor.buffer.len();
        assert!(matches!(
            editor.complete(&AppConfig::default()).unwrap(),
            ReplCompletionOutcome::Applied
        ));
        assert_eq!(editor.buffer, "/help");

        let (cfg, _tmp) = crate::repl::session::tests::config_with_temp_session_dir();
        let saved = Repl::new(cfg.clone(), None);
        crate::repl::session::save("alpha", &saved).unwrap();
        crate::repl::session::save("beta", &saved).unwrap();

        editor.start_line();
        editor.buffer = "/load al".to_string();
        editor.cursor = editor.buffer.len();
        assert!(matches!(
            editor.complete(&cfg).unwrap(),
            ReplCompletionOutcome::Applied
        ));
        assert_eq!(editor.buffer, "/load alpha");
    }

    #[test]
    fn line_editor_tab_lists_ambiguous_completion_candidates() {
        let mut editor = ReplLineEditor::default();
        editor.start_line();
        editor.buffer = "/s".to_string();
        editor.cursor = editor.buffer.len();

        match editor.complete(&AppConfig::default()).unwrap() {
            ReplCompletionOutcome::Suggestions(suggestions) => {
                assert!(suggestions.contains(&"/save".to_string()));
                assert!(suggestions.contains(&"/sessions".to_string()));
                assert_eq!(editor.buffer, "/s");
            }
            other => panic!("expected ambiguous suggestions, got {other:?}"),
        }
    }

    #[test]
    fn line_editor_submits_on_enter_variants() {
        let mut editor = ReplLineEditor::default();
        editor.start_line();
        for ch in "first".chars() {
            editor.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert_eq!(
            editor.handle_key(KeyEvent::new(KeyCode::Char('\n'), KeyModifiers::NONE)),
            LineEditorOutcome::Submit("first".to_string())
        );

        editor.start_line();
        for ch in "second".chars() {
            editor.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert_eq!(
            editor.handle_key(KeyEvent::new(KeyCode::Char('\r'), KeyModifiers::NONE)),
            LineEditorOutcome::Submit("second".to_string())
        );
        editor.start_line();
        for ch in "third".chars() {
            editor.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert_eq!(
            editor.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL)),
            LineEditorOutcome::Submit("third".to_string())
        );
        editor.start_line();
        for ch in "fourth".chars() {
            editor.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert_eq!(
            editor.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::CONTROL)),
            LineEditorOutcome::Submit("fourth".to_string())
        );
        assert_eq!(
            editor.history,
            vec![
                "first".to_string(),
                "second".to_string(),
                "third".to_string(),
                "fourth".to_string()
            ]
        );
    }

    #[test]
    fn ansi_escape_sequences_decode_common_line_editor_keys() {
        assert!(is_escape_key(&KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE
        )));
        assert!(is_escape_key(&KeyEvent::new(
            KeyCode::Char('\u{1b}'),
            KeyModifiers::NONE
        )));
        assert_eq!(key_code_from_ansi_sequence("[A"), Some(KeyCode::Up));
        assert_eq!(key_code_from_ansi_sequence("[B"), Some(KeyCode::Down));
        assert_eq!(key_code_from_ansi_sequence("[C"), Some(KeyCode::Right));
        assert_eq!(key_code_from_ansi_sequence("[D"), Some(KeyCode::Left));
        assert_eq!(key_code_from_ansi_sequence("[H"), Some(KeyCode::Home));
        assert_eq!(key_code_from_ansi_sequence("OH"), Some(KeyCode::Home));
        assert_eq!(key_code_from_ansi_sequence("[F"), Some(KeyCode::End));
        assert_eq!(key_code_from_ansi_sequence("OF"), Some(KeyCode::End));
        assert_eq!(key_code_from_ansi_sequence("[3~"), Some(KeyCode::Delete));
        assert_eq!(key_code_from_ansi_sequence("[Z"), None);
    }

    #[test]
    fn line_editor_supports_cursor_editing_and_utf8_boundaries() {
        let mut editor = ReplLineEditor::default();
        editor.start_line();
        for ch in "ab好".chars() {
            assert_eq!(
                editor.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)),
                LineEditorOutcome::Redraw
            );
        }
        assert_eq!(editor.buffer, "ab好");
        assert_eq!(
            editor.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
            LineEditorOutcome::Redraw
        );
        assert_eq!(
            editor.handle_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE)),
            LineEditorOutcome::Redraw
        );
        assert_eq!(editor.buffer, "abX好");
        assert_eq!(
            editor.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
            LineEditorOutcome::Redraw
        );
        assert_eq!(editor.buffer, "ab好");
        assert_eq!(
            editor.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE)),
            LineEditorOutcome::Redraw
        );
        assert_eq!(editor.buffer, "ab");
        assert!(editor.buffer.is_char_boundary(editor.cursor));
    }

    #[test]
    fn line_editor_control_keys_clear_and_delete_words() {
        let mut editor = ReplLineEditor::default();
        editor.start_line();
        for ch in "run cargo test".chars() {
            editor.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(
            editor.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
            LineEditorOutcome::Redraw
        );
        assert_eq!(editor.buffer, "run cargo ");
        assert_eq!(
            editor.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)),
            LineEditorOutcome::Redraw
        );
        assert!(editor.buffer.is_empty());
        assert_eq!(
            editor.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            LineEditorOutcome::Eof
        );
    }

    #[test]
    fn invoked_binary_name_falls_back_to_deepseek_when_missing() {
        let name = invoked_binary_name();
        assert!(!name.trim().is_empty());
    }

    #[test]
    fn repl_turn_snapshot_label_compacts_prompt() {
        let label = repl_turn_snapshot_label("  edit   the file\n\nand run tests  ");
        assert_eq!(label, "REPL turn before: edit the file and run tests");
        let long = repl_turn_snapshot_label(&"x ".repeat(200));
        assert!(long.len() <= "REPL turn before: ".len() + 80);
    }

    #[test]
    fn create_turn_snapshot_captures_repl_worktree_state() {
        let repo = temp_root("turn-snapshot");
        fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init"]);
        fs::write(repo.join("src.txt"), "base\n").unwrap();
        run_git(&repo, &["add", "src.txt"]);
        run_git(
            &repo,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "commit",
                "-m",
                "initial",
            ],
        );
        fs::write(repo.join("src.txt"), "changed before repl turn\n").unwrap();

        let _cwd = crate::util::cwd::CwdGuard::enter(&repo).unwrap();
        let mut config = AppConfig::default();
        config.workspace.config_dir = repo.join(".dscode").display().to_string();
        let repl = Repl::new(config, None);
        let snapshot_id = repl
            .create_turn_snapshot("edit src and run tests")
            .expect("REPL turn snapshot");

        let store = crate::core::rollback::RollbackStore::new(repo.join(".dscode/rollback"));
        let snapshot = store.load_snapshot(&snapshot_id).unwrap();
        assert_eq!(snapshot.label, "REPL turn before: edit src and run tests");
        assert!(snapshot.patch_bytes > 0);
        assert_eq!(snapshot.runtime_thread_id, None);
        assert_eq!(snapshot.runtime_turn_id, None);
    }
}
