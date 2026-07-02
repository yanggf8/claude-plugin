//! `parse_review_args`, `gather_diff`, `build_review_prompt`, `build_review_claude_args`, `render_review`

use std::env;
use std::fmt;
use std::path::Path;
use std::process::Command;

use serde::Deserialize;

pub const MAX_DIFF_CHARS: usize = 100_000;

/// Parsed routing options for the `review` subcommand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewOptions {
    pub background: bool,
    pub adversarial: bool,
    pub base: Option<String>,
    pub scope: String,
    pub effort: Option<String>,
    pub model: Option<String>,
    pub cwd: Option<String>,
    pub focus: String,
}

/// Collected diff text and metadata for review prompting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffResult {
    pub text: String,
    pub truncated: bool,
    pub empty: bool,
}

/// Error returned when review argv parsing fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewArgError {
    message: String,
}

impl ReviewArgError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn exit_code(&self) -> i32 {
        1
    }
}

impl fmt::Display for ReviewArgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ReviewArgError {}

fn delegate_model_from_env() -> Option<String> {
    env::var("CLAUDE_DELEGATE_MODEL")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

pub fn parse_review_args(argv: &[String]) -> Result<ReviewOptions, ReviewArgError> {
    let mut opts = ReviewOptions {
        background: false,
        adversarial: false,
        base: None,
        scope: "auto".to_string(),
        effort: None,
        model: delegate_model_from_env(),
        cwd: None,
        focus: String::new(),
    };

    let mut focus_parts: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < argv.len() {
        let a = argv[i].as_str();

        if matches!(a, "--base" | "--scope" | "--effort" | "--model" | "--cwd") {
            i += 1;
            let next = argv.get(i);
            match (a, next) {
                ("--base", Some(value)) => opts.base = Some(value.clone()),
                ("--scope", Some(value)) => opts.scope = value.clone(),
                ("--effort", Some(value)) => opts.effort = Some(value.clone()),
                ("--model", Some(value)) => opts.model = Some(value.clone()),
                ("--cwd", Some(value)) => opts.cwd = Some(value.clone()),
                (flag, None) => {
                    return Err(ReviewArgError::new(format!("missing value for {flag}")));
                }
                _ => unreachable!("matched only known value flags"),
            }
            i += 1;
        } else if a == "--background" {
            opts.background = true;
            i += 1;
        } else if a == "--wait" {
            i += 1;
        } else if a == "--adversarial" {
            opts.adversarial = true;
            i += 1;
        } else {
            focus_parts.push(&argv[i]);
            i += 1;
        }
    }

    opts.focus = focus_parts.join(" ").trim().to_string();

    if opts.scope == "auto" {
        opts.scope = if opts.base.is_some() {
            "branch".to_string()
        } else {
            "working-tree".to_string()
        };
    }
    if opts.scope == "branch" && opts.base.is_none() {
        opts.base = Some("main".to_string());
    }

    Ok(opts)
}

fn run_git(cwd: &Path, args: &[&str]) -> std::io::Result<String> {
    let output = Command::new("git").args(args).current_dir(cwd).output()?;
    if output.status.success() || !output.stdout.is_empty() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Ok(String::new())
    }
}

pub fn gather_diff(opts: &ReviewOptions, cwd: &Path) -> std::io::Result<DiffResult> {
    let (summary, patch) = if opts.scope == "branch" {
        let base = opts.base.as_deref().unwrap_or("main");
        let range = format!("{base}...HEAD");
        let summary = run_git(cwd, &["diff", &range, "--stat"])?;
        let patch = run_git(cwd, &["diff", &range])?;
        (summary, patch)
    } else {
        let summary = run_git(cwd, &["status", "--short", "--untracked-files=all"])?;
        let mut patch = run_git(cwd, &["diff", "HEAD"])?;
        for line in summary.lines() {
            if let Some(path) = line.strip_prefix("?? ") {
                let path = path.trim();
                if !path.is_empty() {
                    let untracked = run_git(cwd, &["diff", "--no-index", "/dev/null", path])?;
                    patch.push('\n');
                    patch.push_str(&untracked);
                }
            }
        }
        (summary, patch)
    };

    let summary_trimmed = summary.trim();
    let patch_trimmed = patch.trim();
    let mut combined = format!("# Change summary\n{summary_trimmed}\n\n# Diff\n{patch_trimmed}");
    let mut truncated = false;

    if combined.len() > MAX_DIFF_CHARS {
        combined.truncate(MAX_DIFF_CHARS);
        truncated = true;
    }

    Ok(DiffResult {
        text: combined,
        truncated,
        empty: summary_trimmed.is_empty() && patch_trimmed.is_empty(),
    })
}

