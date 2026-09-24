use serde::{Deserialize, Serialize};
use sqlformat::{format, FormatOptions, Indent, QueryParams};
use sqlparser::ast::Statement;
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

#[derive(Deserialize)]
pub struct SqlFormatOptions {
    pub input: String,
    pub indent: u8, // 2 or 4
    pub uppercase: bool,
    pub lines_between_queries: u8,
}

#[derive(Serialize)]
pub struct SqlFormatResult {
    pub formatted: String,
    pub error: Option<String>,
}

#[tauri::command]
pub fn format_sql(options: SqlFormatOptions) -> SqlFormatResult {
    if options.input.trim().is_empty() {
        return SqlFormatResult {
            formatted: String::new(),
            error: None,
        };
    }

    let opts = FormatOptions {
        indent: Indent::Spaces(options.indent.clamp(2, 8)),
        uppercase: options.uppercase,
        lines_between_queries: options.lines_between_queries.clamp(1, 5),
    };

    let formatted = format(&options.input, &QueryParams::None, opts);
    SqlFormatResult {
        formatted,
        error: None,
    }
}

// ───────────────────────── SB-4 (2026-05-29): real SQL lint ─────────────
//
// Replaces the old "Explain" panel, which was regex string-matching
// dressed up as a query planner (counted the substring "join", grepped
// for "select *"). That was a gimmick: it implied a database EXPLAIN
// plan while doing none of it, and the regex was fragile.
//
// This is honest static analysis backed by a REAL parser (sqlparser
// AST): syntax validation, accurate statement classification, and the
// high-value safety check the regex never did reliably — UPDATE/DELETE
// with no WHERE clause (affects every row). It is NOT a database plan
// (we have no live DB/schema), and the UI labels it "Lint" accordingly.

#[derive(Deserialize)]
pub struct SqlAnalyzeOptions {
    pub input: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlStat {
    pub label: String,
    pub value: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlFinding {
    /// "high" | "medium" | "low" | "info".
    pub severity: String,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlAnalyzeResult {
    pub statement_count: usize,
    pub valid: bool,
    pub stats: Vec<SqlStat>,
    pub findings: Vec<SqlFinding>,
    pub parse_error: Option<String>,
}

fn sql_stat(label: &str, value: impl Into<String>) -> SqlStat {
    SqlStat {
        label: label.to_string(),
        value: value.into(),
    }
}

fn sql_finding(severity: &str, message: impl Into<String>) -> SqlFinding {
    SqlFinding {
        severity: severity.to_string(),
        message: message.into(),
    }
}

/// Human-readable statement kind. `Statement` is `#[non_exhaustive]`, so
/// the wildcard arm is mandatory and keeps us forward-compatible.
fn statement_kind(stmt: &Statement) -> &'static str {
    match stmt {
        Statement::Query(_) => "SELECT",
        Statement::Insert(_) => "INSERT",
        Statement::Update { .. } => "UPDATE",
        Statement::Delete(_) => "DELETE",
        Statement::CreateTable(_) => "CREATE TABLE",
        Statement::CreateView { .. } => "CREATE VIEW",
        Statement::CreateIndex(_) => "CREATE INDEX",
        Statement::AlterTable { .. } => "ALTER TABLE",
        Statement::Drop { .. } => "DROP",
        Statement::Truncate { .. } => "TRUNCATE",
        _ => "OTHER",
    }
}

#[tauri::command]
pub fn analyze_sql(options: SqlAnalyzeOptions) -> SqlAnalyzeResult {
    let sql = options.input.trim();
    if sql.is_empty() {
        return SqlAnalyzeResult {
            statement_count: 0,
            valid: true,
            stats: Vec::new(),
            findings: Vec::new(),
            parse_error: None,
        };
    }

    let dialect = GenericDialect {};
    match Parser::parse_sql(&dialect, sql) {
        // Real syntax validation — something no regex pass could ever do.
        Err(err) => SqlAnalyzeResult {
            statement_count: 0,
            valid: false,
            stats: vec![sql_stat("Status", "Invalid SQL")],
            findings: vec![sql_finding("high", format!("Syntax error: {err}"))],
            parse_error: Some(err.to_string()),
        },
        Ok(statements) => {
            let mut findings = Vec::new();
            let mut kind_counts: std::collections::BTreeMap<&'static str, usize> =
                std::collections::BTreeMap::new();
            let mut writes_without_where = 0usize;

            for stmt in &statements {
                let kind = statement_kind(stmt);
                *kind_counts.entry(kind).or_insert(0) += 1;

                // The headline safety check: an UPDATE or DELETE with no
                // WHERE clause rewrites/erases EVERY row. Accurate from the
                // AST (selection is None), not guessed from text.
                let no_where = match stmt {
                    Statement::Update { selection, .. } => selection.is_none(),
                    Statement::Delete(delete) => delete.selection.is_none(),
                    _ => false,
                };
                if no_where {
                    writes_without_where += 1;
                    findings.push(sql_finding(
                        "high",
                        format!(
                            "{kind} has no WHERE clause — it affects EVERY row in the table. Add a WHERE filter unless that's truly intended."
                        ),
                    ));
                }
            }

            let kinds_summary = kind_counts
                .iter()
                .map(|(kind, count)| format!("{count} {kind}"))
                .collect::<Vec<_>>()
                .join(", ");

            let mut stats = vec![
                sql_stat("Statements", statements.len().to_string()),
                sql_stat("Valid SQL", "Yes"),
            ];
            if !kinds_summary.is_empty() {
                stats.push(sql_stat("Types", kinds_summary));
            }
            if writes_without_where > 0 {
                stats.push(sql_stat(
                    "Unfiltered writes",
                    writes_without_where.to_string(),
                ));
            }

            if findings.is_empty() {
                findings.push(sql_finding(
                    "info",
                    "Parsed cleanly. No high-risk patterns found in this static pass (this is a lint, not a database query plan).",
                ));
            }

            SqlAnalyzeResult {
                statement_count: statements.len(),
                valid: true,
                stats,
                findings,
                parse_error: None,
            }
        }
    }
}
