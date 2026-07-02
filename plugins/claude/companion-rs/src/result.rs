//! `ClaudeResult` (serde struct) + `parse_result`

use serde::Deserialize;

/// Parsed Claude CLI JSON output (`--output-format json`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ClaudeResult {
    pub result: Option<String>,
    pub session_id: Option<String>,
    pub stop_reason: Option<String>,
    pub total_cost_usd: Option<TotalCost>,
}

/// `total_cost_usd` accepts a JSON number or numeric string (Node `Number()` coercion).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TotalCost(f64);

impl<'de> Deserialize<'de> for TotalCost {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::Number(number) => number
                .as_f64()
                .map(TotalCost)
                .ok_or_else(|| serde::de::Error::custom("invalid number for total_cost_usd")),
            serde_json::Value::String(text) => text
                .parse::<f64>()
                .map(TotalCost)
                .map_err(|_| serde::de::Error::custom("invalid numeric string for total_cost_usd")),
            _ => Err(serde::de::Error::custom(
                "total_cost_usd must be a number or numeric string",
            )),
        }
    }
}

/// Parse Claude stdout JSON; returns `None` on malformed/non-JSON input (raw passthrough upstream).
pub fn parse_result(raw: &str) -> Option<ClaudeResult> {
    serde_json::from_str(raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::{parse_review_args, render_review};
    use crate::task::{parse_task_args, render_task};

    fn argv(tokens: &[&str]) -> Vec<String> {
        tokens.iter().map(|token| (*token).to_string()).collect()
    }

    #[test]
    fn parse_result_parses_claudes_real_field_names() {
        let raw = r#"{
            "result": "42",
            "session_id": "sess-123",
            "stop_reason": "end_turn",
            "total_cost_usd": 0.0042
        }"#;

        let parsed = parse_result(raw).expect("valid Claude JSON should parse");

        assert_eq!(parsed.result.as_deref(), Some("42"));
        assert_eq!(parsed.session_id.as_deref(), Some("sess-123"));
        assert_eq!(parsed.stop_reason.as_deref(), Some("end_turn"));
        assert!(parsed.total_cost_usd.is_some());
    }

    #[test]
    fn claude_result_ignores_unknown_fields() {
        let parsed: ClaudeResult = serde_json::from_str(
            r#"{
                "result": "kept",
                "session_id": "sess-extra",
                "stop_reason": "end_turn",
                "total_cost_usd": 0.0001,
                "future_claude_field": {
                    "nested": true
                }
            }"#,
        )
        .expect("unknown fields from future Claude JSON should be tolerated");

        assert_eq!(parsed.result.as_deref(), Some("kept"));
        assert_eq!(parsed.session_id.as_deref(), Some("sess-extra"));
        assert_eq!(parsed.stop_reason.as_deref(), Some("end_turn"));
        assert!(parsed.total_cost_usd.is_some());
    }

    #[test]
    fn claude_result_accepts_total_cost_usd_as_number_or_numeric_string() {
        let numeric_cost: ClaudeResult = serde_json::from_str(
            r#"{
                "result": "number",
                "total_cost_usd": 0.0123
            }"#,
        )
        .expect("numeric total_cost_usd should parse");

        let string_cost: ClaudeResult = serde_json::from_str(
            r#"{
                "result": "string",
                "total_cost_usd": "0.0123"
            }"#,
        )
        .expect("numeric string total_cost_usd should parse");

        assert!(numeric_cost.total_cost_usd.is_some());
        assert!(string_cost.total_cost_usd.is_some());
    }

    #[test]
    fn parse_result_returns_none_for_malformed_json() {
        assert!(parse_result("not json at all").is_none());
    }

    #[test]
    fn render_task_normalizes_claude_result_session_id_and_stop_reason_into_the_rendered_block() {
        let raw = r#"{
            "result": "The answer is 4.",
            "session_id": "sess-abc",
            "stop_reason": "end_turn",
            "total_cost_usd": "0.0123"
        }"#;
        let args = argv(&[
            "--read",
            "--model",
            "opus",
            "--cwd",
            "/tmp/work",
            "What",
            "is",
            "2+2?",
        ]);
        let opts = parse_task_args(&args).expect("task args should parse");

        let rendered = render_task(raw, &opts);

        assert_eq!(
            rendered,
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
        assert!(!rendered.contains("undefined"));
    }

    #[test]
    fn render_task_falls_back_to_raw_text_when_json_parsing_fails() {
        let args = argv(&["--model", "opus", "--cwd", "/tmp/work", "do", "it"]);
        let opts = parse_task_args(&args).expect("task args should parse");

        assert_eq!(render_task("not json at all", &opts), "not json at all\n");
    }

    #[test]
    fn render_review_normalizes_result_and_session_id_from_claude_json() {
        let raw = r#"{
            "result": "Looks fine overall.",
            "session_id": "sess-xyz",
            "stop_reason": "end_turn"
        }"#;
        let args = argv(&["--scope", "working-tree", "--model", "opus"]);
        let opts = parse_review_args(&args).expect("review args should parse");

        let rendered = render_review(raw, &opts);

        assert_eq!(
            rendered,
            concat!(
                "=== claude review ===\n",
                "scope:   working tree\n",
                "model:   opus\n",
                "session: sess-xyz\n",
                "\n",
                "Looks fine overall.\n",
            )
        );
        assert!(!rendered.contains("undefined"));
    }
}