pub fn build_review_prompt(opts: &ReviewOptions, diff: &DiffResult) -> String {
    let mut lines = Vec::new();

    lines.push(
        "You are performing a READ-ONLY code review. Do NOT edit, create, or delete any files — only inspect and report."
            .to_string(),
    );
    lines.push(String::new());

    if opts.scope == "branch" {
        let base = opts.base.as_deref().unwrap_or("main");
        lines.push(format!(
            "Scope: the changes on the current branch relative to `{base}`."
        ));
    } else {
        lines.push("Scope: the uncommitted working-tree changes.".to_string());
    }

    lines.push(
        "Base your review solely on the diff below. Do NOT use any tools — review the diff as given and answer directly with your findings as text."
            .to_string(),
    );

    if diff.truncated {
        lines.push(
            "(NOTE: the diff was truncated for length — review what is shown and flag that coverage is partial.)"
                .to_string(),
        );
    }

    lines.push(String::new());
    lines.push("```diff".to_string());
    lines.push(diff.text.clone());
    lines.push("```".to_string());
    lines.push(String::new());

    if opts.adversarial {
        lines.push(
            "Be adversarial: assume the change is wrong until proven otherwise. Actively hunt for correctness bugs, security issues, race conditions, broken error handling, and missed edge cases."
                .to_string(),
        );
        lines.push(String::new());
    }

    lines.push(
        "Report findings ordered by severity (blocker, high, medium, low, nit). For each finding give file:line, what the issue is, why it matters, and a concrete suggested fix."
            .to_string(),
    );

    if !opts.focus.is_empty() {
        lines.push(String::new());
        lines.push(format!("Reviewer focus from the user: {}", opts.focus));
    }

    lines.join("\n")
}

pub fn build_review_claude_args(opts: &ReviewOptions, diff: &DiffResult) -> Vec<String> {
    let mut args = vec![
        "-p".to_string(),
        build_review_prompt(opts, diff),
        "--output-format".to_string(),
        "json".to_string(),
        "--permission-mode".to_string(),
        "plan".to_string(),
    ];

    if let Some(model) = &opts.model {
        args.push("--model".to_string());
        args.push(model.clone());
    }
    if let Some(effort) = &opts.effort {
        args.push("--effort".to_string());
        args.push(effort.clone());
    }

    args
}

#[derive(Debug, Deserialize)]
struct ReviewRenderJson {
    result: Option<String>,
    session_id: Option<String>,
}

