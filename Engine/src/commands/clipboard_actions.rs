//! Quick-actions: small, instant transformations applied to clipboard
//! history entries.
//!
//! Each action takes the entry's text, runs a pure-function transform,
//! and returns the result. The frontend decides what to do with the
//! result (copy to clipboard, replace the entry, show a toast).
//!
//! Why a single dispatch command rather than one command per operation:
//!   - Keeps the Tauri capability surface tiny (one allow-rule covers
//!     every action, not a dozen)
//!   - Makes adding a new action a 5-line change here + a 1-line UI
//!     change — friction is what kills feature shipping
//!   - The action is purely text-in, text-out: no path validation, no
//!     filesystem, no network. Hardening surface = zero.

use base64::Engine;
use serde::{Deserialize, Serialize};

/// All supported quick-action operations. Matches up 1:1 with the
/// frontend's action menu entries. New operations get a variant here
/// + a match arm in `run`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardActionKind {
    FormatJson,
    MinifyJson,
    DecodeBase64,
    EncodeBase64,
    UrlDecode,
    UrlEncode,
    Uppercase,
    Lowercase,
    TitleCase,
    Trim,
    LineSort,
    LineDedupe,
    LineReverse,
    ReverseString,
}

#[tauri::command]
pub fn run_clipboard_action(
    action: ClipboardActionKind,
    input: String,
) -> Result<String, String> {
    match action {
        ClipboardActionKind::FormatJson => format_json(&input),
        ClipboardActionKind::MinifyJson => minify_json(&input),
        ClipboardActionKind::DecodeBase64 => decode_base64(&input),
        ClipboardActionKind::EncodeBase64 => Ok(encode_base64(&input)),
        ClipboardActionKind::UrlDecode => url_decode(&input),
        ClipboardActionKind::UrlEncode => Ok(url_encode(&input)),
        ClipboardActionKind::Uppercase => Ok(input.to_uppercase()),
        ClipboardActionKind::Lowercase => Ok(input.to_lowercase()),
        ClipboardActionKind::TitleCase => Ok(title_case(&input)),
        ClipboardActionKind::Trim => Ok(input.trim().to_string()),
        ClipboardActionKind::LineSort => Ok(line_sort(&input)),
        ClipboardActionKind::LineDedupe => Ok(line_dedupe(&input)),
        ClipboardActionKind::LineReverse => Ok(line_reverse(&input)),
        ClipboardActionKind::ReverseString => Ok(input.chars().rev().collect()),
    }
}

fn format_json(input: &str) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_str(input)
        .map_err(|e| format!("Not valid JSON: {e}"))?;
    serde_json::to_string_pretty(&value).map_err(|e| format!("Format failed: {e}"))
}

fn minify_json(input: &str) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_str(input)
        .map_err(|e| format!("Not valid JSON: {e}"))?;
    serde_json::to_string(&value).map_err(|e| format!("Minify failed: {e}"))
}

fn decode_base64(input: &str) -> Result<String, String> {
    // Strip whitespace and any data-URL prefix — clipboard base64 often
    // arrives wrapped in `data:image/png;base64,...` from "copy as data URL".
    let cleaned = input
        .trim()
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(',').map(|(_, body)| body))
        .unwrap_or(input.trim());
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cleaned.replace(['\n', '\r', ' '], "").as_bytes())
        .map_err(|e| format!("Not valid base64: {e}"))?;
    String::from_utf8(bytes).map_err(|e| {
        format!(
            "Decoded bytes are not valid UTF-8 — looks like binary data: {e}"
        )
    })
}

fn encode_base64(input: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(input.as_bytes())
}

fn url_decode(input: &str) -> Result<String, String> {
    urlencoding::decode(input)
        .map(|cow| cow.into_owned())
        .map_err(|e| format!("URL decode failed: {e}"))
}

fn url_encode(input: &str) -> String {
    urlencoding::encode(input).into_owned()
}

/// Crude title case: uppercase the first letter of every word. Good
/// enough for the "I want this in title case" everyday case; users who
/// need real linguistic title-casing should use a dedicated tool.
fn title_case(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut at_word_start = true;
    for ch in input.chars() {
        if ch.is_whitespace() || ch == '-' || ch == '_' {
            at_word_start = true;
            out.push(ch);
        } else if at_word_start {
            out.extend(ch.to_uppercase());
            at_word_start = false;
        } else {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

fn line_sort(input: &str) -> String {
    let mut lines: Vec<&str> = input.lines().collect();
    lines.sort_unstable();
    lines.join("\n")
}

fn line_dedupe(input: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    input
        .lines()
        .filter(|line| seen.insert(*line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_reverse(input: &str) -> String {
    input.lines().rev().collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_pretty_prints_json() {
        let out =
            run_clipboard_action(ClipboardActionKind::FormatJson, r#"{"a":1,"b":[2,3]}"#.into())
                .unwrap();
        assert!(out.contains("\"a\": 1"));
        assert!(out.contains("\n"));
    }

    #[test]
    fn minify_strips_whitespace() {
        let out =
            run_clipboard_action(ClipboardActionKind::MinifyJson, "{\n  \"a\": 1\n}".into())
                .unwrap();
        assert_eq!(out, r#"{"a":1}"#);
    }

    #[test]
    fn base64_roundtrip() {
        let encoded =
            run_clipboard_action(ClipboardActionKind::EncodeBase64, "hello world".into()).unwrap();
        let decoded = run_clipboard_action(ClipboardActionKind::DecodeBase64, encoded).unwrap();
        assert_eq!(decoded, "hello world");
    }

    #[test]
    fn url_encode_decode_special_chars() {
        let original = "hello world!&foo=bar";
        let encoded =
            run_clipboard_action(ClipboardActionKind::UrlEncode, original.into()).unwrap();
        assert!(encoded.contains("%20") || encoded.contains("+"));
        let decoded = run_clipboard_action(ClipboardActionKind::UrlDecode, encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn case_transforms() {
        assert_eq!(
            run_clipboard_action(ClipboardActionKind::Uppercase, "hello".into()).unwrap(),
            "HELLO"
        );
        assert_eq!(
            run_clipboard_action(ClipboardActionKind::Lowercase, "HELLO".into()).unwrap(),
            "hello"
        );
        assert_eq!(
            run_clipboard_action(ClipboardActionKind::TitleCase, "hello world".into()).unwrap(),
            "Hello World"
        );
    }

    #[test]
    fn line_operations() {
        let input = "banana\napple\ncherry\napple";
        assert_eq!(
            run_clipboard_action(ClipboardActionKind::LineSort, input.into()).unwrap(),
            "apple\napple\nbanana\ncherry"
        );
        assert_eq!(
            run_clipboard_action(ClipboardActionKind::LineDedupe, input.into()).unwrap(),
            "banana\napple\ncherry"
        );
        assert_eq!(
            run_clipboard_action(ClipboardActionKind::LineReverse, input.into()).unwrap(),
            "apple\ncherry\napple\nbanana"
        );
    }

    #[test]
    fn json_format_rejects_invalid_input() {
        let result = run_clipboard_action(
            ClipboardActionKind::FormatJson,
            "not really json".into(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn base64_decode_handles_data_url_prefix() {
        // "Hello" base64 = SGVsbG8=
        let result = run_clipboard_action(
            ClipboardActionKind::DecodeBase64,
            "data:text/plain;base64,SGVsbG8=".into(),
        );
        assert_eq!(result.unwrap(), "Hello");
    }
}
