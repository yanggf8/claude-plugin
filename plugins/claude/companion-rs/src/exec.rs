//! `execute_claude`: foreground capture and detached background spawn + logfile

use std::env;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

/// Outcome of launching `claude` in foreground or background mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecuteClaudeResult {
    Foreground {
        output: String,
    },
    Background {
        pid: u32,
        log_file: PathBuf,
        output: String,
    },
}

/// Failure from foreground `claude` execution (non-zero exit or empty stdout on success).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecuteClaudeError {
    exit_code: i32,
    output: String,
}

impl ExecuteClaudeError {
    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }

    pub fn output(&self) -> &str {
        &self.output
    }
}

pub fn execute_claude<F>(
    bin: &Path,
    args: &[String],
    cwd: &Path,
    background: bool,
    label: &str,
    mut render: F,
) -> Result<ExecuteClaudeResult, ExecuteClaudeError>
where
    F: FnMut(&str) -> String,
{
    if background {
        execute_background(bin, args, cwd, label)
    } else {
        execute_foreground(bin, args, cwd, label, &mut render)
    }
}

fn execute_foreground<F>(
    bin: &Path,
    args: &[String],
    cwd: &Path,
    label: &str,
    render: &mut F,
) -> Result<ExecuteClaudeResult, ExecuteClaudeError>
where
    F: FnMut(&str) -> String,
{
    let output = Command::new(bin)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|err| ExecuteClaudeError {
            exit_code: 1,
            output: format!("Failed to launch claude: {err}\n"),
        })?;

    let exit_code = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    if exit_code == 0 && !stdout.trim().is_empty() {
        return Ok(ExecuteClaudeResult::Foreground {
            output: render(&stdout),
        });
    }

    Err(ExecuteClaudeError {
        exit_code,
        output: format_failure_output(label, exit_code, &stdout, &stderr),
    })
}

fn execute_background(
    bin: &Path,
    args: &[String],
    cwd: &Path,
    label: &str,
) -> Result<ExecuteClaudeResult, ExecuteClaudeError> {
    let log_dir = env::temp_dir().join("claude-delegate");
    let _ = fs::create_dir_all(&log_dir);

    let log_file = log_dir.join(format!("claude-{}.log", std::process::id()));
    let log_handle = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
        .map_err(|err| ExecuteClaudeError {
            exit_code: 1,
            output: format!("Failed to open background log file: {err}\n"),
        })?;

    let stdout = log_handle.try_clone().map_err(|err| ExecuteClaudeError {
        exit_code: 1,
        output: format!("Failed to duplicate log file handle: {err}\n"),
    })?;

    let mut cmd = Command::new(bin);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(log_handle);

    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let child = cmd.spawn().map_err(|err| ExecuteClaudeError {
        exit_code: 1,
        output: format!("Failed to launch claude: {err}\n"),
    })?;

    let pid = child.id();
    let log_display = log_file.display();
    let cwd_display = cwd.display();

    let output = format!(
        "=== claude {label} (background) ===\n\
         pid:     {pid}\n\
         cwd:     {cwd_display}\n\
         log:     {log_display}\n\
         \n\
         Running in the background. Tail the log to watch:\n\
           tail -f {log_display}\n\
         When it finishes, the JSON result (incl. result + session_id) is in that file.\n\
         \n"
    );

    Ok(ExecuteClaudeResult::Background {
        pid,
        log_file,
        output,
    })
}

