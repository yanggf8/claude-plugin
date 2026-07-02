//! Integration test harness scaffold for the compiled `claude-companion` binary.

#[test]
fn harness_runs() {
    assert_eq!(1 + 1, 2);
}

use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn companion_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_claude-companion"))
}

fn run_companion(args: &[&str]) -> Output {
    Command::new(companion_bin())
        .args(args)
        .env("CLAUDE_DELEGATE_MODEL", "")
        .output()
        .expect("claude-companion binary should run")
}

fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn status_code(output: &Output) -> i32 {
    output.status.code().unwrap_or(1)
}

fn live_enabled() -> bool {
    env::var("CLAUDE_INTEGRATION").as_deref() == Ok("1")
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        let path = env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path).expect("temp directory should be created");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn no_subcommand_prints_usage_and_exits_zero() {
    let output = run_companion(&[]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(status_code(&output), 0, "{}", output_text(&output));
    assert!(stdout.contains("Usage:"), "stdout:\n{stdout}");
    assert!(stdout.contains("setup"), "stdout:\n{stdout}");
    assert!(stdout.contains("task"), "stdout:\n{stdout}");
    assert!(stdout.contains("review"), "stdout:\n{stdout}");
}

#[test]
fn unknown_subcommand_exits_non_zero() {
    let output = run_companion(&["not-a-real-subcommand"]);

    assert_ne!(status_code(&output), 0, "{}", output_text(&output));
}

#[test]
fn task_without_prompt_exits_one_with_message() {
    let output = run_companion(&["task"]);
    let text = output_text(&output);

    assert_eq!(status_code(&output), 1, "{text}");
    assert!(
        text.to_ascii_lowercase().contains("no task text"),
        "expected missing prompt message, got:\n{text}"
    );
}

#[test]
fn setup_json_outputs_valid_report_shape() {
    let output = run_companion(&["setup", "--json"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let body: Value = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!(
            "setup --json should print valid JSON, parse error: {err}\nstdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });

    let ok = body
        .get("ok")
        .and_then(Value::as_bool)
        .expect("setup JSON should contain boolean ok field");
    body.get("defaultModel")
        .and_then(Value::as_str)
        .expect("setup JSON should contain string defaultModel field");

    assert_eq!(status_code(&output), if ok { 0 } else { 1 }, "{stdout}");
}

#[test]
fn live_setup_json_reports_authenticated() {
    if !live_enabled() {
        return;
    }

    let output = run_companion(&["setup", "--json"]);
    assert_eq!(status_code(&output), 0, "{}", output_text(&output));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let body: Value = serde_json::from_str(&stdout).expect("setup --json should print JSON");
    assert_eq!(
        body.get("authenticated").and_then(Value::as_bool),
        Some(true)
    );
}

#[test]
fn live_task_read_foreground_renders_delegate_result() {
    if !live_enabled() {
        return;
    }

    let output = run_companion(&["task", "--read", "what is 2+2"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(status_code(&output), 0, "{}", output_text(&output));
    assert!(
        stdout.contains("=== claude delegate result ==="),
        "stdout:\n{stdout}"
    );
}

#[test]
fn live_default_write_mode_writes_file_headless() {
    if !live_enabled() {
        return;
    }

    let dir = TempDir::new("claude-companion-live-write");
    let marker = format!(
        "CLAUDE_COMPANION_MARKER_{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_millis()
    );
    let target_file = dir.path().join("marker.txt");
    let prompt = format!(
        "Write the exact text \"{marker}\" (nothing else) to the file at path {}. Create the file if it does not exist. Do not ask for confirmation.",
        target_file.display()
    );

    let output = Command::new(companion_bin())
        .args(["task", "--cwd"])
        .arg(dir.path())
        .arg(prompt)
        .env("CLAUDE_DELEGATE_MODEL", "")
        .output()
        .expect("claude-companion binary should run");

    assert_eq!(status_code(&output), 0, "{}", output_text(&output));
    assert!(
        target_file.exists(),
        "expected {} to be written",
        target_file.display()
    );
    let contents = fs::read_to_string(&target_file)
        .unwrap_or_else(|err| panic!("expected to read {}: {err}", target_file.display()));
    assert!(
        contents.contains(&marker),
        "expected file to contain marker {marker}, got:\n{contents}"
    );
}

#[test]
fn live_background_task_log_eventually_contains_result_json() {
    if !live_enabled() {
        return;
    }

    let output = run_companion(&["task", "--background", "say the word ok and nothing else"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(status_code(&output), 0, "{}", output_text(&output));
    assert!(
        extract_after_label(&stdout, "pid:").is_some(),
        "stdout:\n{stdout}"
    );
    let log_file = extract_after_label(&stdout, "log:")
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("expected a log path in output:\n{stdout}"));

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut final_json = None;
    while Instant::now() < deadline {
        if let Ok(content) = fs::read_to_string(&log_file) {
            if let Some(parsed) = try_parse_final_json(&content) {
                if parsed.get("result").is_some() || parsed.get("session_id").is_some() {
                    final_json = Some(parsed);
                    break;
                }
            }
        }
        thread::sleep(Duration::from_millis(500));
    }

    let final_json = final_json.unwrap_or_else(|| {
        panic!(
            "timed out waiting for parseable JSON result in {}",
            log_file.display()
        )
    });
    assert!(
        final_json.get("result").is_some(),
        "parsed JSON: {final_json}"
    );
    assert!(
        final_json.get("session_id").is_some(),
        "parsed JSON: {final_json}"
    );
}

fn extract_after_label<'a>(text: &'a str, label: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|line| line.trim_start().strip_prefix(label).map(str::trim))
        .filter(|value| !value.is_empty())
}

fn try_parse_final_json(content: &str) -> Option<Value> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(value) = serde_json::from_str(trimmed) {
        return Some(value);
    }

    let last = trimmed.lines().rev().find(|line| !line.trim().is_empty())?;
    serde_json::from_str(last).ok()
}
