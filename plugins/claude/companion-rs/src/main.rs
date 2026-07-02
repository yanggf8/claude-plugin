mod exec;
mod resolve;
mod result;
mod review;
mod task;
#[cfg(test)]
mod test_support;

use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process;

use exec::{execute_claude, ExecuteClaudeResult};
use resolve::{require_ready, setup_report, SetupReport};
use review::{build_review_claude_args, gather_diff, parse_review_args, render_review};
use task::{build_task_args, parse_task_args, render_task};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let sub = args.first().map(String::as_str);

    match sub {
        Some("setup") => run_setup(&args[1..]),
        Some("task") => run_task(&args[1..]),
        Some("review") => run_review(&args[1..]),
        Some(_) => {
            print_usage();
            process::exit(1);
        }
        None => {
            print_usage();
            process::exit(0);
        }
    }
}

fn env_model_display() -> String {
    env::var("CLAUDE_DELEGATE_MODEL")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "(inherits Claude Code default)".to_string())
}

fn print_usage() {
    let default_model = env_model_display();
    let banner = format!(
        "claude-companion — drive the headless Claude Code CLI\n\
         \n\
         Default model: {default_model} (override with --model or CLAUDE_DELEGATE_MODEL)\n\
         \n\
         Usage:\n\
         \x20 claude-companion setup [--json] [--probe]\n\
         \x20 claude-companion task [routing flags] <task text>\n\
         \x20 claude-companion review [--base <ref>] [--scope auto|working-tree|branch]\n\
         \x20                         [--adversarial] [--background] [focus text]\n\
         \n\
         task routing flags:\n\
         \x20 --read | --background | --effort <l> | --model <id> | --cwd <p>\n\
         \x20 --add-dir <path> | --worktree [name] | --resume [session-id]\n\
         \n\
         review is always read-only (plan mode).\n\
         \n"
    );
    let _ = io::stdout().write_all(banner.as_bytes());
}

fn run_setup(argv: &[String]) {
    let json = argv.iter().any(|a| a == "--json");
    let probe = argv.iter().any(|a| a == "--probe");
    let report = setup_report(probe);

    if json {
        let body = serde_json::to_string_pretty(&report).unwrap_or_else(|err| {
            eprintln!("failed to serialize setup report: {err}");
            process::exit(1);
        });
        let _ = io::stdout().write_all(body.as_bytes());
        let _ = io::stdout().write_all(b"\n");
    } else {
        print_setup_human(&report);
    }

    process::exit(if report.ok { 0 } else { 1 });
}

fn print_setup_human(report: &SetupReport) {
    let mut lines = Vec::new();
    lines.push(format!(
        "claude binary:    {}",
        report
            .binary
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "NOT FOUND".to_string())
    ));
    lines.push(format!(
        "claude version:   {}",
        report.version.as_deref().unwrap_or("unknown")
    ));
    lines.push(format!(
        "authenticated:    {}",
        if report.authenticated { "yes" } else { "no" }
    ));
    if let Some(source) = &report.auth_source {
        lines.push(format!("auth source:      {source}"));
    }
    lines.push(format!("default model:    {}", report.default_model));

    if report.binary.is_none() {
        lines.push(String::new());
        lines.push("Claude Code CLI not found. Install:".to_string());
        lines.push("  npm install -g @anthropic-ai/claude-code".to_string());
    } else if !report.authenticated {
        lines.push(String::new());
        lines.push("Claude is installed but not signed in. Run:".to_string());
        lines.push("  claude".to_string());
        lines.push("or set ANTHROPIC_API_KEY in your environment.".to_string());
    } else if report.probe_ok == Some(false) {
        lines.push(String::new());
        lines.push(
            "Auth looks present but probe failed — re-run login or check API key.".to_string(),
        );
    } else {
        lines.push(String::new());
        lines.push("Claude is ready to delegate to.".to_string());
        lines.push("Use /claude:rescue or the claude-rescue subagent.".to_string());
    }

    let _ = io::stdout().write_all(format!("{}\n", lines.join("\n")).as_bytes());
}

fn run_task(argv: &[String]) {
    let bin = match require_ready() {
        Ok(bin) => bin,
        Err(err) => {
            let _ = io::stdout().write_all(format!("{err}\n").as_bytes());
            process::exit(1);
        }
    };

    let opts = match parse_task_args(argv) {
        Ok(opts) => opts,
        Err(err) => {
            let _ = io::stdout().write_all(format!("{err}\n").as_bytes());
            process::exit(err.exit_code());
        }
    };

    let cwd = resolve_cwd(opts.cwd.as_deref());
    let claude_args = build_task_args(&opts);
    let background = opts.background;

    match execute_claude(
        &bin,
        &claude_args,
        &cwd,
        background,
        "delegate",
        |raw| render_task(raw, &opts),
    ) {
        Ok(ExecuteClaudeResult::Foreground { output }) | Ok(ExecuteClaudeResult::Background {
            output,
            ..
        }) => {
            let _ = io::stdout().write_all(output.as_bytes());
            process::exit(0);
        }
        Err(err) => {
            let code = err.exit_code();
            let _ = io::stdout().write_all(err.output().as_bytes());
            process::exit(if code == 0 { 1 } else { code });
        }
    }
}

fn run_review(argv: &[String]) {
    let bin = match require_ready() {
        Ok(bin) => bin,
        Err(err) => {
            let _ = io::stdout().write_all(format!("{err}\n").as_bytes());
            process::exit(1);
        }
    };

    let opts = match parse_review_args(argv) {
        Ok(opts) => opts,
        Err(err) => {
            let _ = io::stdout().write_all(format!("{err}\n").as_bytes());
            process::exit(err.exit_code());
        }
    };

    let cwd = resolve_cwd(opts.cwd.as_deref());
    let diff = match gather_diff(&opts, &cwd) {
        Ok(diff) => diff,
        Err(err) => {
            let _ = io::stdout().write_all(format!("failed to gather diff: {err}\n").as_bytes());
            process::exit(1);
        }
    };

    if diff.empty {
        let scope_msg = if opts.scope == "branch" {
            format!(
                "changes vs {}",
                opts.base.as_deref().unwrap_or("main")
            )
        } else {
            "uncommitted changes".to_string()
        };
        let _ = io::stdout().write_all(
            format!("=== claude review ===\nNothing to review — no {scope_msg} found.\n").as_bytes(),
        );
        process::exit(0);
    }

    let claude_args = build_review_claude_args(&opts, &diff);
    let label = if opts.adversarial {
        "adversarial review"
    } else {
        "review"
    };
    let background = opts.background;

    match execute_claude(
        &bin,
        &claude_args,
        &cwd,
        background,
        label,
        |raw| render_review(raw, &opts),
    ) {
        Ok(ExecuteClaudeResult::Foreground { output }) | Ok(ExecuteClaudeResult::Background {
            output,
            ..
        }) => {
            let _ = io::stdout().write_all(output.as_bytes());
            process::exit(0);
        }
        Err(err) => {
            let code = err.exit_code();
            let _ = io::stdout().write_all(err.output().as_bytes());
            process::exit(if code == 0 { 1 } else { code });
        }
    }
}

fn resolve_cwd(cwd: Option<&str>) -> PathBuf {
    cwd.map(PathBuf::from)
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

