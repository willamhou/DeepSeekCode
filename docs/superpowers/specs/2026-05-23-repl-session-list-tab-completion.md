# REPL Session List And Tab Completion

## Context

The original REPL design still tracked `/sessions` listing and `/load`
completion as Phase 9b candidates. After raw-mode line editing landed, this was
the next small but visible gap versus mature coding-agent CLIs: users could save
and load sessions, but had to remember exact names.

## Scope

- Add a safe session-name listing helper over `.dscode/sessions/*.json`.
- Add `/sessions [prefix]` to list saved session names without mutating REPL
  state.
- Add Tab completion in the raw-mode line editor for built-in slash commands and
  saved session names after `/load `.
- Keep the non-interactive buffered reader path unchanged.

## Implementation

- `repl::session::list_names` scans the configured session directory, accepts
  only regular `.json` files with safe session names, sorts names, and dedups.
- `/sessions [prefix]` prints all saved session names, or only names matching a
  prefix.
- The raw-mode editor handles Tab before normal character editing:
  - unique matches replace the current command/session-name prefix;
  - multiple exact-prefix matches extend to the longest common prefix when
    possible;
  - ambiguous matches are printed above the prompt without submitting the line.

## Verification

- `cargo test repl::session::tests::list_names_returns_sorted_valid_json_sessions_only --lib -- --test-threads=1`
- `cargo test complete_repl_input --lib -- --test-threads=1`
- `cargo test line_editor_tab --lib -- --test-threads=1`
- `cargo test sessions_slash --lib -- --test-threads=1`

## Remaining

This does not add persistent shell-style history across separate `deepseek run`
and `deepseek chat` invocations. That remains a separate cross-command history
design.
