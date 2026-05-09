use crate::error::AppResult;
use crate::repl::repl::{Repl, DEFAULT_BUDGET};
use crate::skills::registry::SkillRegistry;
use std::path::{Path, PathBuf};

pub enum SlashOutcome {
    NotASlash,
    Submit(String),
    Continue,
    Quit,
}

pub fn try_handle_slash(repl: &mut Repl, line: &str) -> AppResult<SlashOutcome> {
    if !line.starts_with('/') {
        return Ok(SlashOutcome::NotASlash);
    }
    let mut tokens = line.split_whitespace();
    let command = tokens.next().unwrap_or("");
    let args: Vec<&str> = tokens.collect();

    match command {
        "/quit" | "/q" | "/exit" => Ok(SlashOutcome::Quit),
        "/help" | "/h" | "/?" => {
            print_help();
            Ok(SlashOutcome::Continue)
        }
        "/clear" => {
            repl.transcript.clear();
            repl.tokens_prompt = 0;
            repl.tokens_completion = 0;
            repl.todos.borrow_mut().items.clear();
            println!(
                "cleared transcript (kept budget={}, skill={})",
                repl.budget,
                repl.skill.as_deref().unwrap_or("-")
            );
            Ok(SlashOutcome::Continue)
        }
        "/budget" => {
            handle_budget(repl, &args);
            Ok(SlashOutcome::Continue)
        }
        "/skill" => {
            handle_skill(repl, &args);
            Ok(SlashOutcome::Continue)
        }
        "/diff" => {
            handle_diff();
            Ok(SlashOutcome::Continue)
        }
        "/cost" => {
            handle_cost(repl);
            Ok(SlashOutcome::Continue)
        }
        "/save" => {
            handle_save(repl, &args);
            Ok(SlashOutcome::Continue)
        }
        "/load" => {
            handle_load(repl, &args);
            Ok(SlashOutcome::Continue)
        }
        "/todos" => {
            let inner = repl.todos.borrow();
            if inner.is_empty() {
                eprintln!("no todos yet");
            } else {
                for line in inner.render_for_display().lines() {
                    eprintln!("{line}");
                }
            }
            Ok(SlashOutcome::Continue)
        }
        other => {
            if let Some(prompt) = load_custom_slash_command(repl, other, &args)? {
                return Ok(SlashOutcome::Submit(prompt));
            }
            println!("unknown slash command `{other}`; type /help for the list");
            Ok(SlashOutcome::Continue)
        }
    }
}

fn print_help() {
    println!("slash commands:");
    println!("  /quit, /q, /exit              exit the REPL");
    println!("  /help, /h, /?                 show this help");
    println!("  /clear                        wipe transcript + token counters");
    println!("  /budget [N]                   show or set per-turn step budget (1..200)");
    println!("  /skill [name|-]               show, switch, or clear the active skill");
    println!("  /diff                         show pending git diff");
    println!("  /save <name>                  save the session to .dscode/sessions/<name>.json");
    println!("  /load <name>                  restore a saved session");
    println!("  /todos                        show the current todo list (read-only)");
    println!("  /cost                         show prompt/completion token totals");
    println!("custom commands:");
    println!("  /name [args]                  run .dscode/commands/name.md or a user command");
}

fn handle_budget(repl: &mut Repl, args: &[&str]) {
    if args.is_empty() {
        println!("budget: {} (default {DEFAULT_BUDGET})", repl.budget);
        return;
    }
    if args.len() > 1 {
        println!("usage: /budget [N]");
        return;
    }
    match args[0].parse::<usize>() {
        Ok(value) if (1..=200).contains(&value) => {
            let prev = repl.budget;
            repl.budget = value;
            println!("budget set to {value} (was {prev})");
        }
        Ok(_) => println!("budget out of range; expected 1..=200"),
        Err(_) => println!("budget must be a positive integer; got `{}`", args[0]),
    }
}

fn handle_skill(repl: &mut Repl, args: &[&str]) {
    if args.is_empty() {
        println!("skill: {}", repl.skill.as_deref().unwrap_or("-"));
        return;
    }
    if args.len() > 1 {
        println!("usage: /skill [name|-]");
        return;
    }
    let target = args[0];
    if target == "-" {
        repl.skill = None;
        println!("skill cleared");
        return;
    }
    let registry = match SkillRegistry::load_dir("skills") {
        Ok(reg) => reg,
        Err(error) => {
            println!("could not load skills: {error}");
            return;
        }
    };
    if registry.find(target).is_some() {
        repl.skill = Some(target.to_string());
        println!("skill switched to {target}");
    } else {
        let names: Vec<&str> = registry.iter().map(|s| s.name.as_str()).collect();
        println!(
            "skill `{target}` not found; known: {}",
            if names.is_empty() {
                "(none)".to_string()
            } else {
                names.join(", ")
            }
        );
    }
}

