//! `resolve_claude_bin`, `claude_version`, `auth_state`, `require_ready`, `plugin_root`

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthState {
    pub authenticated: bool,
    pub source: Option<String>,
    pub cred_path: Option<PathBuf>,
}

#[derive(Debug)]
pub struct RequireReadyError {
    message: String,
}

impl std::fmt::Display for RequireReadyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RequireReadyError {}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupReport {
    pub ok: bool,
    pub binary: Option<PathBuf>,
    pub version: Option<String>,
    pub authenticated: bool,
    pub auth_source: Option<String>,
    pub default_model: String,
    pub plugin_root: PathBuf,
    pub probe_ok: Option<bool>,
}

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn env_model() -> Option<String> {
    env::var("CLAUDE_DELEGATE_MODEL")
        .ok()
        .filter(|value| !value.is_empty())
}

fn path_exists(path: &Path) -> bool {
    fs::metadata(path).is_ok()
}

pub fn resolve_claude_bin() -> Option<PathBuf> {
    if let Ok(bin) = env::var("CLAUDE_BIN") {
        let path = PathBuf::from(bin);
        if path_exists(&path) {
            return Some(path);
        }
    }

    if let Some(path) = which_claude() {
        return Some(path);
    }

    let home = home_dir();
    let local = home.join(".local/bin/claude");
    if path_exists(&local) {
        return Some(local);
    }

    nvm_claude_fallback(&home)
}

fn which_claude() -> Option<PathBuf> {
    let output = Command::new("which").arg("claude").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return None;
    }
    Some(PathBuf::from(path))
}

fn nvm_claude_fallback(home: &Path) -> Option<PathBuf> {
    let versions_dir = home.join(".nvm/versions/node");
    let entries = fs::read_dir(&versions_dir).ok()?;
    let mut versions: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    versions.sort();
    versions.reverse();

    for version_dir in versions {
        let candidate = version_dir.join("bin/claude");
        if path_exists(&candidate) {
            return Some(candidate);
        }
    }

    None
}

pub fn claude_version(bin: Option<&Path>) -> Option<String> {
    let bin = bin?;
    let output = Command::new(bin).arg("--version").output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}").trim().to_string();
    if combined.is_empty() {
        None
    } else {
        Some(combined)
    }
}

pub fn auth_state() -> AuthState {
    if let Ok(key) = env::var("ANTHROPIC_API_KEY") {
        if !key.trim().is_empty() {
            return AuthState {
                authenticated: true,
                source: Some("api_key".into()),
                cred_path: None,
            };
        }
    }

    let cred_path = home_dir().join(".claude/.credentials.json");
    if let Some(auth) = read_oauth_credentials(&cred_path) {
        return auth;
    }

    AuthState {
        authenticated: false,
        source: None,
        cred_path: Some(cred_path),
    }
}

fn read_oauth_credentials(cred_path: &Path) -> Option<AuthState> {
    let metadata = fs::metadata(cred_path).ok()?;
    if metadata.len() == 0 {
        return None;
    }

    let raw = fs::read_to_string(cred_path).ok()?;
    let creds: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let _token = creds
        .get("claudeAiOauth")
        .and_then(|oauth| oauth.get("accessToken"))
        .and_then(|token| token.as_str())
        .filter(|token| !token.trim().is_empty())?;

    Some(AuthState {
        authenticated: true,
        source: Some("oauth".into()),
        cred_path: Some(cred_path.to_path_buf()),
    })
}

pub fn require_ready() -> Result<PathBuf, RequireReadyError> {
    let bin = resolve_claude_bin().ok_or_else(|| RequireReadyError {
        message: "claude CLI not found. Run /claude:setup for install hints.".into(),
    })?;

    let auth = auth_state();
    if !auth.authenticated {
        return Err(RequireReadyError {
            message:
                "claude is not authenticated. Run `claude` once to sign in, or set ANTHROPIC_API_KEY."
                    .into(),
        });
    }

    Ok(bin)
}