fn format_failure_output(label: &str, exit_code: i32, stdout: &str, stderr: &str) -> String {
    let mut parts = vec![
        format!("=== claude {label} failed ==="),
        format!("exit code: {exit_code}"),
    ];

    if !stdout.trim().is_empty() {
        parts.push(String::new());
        parts.push("stdout:".to_string());
        parts.push(stdout.trim().to_string());
    }

    if !stderr.trim().is_empty() {
        let lines: Vec<&str> = stderr.trim().split('\n').collect();
        let tail_start = lines.len().saturating_sub(12);
        parts.push(String::new());
        parts.push("stderr (tail):".to_string());
        parts.push(lines[tail_start..].join("\n"));
    }

    parts.push(String::new());
    parts.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::{execute_claude, ExecuteClaudeResult};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process;
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn execute_claude_foreground_renders_raw_stdout() {
        let temp_dir = TempDir::new("claude-companion-exec-foreground");
        let script = temp_dir.path().join("fake-claude");
        write_executable(
            &script,
            "#!/bin/sh\nprintf '{\"result\":\"ok\",\"session_id\":\"sess-foreground\"}\\n'\n",
        );

        let mut raw_seen = None;
        let result = execute_claude(
            &script,
            &[],
            temp_dir.path(),
            false,
            "delegate",
            |raw| {
                raw_seen = Some(raw.to_string());
                format!("rendered: {raw}")
            },
        )
        .unwrap();

        let raw = "{\"result\":\"ok\",\"session_id\":\"sess-foreground\"}\n";
        assert_eq!(raw_seen.as_deref(), Some(raw));
        assert_eq!(
            result,
            ExecuteClaudeResult::Foreground {
                output: format!("rendered: {raw}"),
            }
        );
    }

    #[test]
    fn execute_claude_foreground_failure_reports_exit_stdout_and_stderr_tail() {
        let temp_dir = TempDir::new("claude-companion-exec-failure");
        let script = temp_dir.path().join("fake-claude");
        write_executable(
            &script,
            "#!/bin/sh\n\
             printf 'stdout payload\\n'\n\
             i=1\n\
             while [ \"$i\" -le 15 ]; do\n\
               printf 'stderr line %02d\\n' \"$i\" >&2\n\
               i=$((i + 1))\n\
             done\n\
             exit 7\n",
        );

        let err = execute_claude(&script, &[], temp_dir.path(), false, "delegate", |raw| {
            format!("should not render: {raw}")
        })
        .unwrap_err();

        assert_eq!(err.exit_code(), 7);
        assert_eq!(
            err.output(),
            "=== claude delegate failed ===\n\
             exit code: 7\n\
             \n\
             stdout:\n\
             stdout payload\n\
             \n\
             stderr (tail):\n\
             stderr line 04\n\
             stderr line 05\n\
             stderr line 06\n\
             stderr line 07\n\
             stderr line 08\n\
             stderr line 09\n\
             stderr line 10\n\
             stderr line 11\n\
             stderr line 12\n\
             stderr line 13\n\
             stderr line 14\n\
             stderr line 15\n\
             \n"
        );
    }

    #[test]
    fn execute_claude_background_detaches_and_writes_log() {
        let temp_dir = TempDir::new("claude-companion-exec-background");
        let script = temp_dir.path().join("fake-claude");
        write_executable(
            &script,
            "#!/bin/sh\nsleep 2\nprintf '{\"result\":\"ok\",\"session_id\":\"sess-background\"}\\n'\n",
        );

        let expected_log = std::env::temp_dir()
            .join("claude-delegate")
            .join(format!("claude-{}.log", process::id()));
        let _ = fs::remove_file(&expected_log);

        let start = Instant::now();
        let result = execute_claude(
            &script,
            &[],
            temp_dir.path(),
            true,
            "delegate",
            |raw| format!("should not render: {raw}"),
        )
        .unwrap();
        let elapsed = start.elapsed();

        let (pid, log_file, output) = match result {
            ExecuteClaudeResult::Background {
                pid,
                log_file,
                output,
            } => (pid, log_file, output),
            other => panic!("expected background result, got {other:?}"),
        };

        assert!(pid > 0);
        assert_eq!(log_file, expected_log);
        assert!(log_file.is_file());
        assert!(
            elapsed < Duration::from_secs(1),
            "background launch took {elapsed:?}, expected true detachment before script sleep"
        );
        assert!(output.contains("=== claude delegate (background) ==="));
        assert!(output.contains(&format!("pid:     {pid}")));
        assert!(output.contains(&format!("log:     {}", log_file.display())));

        let expected_json = "{\"result\":\"ok\",\"session_id\":\"sess-background\"}";
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let contents = fs::read_to_string(&log_file).unwrap_or_default();
            if contents.contains(expected_json) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {expected_json} in {}",
                log_file.display()
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(prefix: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("{prefix}-{unique}"));
            fs::create_dir_all(&path).unwrap();
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

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    fn write_executable(path: &Path, contents: &str) {
        write_file(path, contents);
        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).unwrap();
        }
    }
}