fn handle_diff() {
    match crate::util::process::run_capture("git", &["diff"]) {
        Ok(captured) => {
            if !captured.success {
                println!("git diff failed: {}", captured.stderr.trim());
                return;
            }
            let body = captured.stdout;
            if body.trim().is_empty() {
                println!("no pending changes");
            } else {
                println!("{body}");
            }
        }
        Err(error) => println!("could not run git diff: {error}"),
    }
}

fn handle_cost(repl: &Repl) {
    if repl.tokens_prompt == 0 && repl.tokens_completion == 0 {
        println!("no remote calls yet");
        return;
    }
    let total = repl.tokens_prompt + repl.tokens_completion;
    println!(
        "prompt: {}, completion: {}, total: {}",
        repl.tokens_prompt, repl.tokens_completion, total
    );
}

fn handle_save(repl: &mut Repl, args: &[&str]) {
    let name = match args {
        [name] => *name,
        _ => {
            println!("usage: /save <name>");
            return;
        }
    };
    match crate::repl::session::save(name, repl) {
        Ok(path) => println!("saved -> {}", path.display()),
        Err(error) => println!("save failed: {error}"),
    }
}

fn handle_load(repl: &mut Repl, args: &[&str]) {
    let name = match args {
        [name] => *name,
        _ => {
            println!("usage: /load <name>");
            return;
        }
    };
    match crate::repl::session::load(name, &repl.config) {
        Ok(loaded) => {
            *repl = loaded;
            println!(
                "loaded {name} (transcript: {} turns, tokens: {} / {})",
                repl.transcript.turns.len(),
                repl.tokens_prompt,
                repl.tokens_completion,
            );
        }
        Err(error) => println!("load failed: {error}"),
    }
}

fn load_custom_slash_command(
    repl: &Repl,
    command: &str,
    args: &[&str],
) -> AppResult<Option<String>> {
    let Some(relative_path) = custom_command_relative_path(command) else {
        return Ok(None);
    };
    let candidates = [
        repl.config
            .workspace
            .user_commands_dir()
            .join(&relative_path),
        repl.config.workspace.commands_dir().join(&relative_path),
    ];
    let Some(path) = candidates.iter().find(|path| path.is_file()) else {
        return Ok(None);
    };
    let content = std::fs::read_to_string(path)?;
    let command_name = command.trim_start_matches('/');
    let args_raw = args.join(" ");
    let expanded = expand_command_arguments(strip_frontmatter(&content), &args_raw);
    Ok(Some(render_custom_command_prompt(
        command_name,
        path,
        &expanded,
    )))
}

fn custom_command_relative_path(command: &str) -> Option<PathBuf> {
    let name = command.strip_prefix('/')?;
    if name.is_empty() || name.starts_with('.') || name.contains("..") {
        return None;
    }
    let mut path = PathBuf::new();
    for segment in name.split('/') {
        if segment.is_empty()
            || segment.starts_with('.')
            || !segment
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        {
            return None;
        }
        path.push(segment);
    }
    path.set_extension("md");
    Some(path)
}

fn strip_frontmatter(content: &str) -> &str {
    let Some(rest) = content.strip_prefix("---") else {
        return content;
    };
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    if let Some((_, body)) = rest.split_once("\n---\n") {
        body
    } else {
        content
    }
}

fn expand_command_arguments(content: &str, args_raw: &str) -> String {
    let args = split_command_arguments(args_raw);
    let mut expanded = expand_argument_placeholders(content, args_raw, &args);
    if !args_raw.is_empty() && !contains_argument_placeholder(content) {
        if !expanded.ends_with('\n') {
            expanded.push('\n');
        }
        expanded.push_str(&format!("\nARGUMENTS: {args_raw}\n"));
    }
    expanded
}

fn expand_argument_placeholders(content: &str, args_raw: &str, args: &[String]) -> String {
    let mut expanded = String::with_capacity(content.len() + args_raw.len());
    let mut offset = 0;
    while offset < content.len() {
        let remaining = &content[offset..];
        if let Some((token_len, replacement)) = expand_indexed_argument_placeholder(remaining, args)
        {
            expanded.push_str(replacement);
            offset += token_len;
            continue;
        }
        if remaining.starts_with("$ARGUMENTS")
            && remaining.as_bytes().get("$ARGUMENTS".len()) != Some(&b'[')
        {
            expanded.push_str(args_raw);
            offset += "$ARGUMENTS".len();
            continue;
        }
        if let Some((token_len, replacement)) = expand_positional_placeholder(remaining, args) {
            expanded.push_str(replacement);
            offset += token_len;
            continue;
        }
        let ch = remaining.chars().next().expect("offset is in bounds");
        expanded.push(ch);
        offset += ch.len_utf8();
    }
    expanded
}

