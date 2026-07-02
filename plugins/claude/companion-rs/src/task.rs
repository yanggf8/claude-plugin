//! `parse_task_args`, `build_task_args`, `render_task`

use serde::Deserialize;
use std::env;
use std::fmt;

/// Parsed routing options for the `task` subcommand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskOptions {
    pub background: bool,
    pub read: bool,
    pub resume: bool,
    pub resume_id: Option<String>,
    pub effort: Option<String>,
    pub model: Option<String>,
    pub cwd: Option<String>,
    pub add_dirs: Vec<String>,
    /// `None` = flag not present; `Some(None)` = bare `--worktree`; `Some(Some(name))` = named worktree.
    pub worktree: Option<Option<String>>,
    pub prompt: String,
}

/// Error returned when task argv parsing fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskArgError {
    message: String,
}

impl TaskArgError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn exit_code(&self) -> i32 {
        1
    }
}

impl fmt::Display for TaskArgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TaskArgError {}

fn delegate_model_from_env() -> Option<String> {
    env::var("CLAUDE_DELEGATE_MODEL")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

pub fn parse_task_args(argv: &[String]) -> Result<TaskOptions, TaskArgError> {
    let mut opts = TaskOptions {
        background: false,
        read: false,
        resume: false,
        resume_id: None,
        effort: None,
        model: delegate_model_from_env(),
        cwd: None,
        add_dirs: Vec::new(),
        worktree: None,
        prompt: String::new(),
    };

    let mut prompt_parts: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < argv.len() {
        let a = argv[i].as_str();

        if a == "--worktree" || a == "-w" {
            if let Some(next) = argv.get(i + 1) {
                if !next.starts_with('-') {
                    opts.worktree = Some(Some(next.clone()));
                    i += 2;
                    continue;
                }
            }
            opts.worktree = Some(None);
            i += 1;
        } else if a == "--resume" {
            opts.resume = true;
            if let Some(next) = argv.get(i + 1) {
                if !next.starts_with('-') {
                    opts.resume_id = Some(next.clone());
                    i += 2;
                    continue;
                }
            }
            i += 1;
        } else if matches!(a, "--effort" | "--model" | "--cwd" | "--add-dir") {
            i += 1;
            let next = argv.get(i);
            match (a, next) {
                ("--effort", Some(value)) => opts.effort = Some(value.clone()),
                ("--model", Some(value)) => opts.model = Some(value.clone()),
                ("--cwd", Some(value)) => opts.cwd = Some(value.clone()),
                ("--add-dir", Some(value)) => opts.add_dirs.push(value.clone()),
                (flag, None) => {
                    return Err(TaskArgError::new(format!("missing value for {flag}")));
                }
                _ => unreachable!("matched only known value flags"),
            }
            i += 1;
        } else if a == "--background" {
            opts.background = true;
            i += 1;
        } else if a == "--wait" {
            i += 1;
        } else if a == "--read" {
            opts.read = true;
            i += 1;
        } else {
            prompt_parts.push(&argv[i]);
            i += 1;
        }
    }

    opts.prompt = prompt_parts.join(" ").trim().to_string();
    if opts.prompt.is_empty() {
        return Err(TaskArgError::new(
            "No task text provided to claude-companion task.",
        ));
    }

    Ok(opts)
}

pub fn build_task_args(opts: &TaskOptions) -> Vec<String> {
    let mut args = vec![
        "-p".to_string(),
        opts.prompt.clone(),
        "--output-format".to_string(),
        "json".to_string(),
    ];

    if let Some(model) = &opts.model {
        args.push("--model".to_string());
        args.push(model.clone());
    }

    if opts.read {
        args.push("--permission-mode".to_string());
        args.push("plan".to_string());
    } else {
        args.push("--permission-mode".to_string());
        args.push("acceptEdits".to_string());
    }

    if let Some(effort) = &opts.effort {
        args.push("--effort".to_string());
        args.push(effort.clone());
    }

    if opts.resume {
        if let Some(resume_id) = &opts.resume_id {
            args.push("-r".to_string());
            args.push(resume_id.clone());
        } else {
            args.push("-c".to_string());
        }
    }

    if let Some(worktree) = &opts.worktree {
        args.push("--worktree".to_string());
        if let Some(name) = worktree {
            args.push(name.clone());
        }
    }

    for dir in &opts.add_dirs {
        args.push("--add-dir".to_string());
        args.push(dir.clone());
    }

    args
}

#[derive(Debug, Deserialize)]
struct TaskRenderJson {
    result: Option<String>,
    session_id: Option<String>,
    stop_reason: Option<String>,
    total_cost_usd: Option<serde_json::Value>,
}

fn format_cost(value: &serde_json::Value) -> String {
    let amount = match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.parse::<f64>().ok(),
        _ => None,
    };
    format!("${:.4}", amount.unwrap_or(f64::NAN))
}