pub fn plugin_root() -> PathBuf {
    if let Ok(root) = env::var("GROK_PLUGIN_ROOT") {
        if !root.is_empty() {
            return PathBuf::from(root);
        }
    }
    if let Ok(root) = env::var("CLAUDE_PLUGIN_ROOT") {
        if !root.is_empty() {
            return PathBuf::from(root);
        }
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .join("..")
        .canonicalize()
        .unwrap_or_else(|_| manifest_dir.join(".."))
}

pub fn setup_report(probe: bool) -> SetupReport {
    let bin = resolve_claude_bin();
    let version = claude_version(bin.as_deref());
    let auth = auth_state();

    let mut probe_ok = None;
    if probe {
        if let (Some(bin_path), true) = (&bin, auth.authenticated) {
            let mut probe_args = vec![
                "-p".to_string(),
                "ping".to_string(),
                "--output-format".to_string(),
                "json".to_string(),
            ];
            if let Some(model) = env_model() {
                probe_args.push("--model".to_string());
                probe_args.push(model);
            }
            let output = Command::new(bin_path).args(&probe_args).output();
            probe_ok = Some(output.map(|o| o.status.success()).unwrap_or(false));
        }
    }

    let ready = bin.is_some() && auth.authenticated && probe_ok.unwrap_or(true);
    let default_model = env_model().unwrap_or_else(|| "(inherits Claude Code default)".to_string());

    SetupReport {
        ok: ready,
        binary: bin,
        version,
        authenticated: auth.authenticated,
        auth_source: auth.source,
        default_model,
        plugin_root: plugin_root(),
        probe_ok,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_support::env_lock;
    use serde_json::Value;
    use std::env;
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn setup_json_reports_inherited_default_model_when_unset() {
        let home = TempDir::new("claude-companion-resolve-home");
        let plugin_root = TempDir::new("claude-companion-plugin-root");
        let fake_claude = write_fake_claude(home.path().join("fake-claude"));

        with_env(
            &[
                ("CLAUDE_DELEGATE_MODEL", Some(OsStr::new(""))),
                ("CLAUDE_BIN", Some(fake_claude.as_os_str())),
                ("HOME", Some(home.path().as_os_str())),
                ("PATH", Some(OsStr::new(""))),
                ("ANTHROPIC_API_KEY", Some(OsStr::new("test-key"))),
                ("GROK_PLUGIN_ROOT", Some(plugin_root.path().as_os_str())),
                ("CLAUDE_PLUGIN_ROOT", None),
            ],
            || {
                let body = serde_json::to_value(setup_report(false))
                    .expect("setup report should serialize to JSON");

                assert_eq!(body["ok"], Value::Bool(true));
                assert_eq!(
                    body["binary"],
                    Value::String(fake_claude.display().to_string())
                );
                assert_eq!(body["version"], Value::String("Claude Code 1.2.3".into()));
                assert_eq!(body["authenticated"], Value::Bool(true));
                assert_eq!(body["authSource"], Value::String("api_key".into()));
                assert_eq!(
                    body["defaultModel"],
                    Value::String("(inherits Claude Code default)".into())
                );
                assert_eq!(
                    body["pluginRoot"],
                    Value::String(plugin_root.path().display().to_string())
                );
                assert_eq!(body["probeOk"], Value::Null);
            },
        );
    }

    #[test]
    fn setup_json_reports_custom_default_model_from_claude_delegate_model() {
        let home = TempDir::new("claude-companion-resolve-home");
        let plugin_root = TempDir::new("claude-companion-plugin-root");
        let fake_claude = write_fake_claude(home.path().join("fake-claude"));

        with_env(
            &[
                ("CLAUDE_DELEGATE_MODEL", Some(OsStr::new("opus"))),
                ("CLAUDE_BIN", Some(fake_claude.as_os_str())),
                ("HOME", Some(home.path().as_os_str())),
                ("PATH", Some(OsStr::new(""))),
                ("ANTHROPIC_API_KEY", Some(OsStr::new("test-key"))),
                ("GROK_PLUGIN_ROOT", Some(plugin_root.path().as_os_str())),
                ("CLAUDE_PLUGIN_ROOT", None),
            ],
            || {
                let body = serde_json::to_value(setup_report(false))
                    .expect("setup report should serialize to JSON");

                assert_eq!(body["defaultModel"], Value::String("opus".into()));
            },
        );
    }

    #[test]
    fn resolve_claude_bin_prefers_claude_bin_env_var_when_it_exists_on_disk() {
        let home = TempDir::new("claude-companion-resolve-home");
        let fake_claude = write_fake_claude(home.path().join("my-claude"));
        let expected = fake_claude.clone();

        with_env(
            &[
                ("CLAUDE_BIN", Some(fake_claude.as_os_str())),
                ("HOME", Some(home.path().as_os_str())),
                ("PATH", Some(OsStr::new(""))),
            ],
            || {
                assert_eq!(resolve_claude_bin(), Some(expected));
            },
        );
    }

    #[test]
    fn resolve_claude_bin_ignores_missing_claude_bin_env_var_and_falls_through() {
        let home = TempDir::new("claude-companion-resolve-home");
        let path_dir = home.path().join("path-bin");
        fs::create_dir_all(&path_dir).unwrap();
        write_failing_which(path_dir.join("which"));

        let fallback = write_fake_claude(home.path().join(".local/bin/claude"));

        with_env(
            &[
                (
                    "CLAUDE_BIN",
                    Some(home.path().join("does-not-exist").as_os_str()),
                ),
                ("HOME", Some(home.path().as_os_str())),
                ("PATH", Some(path_dir.as_os_str())),
            ],
            || {
                assert_eq!(resolve_claude_bin(), Some(fallback));
            },
        );
    }

    #[test]
    fn resolve_claude_bin_falls_back_to_which_claude_on_path_when_claude_bin_unset() {
        let home = TempDir::new("claude-companion-resolve-home");
        let path_dir = home.path().join("path-bin");
        fs::create_dir_all(&path_dir).unwrap();

        let path_claude = write_fake_claude(path_dir.join("claude"));
        let expected = path_claude.clone();
        write_successful_which(path_dir.join("which"));

        with_env(
            &[
                ("CLAUDE_BIN", None),
                ("HOME", Some(home.path().as_os_str())),
                ("PATH", Some(path_dir.as_os_str())),
                ("CLAUDE_WHICH_RESULT", Some(path_claude.as_os_str())),
            ],
            || {
                assert_eq!(resolve_claude_bin(), Some(expected));
            },
        );
    }

    #[test]
    fn resolve_claude_bin_falls_back_to_local_home_path_when_which_fails() {
        let home = TempDir::new("claude-companion-resolve-home");
        let path_dir = home.path().join("path-bin");
        fs::create_dir_all(&path_dir).unwrap();
        write_failing_which(path_dir.join("which"));

        let local_claude = write_fake_claude(home.path().join(".local/bin/claude"));
        let nvm_claude =
            write_fake_claude(home.path().join(".nvm/versions/node/v20.11.1/bin/claude"));

        with_env(
            &[
                ("CLAUDE_BIN", None),
                ("HOME", Some(home.path().as_os_str())),
                ("PATH", Some(path_dir.as_os_str())),
            ],
            || {
                assert_eq!(resolve_claude_bin(), Some(local_claude));
                assert!(
                    nvm_claude.exists(),
                    "fixture should include the lower-priority nvm fallback"
                );
            },
        );
    }

    #[test]
    fn resolve_claude_bin_falls_back_to_nvm_home_path_when_local_path_is_absent() {
        let home = TempDir::new("claude-companion-resolve-home");
        let path_dir = home.path().join("path-bin");
        fs::create_dir_all(&path_dir).unwrap();
        write_failing_which(path_dir.join("which"));

        let nvm_claude =
            write_fake_claude(home.path().join(".nvm/versions/node/v20.11.1/bin/claude"));

        with_env(
            &[
                ("CLAUDE_BIN", None),
                ("HOME", Some(home.path().as_os_str())),
                ("PATH", Some(path_dir.as_os_str())),
            ],
            || {
                assert_eq!(resolve_claude_bin(), Some(nvm_claude));
            },
        );
    }

    #[test]
    fn resolve_claude_bin_returns_none_when_no_layer_resolves() {
        let home = TempDir::new("claude-companion-resolve-home");
        let path_dir = home.path().join("path-bin");
        fs::create_dir_all(&path_dir).unwrap();
        write_failing_which(path_dir.join("which"));

        with_env(
            &[
                ("CLAUDE_BIN", None),
                ("HOME", Some(home.path().as_os_str())),
                ("PATH", Some(path_dir.as_os_str())),
            ],
            || {
                assert_eq!(resolve_claude_bin(), None);
            },
        );
    }

    #[test]
    fn claude_version_returns_trimmed_stdout_or_stderr_for_existing_binary() {
        let home = TempDir::new("claude-companion-resolve-home");
        let fake_claude = write_fake_claude(home.path().join("fake-claude"));

        assert_eq!(
            claude_version(Some(fake_claude.as_path())).as_deref(),
            Some("Claude Code 1.2.3")
        );
    }

    #[test]
    fn claude_version_returns_none_without_a_binary() {
        assert_eq!(claude_version(None), None);
    }

    #[test]
    fn auth_state_prefers_non_empty_anthropic_api_key() {
        let home = TempDir::new("claude-companion-auth-home");

        with_env(
            &[
                ("HOME", Some(home.path().as_os_str())),
                ("ANTHROPIC_API_KEY", Some(OsStr::new("  test-key  "))),
            ],
            || {
                let auth = auth_state();
                assert!(auth.authenticated);
                assert_eq!(auth.source.as_deref(), Some("api_key"));
            },
        );
    }

    #[test]
    fn auth_state_reads_oauth_access_token_from_credentials_file() {
        let home = TempDir::new("claude-companion-auth-home");
        let cred_path = home.path().join(".claude/.credentials.json");
        write_file(
            &cred_path,
            r#"{"claudeAiOauth":{"accessToken":"oauth-token"}}"#,
        );

        with_env(
            &[
                ("HOME", Some(home.path().as_os_str())),
                ("ANTHROPIC_API_KEY", None),
            ],
            || {
                let auth = auth_state();
                assert!(auth.authenticated);
                assert_eq!(auth.source.as_deref(), Some("oauth"));
                assert_eq!(auth.cred_path.as_deref(), Some(cred_path.as_path()));
            },
        );
    }

    #[test]
    fn auth_state_reports_unauthenticated_for_missing_empty_or_invalid_credentials() {
        let missing_home = TempDir::new("claude-companion-auth-missing-home");

        with_env(
            &[
                ("HOME", Some(missing_home.path().as_os_str())),
                ("ANTHROPIC_API_KEY", None),
            ],
            || {
                let auth = auth_state();
                assert!(!auth.authenticated);
                assert_eq!(auth.source, None);
                assert_eq!(
                    auth.cred_path.as_deref(),
                    Some(
                        missing_home
                            .path()
                            .join(".claude/.credentials.json")
                            .as_path()
                    )
                );
            },
        );

        let invalid_home = TempDir::new("claude-companion-auth-invalid-home");
        write_file(
            &invalid_home.path().join(".claude/.credentials.json"),
            "not json",
        );

        with_env(
            &[
                ("HOME", Some(invalid_home.path().as_os_str())),
                ("ANTHROPIC_API_KEY", None),
            ],
            || {
                let auth = auth_state();
                assert!(!auth.authenticated);
                assert_eq!(auth.source, None);
            },
        );
    }

    #[test]
    fn require_ready_returns_binary_when_resolved_and_authenticated() {
        let home = TempDir::new("claude-companion-ready-home");
        let fake_claude = write_fake_claude(home.path().join("fake-claude"));
        let expected = fake_claude.clone();

        with_env(
            &[
                ("CLAUDE_BIN", Some(fake_claude.as_os_str())),
                ("HOME", Some(home.path().as_os_str())),
                ("PATH", Some(OsStr::new(""))),
                ("ANTHROPIC_API_KEY", Some(OsStr::new("test-key"))),
            ],
            || {
                assert_eq!(require_ready().expect("ready env should pass"), expected);
            },
        );
    }

    #[test]
    fn require_ready_errors_clearly_when_claude_cli_is_missing() {
        let home = TempDir::new("claude-companion-ready-home");

        with_env(
            &[
                ("CLAUDE_BIN", None),
                ("HOME", Some(home.path().as_os_str())),
                ("PATH", Some(OsStr::new(""))),
                ("ANTHROPIC_API_KEY", Some(OsStr::new("test-key"))),
            ],
            || {
                let err = require_ready().expect_err("missing claude should be an error");
                assert_eq!(
                    err.to_string(),
                    "claude CLI not found. Run /claude:setup for install hints."
                );
            },
        );
    }

    #[test]
    fn require_ready_errors_clearly_when_auth_is_missing() {
        let home = TempDir::new("claude-companion-ready-home");
        let fake_claude = write_fake_claude(home.path().join("fake-claude"));

        with_env(
            &[
                ("CLAUDE_BIN", Some(fake_claude.as_os_str())),
                ("HOME", Some(home.path().as_os_str())),
                ("PATH", Some(OsStr::new(""))),
                ("ANTHROPIC_API_KEY", None),
            ],
            || {
                let err = require_ready().expect_err("missing auth should be an error");
                assert_eq!(
                    err.to_string(),
                    "claude is not authenticated. Run `claude` once to sign in, or set ANTHROPIC_API_KEY."
                );
            },
        );
    }

    #[test]
    fn plugin_root_prefers_grok_plugin_root_then_claude_plugin_root() {
        let grok_root = TempDir::new("claude-companion-grok-root");
        let claude_root = TempDir::new("claude-companion-claude-root");

        with_env(
            &[
                ("GROK_PLUGIN_ROOT", Some(grok_root.path().as_os_str())),
                ("CLAUDE_PLUGIN_ROOT", Some(claude_root.path().as_os_str())),
            ],
            || {
                assert_eq!(plugin_root(), grok_root.path());
            },
        );

        with_env(
            &[
                ("GROK_PLUGIN_ROOT", None),
                ("CLAUDE_PLUGIN_ROOT", Some(claude_root.path().as_os_str())),
            ],
            || {
                assert_eq!(plugin_root(), claude_root.path());
            },
        );
    }

    #[test]
    fn plugin_root_falls_back_to_the_claude_plugin_directory() {
        with_env(
            &[("GROK_PLUGIN_ROOT", None), ("CLAUDE_PLUGIN_ROOT", None)],
            || {
                let root = plugin_root();
                assert!(root.is_absolute());
                assert!(root.ends_with(Path::new("plugins/claude")));
            },
        );
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(prefix: &str) -> Self {
            for attempt in 0..100 {
                let nanos = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos();
                let path =
                    env::temp_dir().join(format!("{prefix}-{}-{nanos}-{attempt}", process::id()));
                match fs::create_dir(&path) {
                    Ok(()) => return Self { path },
                    Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(err) => panic!("failed to create temp dir {path:?}: {err}"),
                }
            }
            panic!("failed to create unique temp dir for {prefix}");
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

    fn write_fake_claude(path: PathBuf) -> PathBuf {
        write_executable(
            &path,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'Claude Code 1.2.3'; exit 0; fi\nif [ \"$1\" = \"-p\" ]; then printf '{\"result\":\"pong\",\"session_id\":\"sess-test\"}\\n'; exit 0; fi\nexit 0\n",
        );
        path
    }

    fn write_successful_which(path: PathBuf) {
        write_executable(
            &path,
            "#!/bin/sh\nif [ \"$1\" = \"claude\" ]; then printf '%s\\n' \"$CLAUDE_WHICH_RESULT\"; exit 0; fi\nexit 1\n",
        );
    }

    fn write_failing_which(path: PathBuf) {
        write_executable(&path, "#!/bin/sh\nexit 1\n");
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

    fn with_env<R>(vars: &[(&str, Option<&OsStr>)], f: impl FnOnce() -> R) -> R {
        let _guard = env_lock();
        let saved: Vec<(&str, Option<OsString>)> = vars
            .iter()
            .map(|(key, _)| (*key, env::var_os(key)))
            .collect();

        for (key, value) in vars {
            set_env_var(key, *value);
        }

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));

        for (key, value) in saved {
            set_env_var(key, value.as_deref());
        }

        match result {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    fn set_env_var(key: &str, value: Option<&OsStr>) {
        unsafe {
            match value {
                Some(value) => env::set_var(key, value),
                None => env::remove_var(key),
            }
        }
    }
}
