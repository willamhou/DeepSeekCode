use crate::config::types::AppConfig;
use crate::error::AppResult;
use crate::repl::transcript::Transcript;

pub const DEFAULT_BUDGET: usize = 20;

pub enum ControlFlow {
    Continue,
    Quit,
}

pub struct Repl {
    pub config: AppConfig,
    pub transcript: Transcript,
    pub budget: usize,
    pub skill: Option<String>,
    pub tokens_prompt: u64,
    pub tokens_completion: u64,
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
        }
    }

    pub fn run(&mut self) -> AppResult<()> {
        use std::io::{self, IsTerminal};
        if !io::stdin().is_terminal() {
            return Err(crate::error::policy_denied(
                "dscode chat requires a TTY; use `dscode run \"task\"` for one-shot tasks",
            ));
        }
        let stdin = io::stdin();
        let mut handle = stdin.lock();
        self.run_with_reader(&mut handle, &mut io::stderr())
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
        if line.trim().is_empty() {
            return Ok(ControlFlow::Continue);
        }
        // Slash dispatch lands in Task 4. LLM dispatch lands in Task 5.
        // For now: record the user turn and print a stub acknowledgement.
        self.transcript.push_user(line);
        println!("(received {} chars; LLM dispatch not yet wired)", line.len());
        Ok(ControlFlow::Continue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::AppConfig;

    #[test]
    fn new_starts_with_default_budget_and_empty_transcript() {
        let r = Repl::new(AppConfig::default(), None);
        assert_eq!(r.budget, DEFAULT_BUDGET);
        assert!(r.transcript.turns.is_empty());
        assert_eq!(r.tokens_prompt, 0);
        assert_eq!(r.tokens_completion, 0);
        assert!(r.skill.is_none());
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
    fn handle_line_records_user_turn_for_plain_text() {
        let mut r = Repl::new(AppConfig::default(), None);
        r.handle_line("hello world").unwrap();
        assert_eq!(r.transcript.turns.len(), 1);
        assert_eq!(r.transcript.turns[0].content, "hello world");
    }

    #[test]
    fn run_with_reader_processes_two_lines_and_quits_on_eof() {
        use std::io::Cursor;
        let mut input = Cursor::new(b"hello\nbye\n".to_vec());
        let mut output = Vec::new();
        let mut r = Repl::new(AppConfig::default(), None);
        r.run_with_reader(&mut input, &mut output).unwrap();
        assert_eq!(r.transcript.turns.len(), 2);
        assert_eq!(r.transcript.turns[0].content, "hello");
        assert_eq!(r.transcript.turns[1].content, "bye");
        let prompt = String::from_utf8(output).unwrap();
        assert!(prompt.contains("> "));
    }
}
