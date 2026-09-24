//! Snippet templates — KeepItLocal's headline pro-tier feature.
//!
//! A snippet is a stored body of text keyed by a short trigger. The user
//! types the trigger into the clipboard overlay (or picks it from the
//! Snippets tool screen) and we expand the template into the previous
//! window via the same paste-injection path the clipboard overlay uses.
//!
//! Templates support a small set of variables — `{{date}}`, `{{time}}`,
//! `{{clipboard}}`, `{{cursor}}` — that get resolved at expansion time.
//! This is what makes snippets feel like more than a glorified clipboard
//! pin: an email signature template can auto-stamp today's date, a PR
//! template can pull in whatever you just copied as the summary, etc.
//!
//! Storage: a single redb key `"snippets_v1"` holds the full Vec. Each
//! value is DPAPI-encrypted at rest by the local_db layer, so user
//! templates (which may include sensitive content like API key snippets)
//! get the same protection as clipboard history.

use super::local_db;
use super::preferences;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};
use time::OffsetDateTime;

const SNIPPETS_KEY: &str = "snippets_v1";

/// One stored snippet. Trigger is the user-typed shortcut (no leading
/// slash — UI may or may not show one), label is the human-readable
/// title shown in lists, template is the body that gets expanded.
///
/// `use_count` powers "most used first" sort in the picker — like
/// frecency for snippets, but simpler since the variance is small.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    pub id: u64,
    pub trigger: String,
    pub label: String,
    pub template: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub use_count: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnippetsFile {
    #[serde(default)]
    snippets: Vec<Snippet>,
    #[serde(default)]
    next_id: u64,
}

/// Load the database file path used for snippet storage. Lives next to
/// preferences (shared redb) so encryption + atomic writes apply.
fn snippets_db_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Cannot resolve app data dir: {e}"))?;
    // Reuse preferences dir so the user's "Local Storage" panel doesn't
    // sprout yet another path. The redb file is shared; only the key
    // namespace differs.
    let preferences_dir = preferences::preferences_dir(app)?;
    let _ = dir;
    Ok(local_db::database_path_for_dir(&preferences_dir))
}

fn load_file(app: &AppHandle) -> Result<SnippetsFile, String> {
    let path = snippets_db_path(app)?;
    let stored: Option<SnippetsFile> = local_db::read_json(&path, SNIPPETS_KEY)?;
    Ok(stored.unwrap_or_default())
}