pub fn render_review(raw: &str, opts: &ReviewOptions) -> String {
    let parsed: Option<ReviewRenderJson> = serde_json::from_str(raw).ok();
    let Some(parsed) = parsed else {
        return format!("{}\n", raw.trim_end());
    };

    let mut lines = Vec::new();
    lines.push(format!(
        "=== claude {}review ===",
        if opts.adversarial { "adversarial " } else { "" },
    ));
    lines.push(format!(
        "scope:   {}",
        if opts.scope == "branch" {
            format!("branch vs {}", opts.base.as_deref().unwrap_or("main"))
        } else {
            "working tree".to_string()
        }
    ));
    lines.push(format!(
        "model:   {}",
        opts.model
            .as_deref()
            .unwrap_or("(inherits Claude Code default)")
    ));

    if let Some(session) = &parsed.session_id {
        lines.push(format!("session: {session}"));
    }

    lines.push(String::new());
    lines.push(
        parsed
            .result
            .unwrap_or_else(|| "(no review text returned)".to_string()),
    );

    format!("{}\n", lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env_lock;
    use std::fs::{self, write};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::MutexGuard;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static FIXTURE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    // Holds the crate-wide env lock for the fixture's whole lifetime. This
    // module never mutates env vars itself, but `resolve.rs`'s tests
    // temporarily clear `PATH` (e.g. to test `which claude` failing) — since
    // env vars are process-global, a `Command::new("git")` here could race
    // against that and fail with `NotFound`. Holding the same lock the whole
    // fixture is alive serializes against those tests without needing every
    // test function below to remember to acquire it individually.
    struct FixtureRepo {
        path: PathBuf,
        _env_guard: MutexGuard<'static, ()>,
    }

    impl FixtureRepo {
        fn init() -> Self {
            let env_guard = env_lock();
            let n = FIXTURE_COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "claude-companion-review-fixture-{}-{n}",
                std::process::id()
            ));
            if path.exists() {
                fs::remove_dir_all(&path).expect("remove stale fixture repo");
            }
            fs::create_dir_all(&path).expect("create fixture repo");

            let repo = Self {
                path,
                _env_guard: env_guard,
            };
            repo.git(&["init", "-q"]);
            repo.git(&["config", "user.email", "test@example.com"]);
            repo.git(&["config", "user.name", "Test"]);
            repo.write("a.txt", "line1\n");
            repo.git(&["add", "."]);
            repo.git(&["commit", "-q", "-m", "initial"]);
            repo
        }

        fn path(&self) -> &Path {
            &self.path
        }

        fn write(&self, rel: &str, content: &str) {
            let path = self.path.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create fixture parent dir");
            }
            write(path, content).expect("write fixture file");
        }

        fn git(&self, args: &[&str]) {
            let output = Command::new("git")
                .args(args)
                .current_dir(&self.path)
                .output()
                .expect("run git command");
            assert!(
                output.status.success(),
                "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
                args,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    impl Drop for FixtureRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn argv(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn parse_review_args_auto_scope_uses_working_tree_without_base() {
        let args = argv(&["race", "conditions"]);
        let opts = parse_review_args(&args).expect("parse review args");

        assert_eq!(opts.scope, "working-tree");
        assert_eq!(opts.base.as_deref(), None);
        assert_eq!(opts.focus, "race conditions");
        assert!(!opts.background);
        assert!(!opts.adversarial);
    }

    #[test]
    fn parse_review_args_auto_scope_uses_branch_when_base_is_given() {
        let args = argv(&["--base", "origin/main", "security", "only"]);
        let opts = parse_review_args(&args).expect("parse review args");

        assert_eq!(opts.scope, "branch");
        assert_eq!(opts.base.as_deref(), Some("origin/main"));
        assert_eq!(opts.focus, "security only");
    }

    #[test]
    fn parse_review_args_branch_scope_without_base_defaults_to_main() {
        let args = argv(&["--scope", "branch"]);
        let opts = parse_review_args(&args).expect("parse review args");

        assert_eq!(opts.scope, "branch");
        assert_eq!(opts.base.as_deref(), Some("main"));
    }

    #[test]
    fn parse_review_args_required_value_flags_missing_value_are_hard_errors() {
        for flag in ["--base", "--scope", "--effort", "--model", "--cwd"] {
            let args = argv(&[flag]);
            let err = parse_review_args(&args).expect_err("missing value should fail");

            assert_eq!(err.exit_code(), 1);
            assert!(
                err.to_string()
                    .contains(&format!("missing value for {flag}")),
                "unexpected error for {flag}: {err}"
            );
        }
    }

    #[test]
    fn render_review_normalizes_result_and_session_id_from_claude_json() {
        let args = argv(&[]);
        let opts = parse_review_args(&args).expect("parse review args");
        let raw =
            r#"{"result":"Looks fine overall.","session_id":"sess-xyz","stop_reason":"end_turn"}"#;

        let rendered = render_review(raw, &opts);

        assert!(rendered.contains("=== claude review ==="));
        assert!(rendered.contains("scope:   working tree"));
        assert!(rendered.contains("model:   (inherits Claude Code default)"));
        assert!(rendered.contains("session: sess-xyz"));
        assert!(rendered.contains("Looks fine overall."));
        assert!(!rendered.contains("undefined"));
    }

    #[test]
    fn render_review_falls_back_to_raw_text_when_json_parsing_fails() {
        let args = argv(&[]);
        let opts = parse_review_args(&args).expect("parse review args");

        assert_eq!(render_review("not json at all", &opts), "not json at all\n");
    }

    #[test]
    fn gather_diff_working_tree_scope_reports_modified_and_untracked_file_changes() {
        let repo = FixtureRepo::init();
        repo.write("a.txt", "line1\nline2\n");
        repo.write("new-file.txt", "brand new content\n");
        let args = argv(&[]);
        let opts = parse_review_args(&args).expect("parse review args");

        let diff = gather_diff(&opts, repo.path()).expect("gather diff");

        assert!(!diff.empty);
        assert!(diff.text.contains("a.txt"));
        assert!(diff.text.contains("new-file.txt"));
        assert!(diff.text.contains("+line2"));
        assert!(diff.text.contains("brand new content"));
        assert!(!diff.truncated);
    }

    #[test]
    fn gather_diff_working_tree_scope_reports_empty_when_nothing_changed() {
        let repo = FixtureRepo::init();
        let args = argv(&[]);
        let opts = parse_review_args(&args).expect("parse review args");

        let diff = gather_diff(&opts, repo.path()).expect("gather diff");

        assert!(diff.empty);
        assert_eq!(diff.text, "# Change summary\n\n\n# Diff\n");
        assert!(!diff.truncated);
    }

    #[test]
    fn gather_diff_branch_scope_diffs_against_the_given_base_ref() {
        let repo = FixtureRepo::init();
        repo.git(&["branch", "-m", "main"]);
        repo.git(&["checkout", "-q", "-b", "feature"]);
        repo.write("a.txt", "line1\nfeature-change\n");
        repo.git(&["add", "."]);
        repo.git(&["commit", "-q", "-m", "feature commit"]);
        let args = argv(&["--scope", "branch", "--base", "main"]);
        let opts = parse_review_args(&args).expect("parse review args");

        let diff = gather_diff(&opts, repo.path()).expect("gather diff");

        assert!(!diff.empty);
        assert!(diff.text.contains("feature-change"));
        assert!(diff.text.contains("# Change summary\n"));
        assert!(diff.text.contains("# Diff\n"));
        assert!(!diff.truncated);
    }

    #[test]
    fn gather_diff_branch_scope_reports_empty_when_branch_matches_base() {
        let repo = FixtureRepo::init();
        repo.git(&["branch", "-m", "main"]);
        let args = argv(&["--scope", "branch", "--base", "main"]);
        let opts = parse_review_args(&args).expect("parse review args");

        let diff = gather_diff(&opts, repo.path()).expect("gather diff");

        assert!(diff.empty);
        assert!(!diff.truncated);
    }

    #[test]
    fn gather_diff_truncates_combined_diff_text_over_max_diff_chars_and_sets_truncated_true() {
        assert_eq!(MAX_DIFF_CHARS, 100_000);

        let repo = FixtureRepo::init();
        let big_content = format!("{}\n", "x".repeat(MAX_DIFF_CHARS + 5_000));
        repo.write("big.txt", &big_content);
        let args = argv(&[]);
        let opts = parse_review_args(&args).expect("parse review args");

        let diff = gather_diff(&opts, repo.path()).expect("gather diff");

        assert!(diff.truncated);
        assert_eq!(diff.text.len(), MAX_DIFF_CHARS);
        assert!(!diff.empty);
    }

    #[test]
    fn gather_diff_does_not_truncate_when_combined_diff_text_is_under_max_diff_chars() {
        let repo = FixtureRepo::init();
        repo.write("a.txt", "line1\nsmall-change\n");
        let args = argv(&[]);
        let opts = parse_review_args(&args).expect("parse review args");

        let diff = gather_diff(&opts, repo.path()).expect("gather diff");

        assert!(!diff.truncated);
        assert!(diff.text.len() < MAX_DIFF_CHARS);
        assert!(diff.text.contains("small-change"));
    }

    #[test]
    fn build_review_prompt_includes_read_only_scope_diff_and_focus_text() {
        let repo = FixtureRepo::init();
        repo.write("a.txt", "line1\nline2\n");
        let args = argv(&["check", "error", "handling"]);
        let opts = parse_review_args(&args).expect("parse review args");
        let diff = gather_diff(&opts, repo.path()).expect("gather diff");

        let prompt = build_review_prompt(&opts, &diff);

        assert!(prompt.contains("READ-ONLY code review"));
        assert!(prompt.contains("Do NOT edit, create, or delete any files"));
        assert!(prompt.contains("Scope: the uncommitted working-tree changes."));
        assert!(prompt.contains("Base your review solely on the diff below."));
        assert!(prompt.contains("Do NOT use any tools"));
        assert!(prompt.contains("```diff"));
        assert!(prompt.contains("+line2"));
        assert!(prompt.contains("Reviewer focus from the user: check error handling"));
        assert!(!prompt.contains("Be adversarial"));
    }

    #[test]
    fn build_review_prompt_includes_branch_adversarial_and_truncated_variants() {
        let repo = FixtureRepo::init();
        repo.git(&["branch", "-m", "main"]);
        repo.git(&["checkout", "-q", "-b", "feature"]);
        let big_content = format!("{}\n", "x".repeat(MAX_DIFF_CHARS + 5_000));
        repo.write("a.txt", &format!("line1\n{big_content}"));
        repo.git(&["add", "."]);
        repo.git(&["commit", "-q", "-m", "feature commit"]);
        let args = argv(&["--adversarial", "--base", "main"]);
        let opts = parse_review_args(&args).expect("parse review args");
        let diff = gather_diff(&opts, repo.path()).expect("gather diff");

        let prompt = build_review_prompt(&opts, &diff);

        assert!(diff.truncated);
        assert!(prompt.contains("Scope: the changes on the current branch relative to `main`."));
        assert!(prompt.contains("the diff was truncated for length"));
        assert!(
            prompt.contains("Be adversarial: assume the change is wrong until proven otherwise.")
        );
    }

    #[test]
    fn build_review_claude_args_always_uses_plan_permission_and_never_forwards_cwd() {
        let repo = FixtureRepo::init();
        repo.write("a.txt", "line1\nline2\n");
        let args = argv(&[
            "--model",
            "opus",
            "--effort",
            "high",
            "--cwd",
            "/tmp/should-not-be-forwarded",
            "focus",
        ]);
        let opts = parse_review_args(&args).expect("parse review args");
        let diff = gather_diff(&opts, repo.path()).expect("gather diff");

        let claude_args = build_review_claude_args(&opts, &diff);

        assert_eq!(claude_args[0], "-p");
        assert!(claude_args[1].contains("READ-ONLY code review"));
        assert!(
            claude_args
                .windows(2)
                .any(|pair| pair == ["--output-format", "json"])
        );
        assert!(
            claude_args
                .windows(2)
                .any(|pair| pair == ["--permission-mode", "plan"])
        );
        assert!(
            claude_args
                .windows(2)
                .any(|pair| pair == ["--model", "opus"])
        );
        assert!(
            claude_args
                .windows(2)
                .any(|pair| pair == ["--effort", "high"])
        );
        assert!(!claude_args.iter().any(|arg| arg == "--cwd"));
        assert!(
            !claude_args
                .iter()
                .any(|arg| arg == "/tmp/should-not-be-forwarded")
        );
    }

    #[test]
    fn build_review_claude_args_omits_optional_model_and_effort_when_unset() {
        let repo = FixtureRepo::init();
        repo.write("a.txt", "line1\nline2\n");
        let args = argv(&[]);
        let opts = parse_review_args(&args).expect("parse review args");
        let diff = gather_diff(&opts, repo.path()).expect("gather diff");

        let claude_args = build_review_claude_args(&opts, &diff);

        assert!(!claude_args.iter().any(|arg| arg == "--model"));
        assert!(!claude_args.iter().any(|arg| arg == "--effort"));
        assert!(
            claude_args
                .windows(2)
                .any(|pair| pair == ["--permission-mode", "plan"])
        );
    }
}