pub fn render_task(raw: &str, opts: &TaskOptions) -> String {
    let parsed: Option<TaskRenderJson> = serde_json::from_str(raw).ok();
    let Some(parsed) = parsed else {
        return format!("{}\n", raw.trim_end());
    };

    let mut lines = Vec::new();
    lines.push("=== claude delegate result ===".to_string());
    lines.push(format!(
        "mode:    {}",
        if opts.read {
            "read-only (plan)"
        } else {
            "write-capable"
        }
    ));
    lines.push(format!(
        "model:   {}",
        opts.model
            .as_deref()
            .unwrap_or("(inherits Claude Code default)")
    ));

    if let Some(effort) = &opts.effort {
        lines.push(format!("effort:  {effort}"));
    }
    if let Some(stop) = &parsed.stop_reason {
        lines.push(format!("stop:    {stop}"));
    }
    if let Some(session) = &parsed.session_id {
        lines.push(format!("session: {session}"));
    }
    if let Some(cost) = &parsed.total_cost_usd {
        lines.push(format!("cost:    {}", format_cost(cost)));
    }

    lines.push(String::new());
    lines.push(
        parsed
            .result
            .unwrap_or_else(|| "(no result text returned)".to_string()),
    );

    if parsed.session_id.is_some() {
        let cwd = opts
            .cwd
            .as_deref()
            .map(str::to_string)
            .or_else(|| {
                env::current_dir()
                    .ok()
                    .map(|path| path.display().to_string())
            })
            .unwrap_or_else(|| ".".to_string());
        lines.push(String::new());
        lines.push(format!("Continue this thread: claude -c   (in {cwd})"));
    }

    format!("{}\n", lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, ffi::OsString, sync::Mutex};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct DelegateModelGuard {
        previous: Option<OsString>,
    }

    impl DelegateModelGuard {
        fn set(value: Option<&str>) -> Self {
            let previous = env::var_os("CLAUDE_DELEGATE_MODEL");
            unsafe {
                match value {
                    Some(value) => env::set_var("CLAUDE_DELEGATE_MODEL", value),
                    None => env::remove_var("CLAUDE_DELEGATE_MODEL"),
                }
            }
            Self { previous }
        }
    }

    impl Drop for DelegateModelGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(value) => env::set_var("CLAUDE_DELEGATE_MODEL", value),
                    None => env::remove_var("CLAUDE_DELEGATE_MODEL"),
                }
            }
        }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    fn parse(argv: &[&str]) -> TaskOptions {
        parse_with_delegate_model(argv, None)
    }

    fn parse_with_delegate_model(argv: &[&str], delegate_model: Option<&str>) -> TaskOptions {
        let _lock = ENV_LOCK
            .lock()
            .expect("delegate model env lock should not be poisoned");
        let _guard = DelegateModelGuard::set(delegate_model);
        let args = strings(argv);
        parse_task_args(&args).expect("task argv should parse")
    }

    fn assert_task_arg_error(argv: &[&str], expected_message: &str) {
        let _lock = ENV_LOCK
            .lock()
            .expect("delegate model env lock should not be poisoned");
        let _guard = DelegateModelGuard::set(None);
        let args = strings(argv);
        let err = parse_task_args(&args).expect_err("task argv should fail");

        assert_eq!(err.exit_code(), 1);
        assert!(
            err.to_string().contains(expected_message),
            "expected error message to contain {expected_message:?}, got {err}"
        );
    }

    #[test]
    fn task_without_prompt_exits_1() {
        assert_task_arg_error(&[], "No task text provided to claude-companion task.");
    }

    #[test]
    fn build_task_args_default_write_mode_accept_edits_and_no_model() {
        let opts = parse(&["hello", "world"]);

        assert_eq!(
            build_task_args(&opts),
            strings(&[
                "-p",
                "hello world",
                "--output-format",
                "json",
                "--permission-mode",
                "acceptEdits",
            ])
        );
    }

    #[test]
    fn build_task_args_read_mode_maps_to_permission_mode_plan() {
        let opts = parse(&["--read", "what", "is", "2+2"]);

        assert_eq!(
            build_task_args(&opts),
            strings(&[
                "-p",
                "what is 2+2",
                "--output-format",
                "json",
                "--permission-mode",
                "plan",
            ])
        );
    }

    #[test]
    fn build_task_args_effort_model_and_repeatable_add_dir_map_to_argv_but_cwd_is_not_forwarded() {
        let opts = parse(&[
            "--effort",
            "high",
            "--model",
            "opus",
            "--cwd",
            "/tmp/work",
            "--add-dir",
            "/tmp/a",
            "--add-dir",
            "/tmp/b",
            "do",
            "it",
        ]);

        assert_eq!(opts.cwd.as_deref(), Some("/tmp/work"));
        assert_eq!(
            build_task_args(&opts),
            strings(&[
                "-p",
                "do it",
                "--output-format",
                "json",
                "--model",
                "opus",
                "--permission-mode",
                "acceptEdits",
                "--effort",
                "high",
                "--add-dir",
                "/tmp/a",
                "--add-dir",
                "/tmp/b",
            ])
        );
    }

    #[test]
    fn build_task_args_worktree_bare_maps_to_bare_worktree_flag() {
        let opts = parse(&["do", "stuff", "--worktree"]);

        assert_eq!(
            build_task_args(&opts),
            strings(&[
                "-p",
                "do stuff",
                "--output-format",
                "json",
                "--permission-mode",
                "acceptEdits",
                "--worktree",
            ])
        );
    }

    #[test]
    fn build_task_args_worktree_name_maps_to_worktree_with_value() {
        let opts = parse(&["--worktree", "my-feature", "do", "stuff"]);

        assert_eq!(
            build_task_args(&opts),
            strings(&[
                "-p",
                "do stuff",
                "--output-format",
                "json",
                "--permission-mode",
                "acceptEdits",
                "--worktree",
                "my-feature",
            ])
        );
    }

    #[test]
    fn default_delegate_model_sets_model_in_argv() {
        let opts = parse_with_delegate_model(&["hi"], Some("opus"));

        assert_eq!(
            build_task_args(&opts),
            strings(&[
                "-p",
                "hi",
                "--output-format",
                "json",
                "--model",
                "opus",
                "--permission-mode",
                "acceptEdits",
            ])
        );
    }

    #[test]
    fn explicit_model_overrides_default_delegate_model() {
        let opts = parse_with_delegate_model(&["--model", "haiku", "hi"], Some("opus"));

        assert_eq!(
            build_task_args(&opts),
            strings(&[
                "-p",
                "hi",
                "--output-format",
                "json",
                "--model",
                "haiku",
                "--permission-mode",
                "acceptEdits",
            ])
        );
    }

    #[test]
    fn resume_with_session_id_maps_to_dash_r_session_id() {
        let opts = parse(&["--resume", "abc-123", "keep", "going"]);

        assert_eq!(
            build_task_args(&opts),
            strings(&[
                "-p",
                "keep going",
                "--output-format",
                "json",
                "--permission-mode",
                "acceptEdits",
                "-r",
                "abc-123",
            ])
        );
    }

    #[test]
    fn bare_resume_maps_to_dash_c() {
        let opts = parse(&["keep", "going", "--resume"]);

        assert_eq!(
            build_task_args(&opts),
            strings(&[
                "-p",
                "keep going",
                "--output-format",
                "json",
                "--permission-mode",
                "acceptEdits",
                "-c",
            ])
        );
    }

    #[test]
    fn bare_resume_followed_by_another_flag_maps_to_dash_c_without_swallowing_flag() {
        let opts = parse(&["--resume", "--read", "keep", "going"]);

        assert_eq!(
            build_task_args(&opts),
            strings(&[
                "-p",
                "keep going",
                "--output-format",
                "json",
                "--permission-mode",
                "plan",
                "-c",
            ])
        );
    }

    #[test]
    fn parse_task_args_missing_effort_value_hard_errors() {
        assert_task_arg_error(&["do", "it", "--effort"], "missing value for --effort");
    }

    #[test]
    fn parse_task_args_missing_model_value_hard_errors() {
        assert_task_arg_error(&["do", "it", "--model"], "missing value for --model");
    }

    #[test]
    fn parse_task_args_missing_cwd_value_hard_errors() {
        assert_task_arg_error(&["do", "it", "--cwd"], "missing value for --cwd");
    }

    #[test]
    fn parse_task_args_missing_add_dir_value_hard_errors() {
        assert_task_arg_error(&["do", "it", "--add-dir"], "missing value for --add-dir");
    }

    #[test]
    fn render_task_normalizes_claude_json_byte_identically() {
        let opts = parse(&["--read", "--model", "opus", "--cwd", "/tmp/work", "explain"]);
        let raw = r#"{"result":"The answer is 4.","session_id":"sess-abc","stop_reason":"end_turn","total_cost_usd":0.0123}"#;

        assert_eq!(
            render_task(raw, &opts),
            concat!(
                "=== claude delegate result ===\n",
                "mode:    read-only (plan)\n",
                "model:   opus\n",
                "stop:    end_turn\n",
                "session: sess-abc\n",
                "cost:    $0.0123\n",
                "\n",
                "The answer is 4.\n",
                "\n",
                "Continue this thread: claude -c   (in /tmp/work)\n",
            )
        );
    }

    #[test]
    fn render_task_write_mode_inherited_model_no_result_text_byte_identically() {
        let opts = parse(&["do", "it"]);
        let raw = r#"{"stop_reason":"end_turn"}"#;

        assert_eq!(
            render_task(raw, &opts),
            concat!(
                "=== claude delegate result ===\n",
                "mode:    write-capable\n",
                "model:   (inherits Claude Code default)\n",
                "stop:    end_turn\n",
                "\n",
                "(no result text returned)\n",
            )
        );
    }

    #[test]
    fn render_task_falls_back_to_trimmed_raw_text_when_json_parsing_fails() {
        let opts = parse(&["do", "it"]);

        assert_eq!(
            render_task("not json at all\n\n", &opts),
            "not json at all\n"
        );
    }
}