fn save_file(app: &AppHandle, file: &SnippetsFile) -> Result<(), String> {
    let path = snippets_db_path(app)?;
    local_db::write_json(&path, SNIPPETS_KEY, file)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Normalize the trigger string to the form we match against and store.
/// Strips whitespace, leading slash, and lowercases. Lowercase storage
/// means "Sig" and "sig" don't collide; the UI can re-case for display.
fn normalize_trigger(input: &str) -> String {
    input
        .trim()
        .trim_start_matches('/')
        .trim()
        .to_lowercase()
}

// ─── Tauri commands ──────────────────────────────────────────────────────

#[tauri::command]
pub fn list_snippets(app: AppHandle) -> Result<Vec<Snippet>, String> {
    let file = load_file(&app)?;
    // Sort: most-used first, ties broken by recency. Gives "the
    // snippet you reach for daily" top placement.
    let mut snippets = file.snippets;
    snippets.sort_by(|a, b| {
        b.use_count
            .cmp(&a.use_count)
            .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
    });
    Ok(snippets)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnippetInput {
    pub trigger: String,
    pub label: String,
    pub template: String,
}

/// Create a new snippet. Triggers must be unique (case-insensitive). If
/// the trigger already exists we return an error rather than silently
/// overwriting — UX is "edit the existing one" not "lose the old body".
#[tauri::command]
pub fn create_snippet(app: AppHandle, input: SnippetInput) -> Result<Snippet, String> {
    let trigger = normalize_trigger(&input.trigger);
    if trigger.is_empty() {
        return Err("Trigger cannot be empty".to_string());
    }
    if input.label.trim().is_empty() {
        return Err("Label cannot be empty".to_string());
    }
    if input.template.is_empty() {
        return Err("Template cannot be empty".to_string());
    }

    let mut file = load_file(&app)?;
    if file
        .snippets
        .iter()
        .any(|s| s.trigger.eq_ignore_ascii_case(&trigger))
    {
        return Err(format!("A snippet with trigger \"{trigger}\" already exists"));
    }

    let id = file.next_id.max(1);
    file.next_id = id + 1;
    let now = now_ms();
    let snippet = Snippet {
        id,
        trigger,
        label: input.label.trim().to_string(),
        template: input.template,
        created_at_ms: now,
        updated_at_ms: now,
        use_count: 0,
    };
    file.snippets.push(snippet.clone());
    save_file(&app, &file)?;
    Ok(snippet)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnippetUpdate {
    pub id: u64,
    pub trigger: String,
    pub label: String,
    pub template: String,
}

#[tauri::command]
pub fn update_snippet(app: AppHandle, update: SnippetUpdate) -> Result<Snippet, String> {
    let trigger = normalize_trigger(&update.trigger);
    if trigger.is_empty() {
        return Err("Trigger cannot be empty".to_string());
    }
    if update.label.trim().is_empty() {
        return Err("Label cannot be empty".to_string());
    }
    if update.template.is_empty() {
        return Err("Template cannot be empty".to_string());
    }

    let mut file = load_file(&app)?;

    // Trigger uniqueness check skips the snippet we're updating —
    // otherwise "save without changing trigger" would always fail.
    let trigger_collides = file
        .snippets
        .iter()
        .any(|s| s.id != update.id && s.trigger.eq_ignore_ascii_case(&trigger));
    if trigger_collides {
        return Err(format!("A snippet with trigger \"{trigger}\" already exists"));
    }

    let snippet = file
        .snippets
        .iter_mut()
        .find(|s| s.id == update.id)
        .ok_or_else(|| "Snippet not found".to_string())?;
    snippet.trigger = trigger;
    snippet.label = update.label.trim().to_string();
    snippet.template = update.template;
    snippet.updated_at_ms = now_ms();
    let result = snippet.clone();

    save_file(&app, &file)?;
    Ok(result)
}

#[tauri::command]
pub fn delete_snippet(app: AppHandle, id: u64) -> Result<(), String> {
    let mut file = load_file(&app)?;
    let before = file.snippets.len();
    file.snippets.retain(|s| s.id != id);
    if file.snippets.len() == before {
        return Err("Snippet not found".to_string());
    }
    save_file(&app, &file)?;
    Ok(())
}

/// Increment use_count after a successful expansion. Lightweight write,
/// only called when the user actually paste-expands a snippet — gives us
/// the "most used first" ordering without any heavy analytics.
#[tauri::command]
pub fn record_snippet_use(app: AppHandle, id: u64) -> Result<(), String> {
    let mut file = load_file(&app)?;
    if let Some(snippet) = file.snippets.iter_mut().find(|s| s.id == id) {
        snippet.use_count = snippet.use_count.saturating_add(1);
        save_file(&app, &file)?;
    }
    Ok(())
}

// ─── Template expansion ──────────────────────────────────────────────────

/// Resolve every `{{variable}}` reference in `template` against the
/// current environment and return the final string ready to paste.
///
/// Supported variables (case-sensitive):
///   {{date}}              - YYYY-MM-DD
///   {{date:FORMAT}}       - custom format using YYYY/MM/DD/HH/mm/ss tokens
///   {{time}}              - HH:mm
///   {{time:FORMAT}}       - same tokens as date
///   {{datetime}}          - YYYY-MM-DD HH:mm
///   {{weekday}}           - full weekday name, e.g. Monday
///   {{clipboard}}         - the caller-supplied clipboard text
///   {{cursor}}            - rendered as `|` so the user sees where the
///                            caret should go after pasting (we cannot
///                            actually position the caret post-paste
///                            without a real keyboard hook).
///
/// Unknown variables are left as-is so users see what they typo'd
/// instead of silent removal.
#[tauri::command]
pub fn preview_snippet_expansion(
    template: String,
    clipboard_text: Option<String>,
) -> Result<String, String> {
    Ok(expand_template(&template, clipboard_text.as_deref()))
}

pub fn expand_template(template: &str, clipboard_text: Option<&str>) -> String {
    // Two-pass scan: find every {{...}} then replace. Done with a manual
    // walker rather than regex so the implementation is dependency-free
    // and easy to extend.
    let bytes = template.as_bytes();
    let mut out = String::with_capacity(template.len() + 32);
    let mut i = 0;
    let now = OffsetDateTime::now_local()
        .unwrap_or_else(|_| OffsetDateTime::now_utc());

    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            // Locate the closing }}
            let mut j = i + 2;
            while j + 1 < bytes.len() {
                if bytes[j] == b'}' && bytes[j + 1] == b'}' {
                    break;
                }
                j += 1;
            }
            if j + 1 < bytes.len() && bytes[j] == b'}' && bytes[j + 1] == b'}' {
                let inner = std::str::from_utf8(&bytes[i + 2..j])
                    .unwrap_or("")
                    .trim();
                if let Some(replacement) = resolve_variable(inner, clipboard_text, &now) {
                    out.push_str(&replacement);
                } else {
                    // Unknown variable — pass through verbatim so the
                    // user sees the typo and can fix it.
                    out.push_str(&template[i..=j + 1]);
                }
                i = j + 2;
                continue;
            }
        }
        out.push(template[i..].chars().next().unwrap_or(' '));
        i += template[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }
    out
}

fn resolve_variable(
    name: &str,
    clipboard_text: Option<&str>,
    now: &OffsetDateTime,
) -> Option<String> {
    // Split on ':' for format-bearing variables ({{date:YYYY-MM-DD}})
    let (head, fmt) = match name.split_once(':') {
        Some((h, f)) => (h, Some(f.trim_matches('"').trim())),
        None => (name, None),
    };
    match head {
        "date" => Some(format_datetime(now, fmt.unwrap_or("YYYY-MM-DD"))),
        "time" => Some(format_datetime(now, fmt.unwrap_or("HH:mm"))),
        "datetime" => Some(format_datetime(now, fmt.unwrap_or("YYYY-MM-DD HH:mm"))),
        "weekday" => Some(weekday_name(now.weekday()).to_string()),
        "clipboard" => Some(clipboard_text.unwrap_or("").to_string()),
        // v1: render cursor as a visible pipe so users see where the
        // caret should be moved after paste. Full caret positioning
        // needs a global hook + post-paste keystroke injection which
        // is a Pro+ feature for a later release.
        "cursor" => Some("|".to_string()),
        _ => None,
    }
}

/// Full English name of a weekday. `time::Weekday` has no `Display` impl, and
/// we want a stable English name regardless of OS locale.
fn weekday_name(day: time::Weekday) -> &'static str {
    match day {
        time::Weekday::Monday => "Monday",
        time::Weekday::Tuesday => "Tuesday",
        time::Weekday::Wednesday => "Wednesday",
        time::Weekday::Thursday => "Thursday",
        time::Weekday::Friday => "Friday",
        time::Weekday::Saturday => "Saturday",
        time::Weekday::Sunday => "Sunday",
    }
}

/// Format `dt` with a simple token-replacement formatter. Supported
/// tokens (case-sensitive, all multi-character to avoid matching inside
/// literal English words):
///   YYYY  4-digit year
///   YY    2-digit year
///   MM    2-digit month (01-12)
///   DD    2-digit day
///   HH    2-digit hour (00-23)
///   mm    2-digit minute
///   ss    2-digit second
///
/// Single-character tokens (M, D, H, m, s) are intentionally NOT
/// supported because they would match inside arbitrary text — e.g. the
/// `s` in literal "is" would get expanded to "0" inside the format
/// string "Today is YYYY". Users who want unpadded values can hand-edit
/// the leading zero off after expansion.
///
/// We process LONGER tokens first (YYYY before YY) so we don't
/// half-match inside a longer token. Implemented as a one-pass string
/// scan with a small token table.
fn format_datetime(dt: &OffsetDateTime, fmt: &str) -> String {
    // Order matters: longer tokens MUST come first so `YYYY` doesn't get
    // partially consumed by `YY`.
    let tokens: &[(&str, String)] = &[
        ("YYYY", format!("{:04}", dt.year())),
        ("YY", format!("{:02}", dt.year() % 100)),
        ("MM", format!("{:02}", u8::from(dt.month()))),
        ("DD", format!("{:02}", dt.day())),
        ("HH", format!("{:02}", dt.hour())),
        ("mm", format!("{:02}", dt.minute())),
        ("ss", format!("{:02}", dt.second())),
    ];

    let mut out = String::with_capacity(fmt.len() + 4);
    let bytes = fmt.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let mut matched = false;
        for (token, value) in tokens.iter() {
            let tlen = token.len();
            if i + tlen <= bytes.len() && &fmt[i..i + tlen] == *token {
                out.push_str(value);
                i += tlen;
                matched = true;
                break;
            }
        }
        if !matched {
            // Pass through literal characters (separators, text). We
            // step by char-byte-len so multi-byte chars in custom
            // formats don't corrupt.
            let ch = fmt[i..].chars().next().unwrap_or(' ');
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// Like `expand_template`, but additionally resolves `{{name}}` and the
/// user's custom variables (the frontend resolves these client-side for
/// the overlay; the auto-expand hook has no frontend, so it resolves them
/// here from data the frontend pushed). Built-ins win; then name; then
/// user vars. Unknown `{{...}}` are left verbatim. Variable name match is
/// whitespace-tolerant inside the braces, mirroring the client.
pub fn expand_template_full(
    template: &str,
    clipboard_text: Option<&str>,
    name: Option<&str>,
    user_vars: &std::collections::HashMap<String, String>,
) -> String {
    // First pass: built-in variables ({{date}}, {{clipboard}}, {{cursor}}, …).
    let mut out = expand_template(template, clipboard_text);
    // Second pass: {{name}}.
    if let Some(name) = name {
        out = replace_var(&out, "name", name);
    }
    // Third pass: user-defined variables.
    for (key, value) in user_vars {
        let k = key.trim();
        if k.is_empty() {
            continue;
        }
        out = replace_var(&out, k, value);
    }
    out
}

/// Replace every `{{ key }}` (whitespace-tolerant) with `value`. `key` is
/// treated literally (no regex). Done as a manual scan to stay dependency-free.
fn replace_var(input: &str, key: &str, value: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            // find closing }}
            if let Some(close) = input[i + 2..].find("}}") {
                let inner = input[i + 2..i + 2 + close].trim();
                if inner == key {
                    out.push_str(value);
                    i = i + 2 + close + 2;
                    continue;
                }
            }
        }
        let ch = input[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_slash_and_lowercases() {
        assert_eq!(normalize_trigger("/Sig"), "sig");
        assert_eq!(normalize_trigger("  /PR  "), "pr");
        assert_eq!(normalize_trigger("PLAIN"), "plain");
    }

    #[test]
    fn expand_passes_through_static_text() {
        assert_eq!(expand_template("hello world", None), "hello world");
    }

    #[test]
    fn expand_resolves_clipboard_variable() {
        let result = expand_template("before {{clipboard}} after", Some("XYZ"));
        assert_eq!(result, "before XYZ after");
    }

    #[test]
    fn expand_renders_cursor_marker() {
        let result = expand_template("a {{cursor}} b", None);
        assert_eq!(result, "a | b");
    }

    #[test]
    fn expand_resolves_weekday_variable() {
        let result = expand_template("Notes for {{weekday}}", None);
        assert!(result.starts_with("Notes for "));
        let names = [
            "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday",
        ];
        assert!(
            names.iter().any(|name| result.ends_with(name)),
            "expected a weekday name, got {result:?}"
        );
    }

    #[test]
    fn expand_leaves_unknown_variable_intact() {
        // Typo `{{clipoard}}` should be visible so the user sees the bug.
        let result = expand_template("hi {{clipoard}}!", Some("x"));
        assert_eq!(result, "hi {{clipoard}}!");
    }

    #[test]
    fn format_datetime_yyyy_mm_dd() {
        // Pick a fixed instant: 2026-05-14 09:07:03 UTC
        let dt = OffsetDateTime::UNIX_EPOCH
            + time::Duration::seconds(1747214823);
        let formatted = format_datetime(&dt, "YYYY-MM-DD");
        // We know the year + month structure; check the YYYY part is
        // 4-digit and the dash separator is preserved.
        assert!(formatted.len() == 10);
        assert!(formatted.contains('-'));
    }

    #[test]
    fn format_datetime_preserves_literals() {
        let dt = OffsetDateTime::UNIX_EPOCH;
        let formatted = format_datetime(&dt, "Today is YYYY!");
        assert!(formatted.starts_with("Today is "));
        assert!(formatted.ends_with('!'));
    }

    #[test]
    fn expand_date_with_custom_format() {
        let result = expand_template("at {{date:YYYY/MM/DD}}", None);
        // Format result should have YYYY/MM/DD shape (4 + 1 + 2 + 1 + 2 = 10 chars)
        assert!(result.starts_with("at "));
        assert!(result.matches('/').count() == 2);
    }

    #[test]
    fn expand_full_resolves_name_and_user_vars() {
        let mut vars = std::collections::HashMap::new();
        vars.insert("surname".to_string(), "Smith".to_string());
        let out = expand_template_full(
            "Hi, {{name}} {{surname}} on {{weekday}}",
            None,
            Some("Ada"),
            &vars,
        );
        assert!(out.starts_with("Hi, Ada Smith on "));
    }

    #[test]
    fn expand_full_leaves_unknown_vars() {
        let vars = std::collections::HashMap::new();
        let out = expand_template_full("x {{nope}}", None, Some("Ada"), &vars);
        assert_eq!(out, "x {{nope}}");
    }
}