fn expand_indexed_argument_placeholder<'a>(
    content: &'a str,
    args: &'a [String],
) -> Option<(usize, &'a str)> {
    let after_prefix = content.strip_prefix("$ARGUMENTS[")?;
    let digit_len = leading_digit_len(after_prefix);
    if digit_len == 0 || after_prefix.as_bytes().get(digit_len) != Some(&b']') {
        return None;
    }
    let token_len = "$ARGUMENTS[".len() + digit_len + 1;
    let index = after_prefix[..digit_len].parse::<usize>().ok()?;
    Some((
        token_len,
        args.get(index)
            .map_or(&content[..token_len], String::as_str),
    ))
}

fn expand_positional_placeholder<'a>(
    content: &'a str,
    args: &'a [String],
) -> Option<(usize, &'a str)> {
    let after_prefix = content.strip_prefix('$')?;
    let digit_len = leading_digit_len(after_prefix);
    if digit_len == 0 {
        return None;
    }
    let token_len = 1 + digit_len;
    let index = after_prefix[..digit_len].parse::<usize>().ok()?;
    Some((
        token_len,
        args.get(index)
            .map_or(&content[..token_len], String::as_str),
    ))
}

fn leading_digit_len(value: &str) -> usize {
    value.bytes().take_while(u8::is_ascii_digit).count()
}

fn contains_argument_placeholder(content: &str) -> bool {
    content.contains("$ARGUMENTS")
        || content
            .as_bytes()
            .windows(2)
            .any(|window| window[0] == b'$' && window[1].is_ascii_digit())
}

