//! Shared test-only helpers. Rust runs `#[test]` functions concurrently in one
//! process, so any test that mutates process-global state (env vars) must
//! coordinate with every other test that reads that same state -- including
//! tests in other modules that shell out to `PATH`-resolved binaries like
//! `git`. `env_lock()` is the single, crate-wide lock all such tests acquire.

#![cfg(test)]

use std::sync::{Mutex, MutexGuard, OnceLock};

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