fn split_command_arguments(args_raw: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for ch in args_raw.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            ch if ch.is_whitespace() => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if escaped {
        current.push('\\');
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

fn render_custom_command_prompt(command_name: &str, path: &Path, body: &str) -> String {
    format!(
        "Custom slash command /{command_name}\nSource: {}\n\n{}",
        path.display(),
        body.trim()
    )
}

pub fn validate_session_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("session name cannot be empty".into());
    }
    if name.starts_with('.') {
        return Err("session name cannot start with `.`".into());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("session name cannot contain path separators".into());
    }
    if name.contains("..") {
        return Err("session name cannot contain `..`".into());
    }
    if name.chars().any(|c| c.is_control()) {
        return Err("session name cannot contain control characters".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::AppConfig;

    fn fresh_repl() -> Repl {
        Repl::new(AppConfig::default(), None)
    }

    fn repl_with_command_dirs() -> (Repl, PathBuf) {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "deepseek-slash-commands-{}-{suffix}",
            std::process::id()
        ));
        let mut config = AppConfig::default();
        config.workspace.config_dir = root.join(".dscode").display().to_string();
        config.workspace.session_dir = root.join(".dscode/sessions").display().to_string();
        config.workspace.user_commands_dir = root.join("user-commands").display().to_string();
        (Repl::new(config, None), root)
    }

    #[test]
    fn returns_not_a_slash_for_plain_text() {
        let mut r = fresh_repl();
        let out = try_handle_slash(&mut r, "hello").unwrap();
        assert!(matches!(out, SlashOutcome::NotASlash));
    }

    #[test]
    fn quit_returns_quit_outcome() {
        let mut r = fresh_repl();
        assert!(matches!(
            try_handle_slash(&mut r, "/quit").unwrap(),
            SlashOutcome::Quit,
        ));
    }

    #[test]
    fn quit_aliases_q_and_exit_also_return_quit() {
        for alias in ["/q", "/exit"] {
            let mut r = fresh_repl();
            assert!(
                matches!(try_handle_slash(&mut r, alias).unwrap(), SlashOutcome::Quit),
                "alias `{alias}` should map to Quit",
            );
        }
    }

    #[test]
    fn help_prints_and_continues() {
        let mut r = fresh_repl();
        assert!(matches!(
            try_handle_slash(&mut r, "/help").unwrap(),
            SlashOutcome::Continue,
        ));
    }

    #[test]
    fn clear_wipes_transcript_and_keeps_budget_skill() {
        let mut r = fresh_repl();
        r.transcript.push_user("a");
        r.transcript.push_assistant("b");
        r.tokens_prompt = 100;
        r.budget = 30;
        r.skill = Some("x".to_string());
        try_handle_slash(&mut r, "/clear").unwrap();
        assert!(r.transcript.turns.is_empty());
        assert_eq!(r.tokens_prompt, 0);
        assert_eq!(r.budget, 30);
        assert_eq!(r.skill.as_deref(), Some("x"));
    }

    #[test]
    fn budget_with_valid_number_updates_budget() {
        let mut r = fresh_repl();
        try_handle_slash(&mut r, "/budget 30").unwrap();
        assert_eq!(r.budget, 30);
    }

    #[test]
    fn budget_with_zero_does_not_update() {
        let mut r = fresh_repl();
        let before = r.budget;
        try_handle_slash(&mut r, "/budget 0").unwrap();
        assert_eq!(r.budget, before);
    }

    #[test]
    fn budget_with_too_large_does_not_update() {
        let mut r = fresh_repl();
        let before = r.budget;
        try_handle_slash(&mut r, "/budget 999").unwrap();
        assert_eq!(r.budget, before);
    }

    #[test]
    fn skill_dash_clears_active_skill() {
        let mut r = fresh_repl();
        r.skill = Some("x".to_string());
        try_handle_slash(&mut r, "/skill -").unwrap();
        assert!(r.skill.is_none());
    }

    #[test]
    fn unknown_slash_is_handled_gracefully() {
        let mut r = fresh_repl();
        assert!(matches!(
            try_handle_slash(&mut r, "/bogus").unwrap(),
            SlashOutcome::Continue,
        ));
    }

    #[test]
    fn custom_slash_command_loads_project_markdown() {
        let (mut r, root) = repl_with_command_dirs();
        let command_dir = root.join(".dscode/commands");
        std::fs::create_dir_all(&command_dir).unwrap();
        std::fs::write(
            command_dir.join("review.md"),
            "---\ndescription: Review a path\n---\nReview $0 with mode $1.\n",
        )
        .unwrap();

        let outcome = try_handle_slash(&mut r, "/review \"src lib\" strict").unwrap();

        match outcome {
            SlashOutcome::Submit(prompt) => {
                assert!(prompt.contains("Custom slash command /review"));
                assert!(prompt.contains("Review src lib with mode strict."));
                assert!(!prompt.contains("description: Review a path"));
            }
            _ => panic!("expected custom slash command submission"),
        }
    }

    #[test]
    fn custom_slash_command_appends_arguments_without_placeholder() {
        let (mut r, root) = repl_with_command_dirs();
        let command_dir = root.join(".dscode/commands");
        std::fs::create_dir_all(&command_dir).unwrap();
        std::fs::write(command_dir.join("deploy.md"), "Deploy using the runbook.\n").unwrap();

        let outcome = try_handle_slash(&mut r, "/deploy staging canary").unwrap();

        match outcome {
            SlashOutcome::Submit(prompt) => {
                assert!(prompt.contains("Deploy using the runbook."));
                assert!(prompt.contains("ARGUMENTS: staging canary"));
            }
            _ => panic!("expected custom slash command submission"),
        }
    }

    #[test]
    fn custom_slash_command_supports_namespaces_and_user_override() {
        let (mut r, root) = repl_with_command_dirs();
        let project_dir = root.join(".dscode/commands/pr");
        let user_dir = root.join("user-commands/pr");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::create_dir_all(&user_dir).unwrap();
        std::fs::write(project_dir.join("fix.md"), "project $ARGUMENTS").unwrap();
        std::fs::write(user_dir.join("fix.md"), "user $ARGUMENTS").unwrap();

        let outcome = try_handle_slash(&mut r, "/pr/fix 42").unwrap();

        match outcome {
            SlashOutcome::Submit(prompt) => {
                assert!(prompt.contains("Custom slash command /pr/fix"));
                assert!(prompt.contains("user 42"));
                assert!(!prompt.contains("project 42"));
            }
            _ => panic!("expected custom slash command submission"),
        }
    }

    #[test]
    fn custom_slash_command_expands_indexed_argument_tokens_safely() {
        let args = (0..=10)
            .map(|index| format!("arg{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        let expanded = expand_command_arguments("$0 $10 $ARGUMENTS[10] $99 $ARGUMENTS[99]", &args);

        assert_eq!(expanded, "arg0 arg10 arg10 $99 $ARGUMENTS[99]");
        assert_eq!(
            expand_command_arguments("$ARGUMENTS[nope]", "value"),
            "$ARGUMENTS[nope]",
        );
    }

    #[test]
    fn custom_command_relative_path_rejects_unsafe_names() {
        assert_eq!(
            custom_command_relative_path("/review").unwrap(),
            PathBuf::from("review.md")
        );
        assert_eq!(
            custom_command_relative_path("/pr/fix").unwrap(),
            PathBuf::from("pr/fix.md")
        );
        assert!(custom_command_relative_path("/../x").is_none());
        assert!(custom_command_relative_path("/.hidden").is_none());
        assert!(custom_command_relative_path("/bad$name").is_none());
    }

    #[test]
    fn validate_session_name_rejects_dotdot() {
        assert!(validate_session_name("foo..bar").is_err());
    }

    #[test]
    fn validate_session_name_rejects_path_separators() {
        assert!(validate_session_name("a/b").is_err());
        assert!(validate_session_name("a\\b").is_err());
    }

    #[test]
    fn validate_session_name_rejects_leading_dot() {
        assert!(validate_session_name(".hidden").is_err());
    }

    #[test]
    fn validate_session_name_rejects_empty() {
        assert!(validate_session_name("").is_err());
    }

    #[test]
    fn validate_session_name_accepts_normal_name() {
        assert!(validate_session_name("fix-pr-42").is_ok());
        assert!(validate_session_name("session_2026").is_ok());
    }

    #[test]
    fn cost_with_no_calls_returns_continue() {
        let mut r = fresh_repl();
        assert!(matches!(
            try_handle_slash(&mut r, "/cost").unwrap(),
            SlashOutcome::Continue,
        ));
    }

    #[test]
    fn cost_with_accumulated_tokens_returns_continue() {
        let mut r = fresh_repl();
        r.tokens_prompt = 100;
        r.tokens_completion = 50;
        assert!(matches!(
            try_handle_slash(&mut r, "/cost").unwrap(),
            SlashOutcome::Continue,
        ));
    }

    #[test]
    fn diff_returns_continue() {
        let mut r = fresh_repl();
        assert!(matches!(
            try_handle_slash(&mut r, "/diff").unwrap(),
            SlashOutcome::Continue,
        ));
    }

    #[test]
    fn slash_clear_resets_todos_along_with_transcript_and_tokens() {
        use crate::core::todos::{Todo, TodoStatus};
        let mut r = Repl::new(AppConfig::default(), None);
        r.transcript.push_user("hi");
        r.tokens_prompt = 100;
        r.todos.borrow_mut().replace(vec![Todo {
            content: "X".to_string(),
            active_form: "Xing".to_string(),
            status: TodoStatus::Pending,
        }]);
        let _ = r.handle_line("/clear").unwrap();
        assert!(r.transcript.turns.is_empty());
        assert_eq!(r.tokens_prompt, 0);
        assert!(r.todos.borrow().is_empty());
    }

    #[test]
    fn slash_todos_returns_continue_when_empty() {
        let mut r = Repl::new(AppConfig::default(), None);
        let outcome = try_handle_slash(&mut r, "/todos").unwrap();
        assert!(matches!(outcome, SlashOutcome::Continue));
        assert!(r.todos.borrow().is_empty());
    }

    #[test]
    fn slash_todos_does_not_mutate_list() {
        use crate::core::todos::{Todo, TodoStatus};
        let mut r = Repl::new(AppConfig::default(), None);
        r.todos.borrow_mut().replace(vec![
            Todo {
                content: "X".to_string(),
                active_form: "Xing".to_string(),
                status: TodoStatus::InProgress,
            },
            Todo {
                content: "Y".to_string(),
                active_form: "Ying".to_string(),
                status: TodoStatus::Pending,
            },
        ]);
        let before_len = r.todos.borrow().items.len();
        let outcome = try_handle_slash(&mut r, "/todos").unwrap();
        assert!(matches!(outcome, SlashOutcome::Continue));
        assert_eq!(
            r.todos.borrow().items.len(),
            before_len,
            "/todos must be read-only"
        );
    }

    #[test]
    fn save_load_round_trip_preserves_todos() {
        use crate::core::todos::{Todo, TodoStatus};
        let (cfg, _tmp) = crate::repl::session::tests::config_with_temp_session_dir();
        let original = Repl::new(cfg.clone(), None);
        original.todos.borrow_mut().replace(vec![Todo {
            content: "T".to_string(),
            active_form: "Ting".to_string(),
            status: TodoStatus::Completed,
        }]);
        crate::repl::session::save("rt", &original).unwrap();
        let loaded = crate::repl::session::load("rt", &cfg).unwrap();
        let inner = loaded.todos.borrow();
        assert_eq!(inner.items.len(), 1);
        assert_eq!(inner.items[0].content, "T");
        assert_eq!(inner.items[0].status, TodoStatus::Completed);
    }
}
