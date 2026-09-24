//! Value masking — turn a raw matched secret into a preview that shows
//! enough for a human to identify the item without exposing its content.
//!
//! Ported from KeepItLocal Memory (`kip-memory-core/src/firewall/detect/mask.rs`).
//! The detector half of that module was NOT ported: Workspace already has a
//! tiered detector of the same lineage in `super::super::sensitive_scan`, and
//! two tiered detectors in one codebase is exactly the duplication this
//! codebase keeps paying for. Only the masking half is new capability.
//!
//! The general rule: keep the first 2 and last 2 characters, replace the
//! middle with `***`. Some kinds get specialized treatment (email keeps the
//! domain; cards and SSNs keep the last 4 for the standard "ending in ⋯");
//! private keys and opaque bearer tokens are fully obscured because there is
//! no useful sub-string to surface.

pub fn mask_value(kind: &str, raw: &str) -> String {
    match kind {
        // ─── Identity & contact ────────────────────────────────────
        "email" => mask_email(raw),
        "ipv4" => mask_ipv4(raw),

        // ─── Keep last 4 (the "ending in ⋯" convention) ────────────
        "credit_card" | "us_ssn" => mask_last_four(raw),

        // ─── High-tier secrets: prefix + last 4 only ──────────────
        "aws_access_key"
        | "github_token"
        | "github_fine_grained_pat"
        | "slack_token"
        | "stripe_secret_key"
        | "google_api_key"
        | "twilio_account_sid"
        | "sendgrid_api_key"
        | "openai_api_key"
        | "npm_token"
        | "database_url_with_password"
        | "ethereum_private_key" => mask_prefix_tail(raw, 4, 4),

        // ─── Fully obscured ────────────────────────────────────────
        // `bearer_token` and `generic_credential_assignment` are Workspace
        // kinds with no Memory counterpart; both are opaque secrets whose
        // prefix identifies nothing, so they get no preview at all.
        "jwt_token" | "slack_webhook" | "bearer_token" | "generic_credential_assignment" => {
            "[REDACTED]".into()
        }
        "private_key_pem" | "pgp_private_key" | "bitcoin_wif_private_key" => "[PRIVATE KEY]".into(),

        _ => mask_prefix_tail(raw, 2, 2),
    }
}

/// Coarsen a quasi-identifier to a less-precise value for the `Generalize`
/// policy action (k-anonymity-style). Unlike [`mask_value`], the result is a
/// *real, disclosed* value — just lower-resolution — so a model gets the shape
/// of a fact without the exact secret.
///
/// Returns `None` for any kind without a defined hierarchy; the firewall then
/// falls back to full redaction (the safe default). Kinds covered today:
///
///   * `dob` / `date`  → year only            (`1987-03-12` → `1987`)
///   * `us_zip`        → 3-digit prefix        (`90210-1234` → `902xx`)
///   * `phone`         → leading country/area  (`+1 415-555-0199` → `+1 415…`)
///   * `number`/`amount` → 2-significant-figure bucket (`142000` → `~140,000`)
///
/// Gated by kind so a secret is never silently coarsened: a PIN/SSN/email has
/// no hierarchy here and therefore redacts.
pub fn generalize_value(kind: &str, raw: &str) -> Option<String> {
    match kind {
        "dob" | "date" => generalize_date(raw),
        "us_zip" => generalize_zip(raw),
        "phone" => generalize_phone(raw),
        "number" | "amount" => generalize_number(raw),
        _ => None,
    }
}

/// First 4-digit run in the string is the year; everything else is dropped.
/// Works for `1987-03-12`, `12/03/1987`, `1987.03.12` alike.
fn generalize_date(raw: &str) -> Option<String> {
    let mut run = String::new();
    for c in raw.chars() {
        if c.is_ascii_digit() {
            run.push(c);
            if run.len() == 4 {
                return Some(run);
            }
        } else {
            run.clear();
        }
    }
    None
}

/// ZIP+4 → first 3 digits + `xx` (the "sectional center" region, ~k≥20k people).
fn generalize_zip(raw: &str) -> Option<String> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).take(3).collect();
    (digits.len() == 3).then(|| format!("{digits}xx"))
}

/// Keep the leading country/area prefix, drop the subscriber line.
/// `+1 415-555-0199` → `+1 415…`.
fn generalize_phone(raw: &str) -> Option<String> {
    let plus = raw.trim_start().starts_with('+');
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 5 {
        return None; // too short to coarsen meaningfully — let it redact
    }
    // 1 country digit (if `+`) + 3 area digits is enough to place a number
    // without identifying the line.
    let keep = if plus { 4 } else { 3 };
    let head = &digits[..keep.min(digits.len())];
    let shown = if plus {
        format!("+{} {}…", &head[..1], &head[1..])
    } else {
        format!("{head}…")
    };
    Some(shown)
}

/// Round a number to two significant figures and tag it `~` (e.g. `142000` →
/// `~140,000`). A coarse bucket that keeps the band while dropping the exact
/// figure. Numbers < 100 are already in a 2-sig-fig bucket and pass through
/// tagged.
fn generalize_number(raw: &str) -> Option<String> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    let n: u128 = digits.parse().ok()?;
    if n < 100 {
        return Some(format!("~{n}"));
    }
    // 2 sig figs: drop to a magnitude that leaves two leading digits.
    let mag = 10u128.pow((digits.len() as u32).saturating_sub(2));
    let rounded = ((n as f64 / mag as f64).round() as u128) * mag;
    Some(format!("~{}", group_thousands(rounded)))
}

/// `142000` → `142,000`. Walk the digits right-to-left, comma every third.
fn group_thousands(n: u128) -> String {
    let digits: Vec<char> = n.to_string().chars().collect();
    let mut rev = String::new();
    for (i, c) in digits.iter().rev().enumerate() {
        if i != 0 && i % 3 == 0 {
            rev.push(',');
        }
        rev.push(*c);
    }
    rev.chars().rev().collect()
}

fn mask_email(raw: &str) -> String {
    if let Some((local, domain)) = raw.split_once('@') {
        let head = local.chars().take(2).collect::<String>();
        format!("{head}***@{domain}")
    } else {
        mask_prefix_tail(raw, 2, 2)
    }
}

fn mask_ipv4(raw: &str) -> String {
    if let Some((first, _)) = raw.split_once('.') {
        format!("{first}.***.***.***")
    } else {
        mask_prefix_tail(raw, 2, 2)
    }
}

/// Digits-only tail preview: `4242 4242 4242 1234` → `**** **** **** 1234`.
/// Used for cards and SSNs, where "ending in ⋯" is the reading convention.
fn mask_last_four(raw: &str) -> String {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 4 {
        return "****".into();
    }
    let tail = &digits[digits.len() - 4..];
    format!("**** **** **** {tail}")
}

fn mask_prefix_tail(raw: &str, head: usize, tail: usize) -> String {
    let chars: Vec<char> = raw.chars().collect();
    if chars.len() <= head + tail {
        return "*".repeat(chars.len().max(4));
    }
    let prefix: String = chars.iter().take(head).collect();
    let suffix: String = chars
        .iter()
        .rev()
        .take(tail)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{prefix}***{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_keeps_domain_loses_local() {
        let m = mask_value("email", "john.doe@example.com");
        assert!(m.contains("@example.com"));
        assert!(!m.contains("john.doe"));
    }

    #[test]
    fn ipv4_keeps_first_octet() {
        let m = mask_value("ipv4", "192.168.1.5");
        assert!(m.starts_with("192."));
        assert!(!m.contains("168"));
    }

    #[test]
    fn credit_card_keeps_last_four() {
        let m = mask_value("credit_card", "4242 4242 4242 1234");
        assert!(m.ends_with("1234"));
        assert!(!m.contains("4242 4242 4242"));
    }

    #[test]
    fn aws_key_keeps_prefix_tail_only() {
        let m = mask_value("aws_access_key", "AKIAIOSFODNN7EXAMPLE");
        assert!(m.starts_with("AKIA"));
        assert!(m.ends_with("MPLE"));
        assert!(!m.contains("IOSFODNN7EXA"));
    }

    #[test]
    fn private_key_fully_obscured() {
        assert_eq!(
            mask_value("private_key_pem", "-----BEGIN RSA PRIVATE KEY-----"),
            "[PRIVATE KEY]"
        );
    }

    /// The four kinds Workspace's detector emits that Memory's policy catalog
    /// never had. Before this port they fell through to the generic 2+2 mask,
    /// which leaks the head and tail of a live secret.
    #[test]
    fn workspace_only_kinds_are_fully_obscured() {
        assert_eq!(
            mask_value("bearer_token", "Bearer sk_live_abcdef123456"),
            "[REDACTED]"
        );
        assert_eq!(
            mask_value("generic_credential_assignment", "hunter2supersecret"),
            "[REDACTED]"
        );
        assert_eq!(
            mask_value(
                "bitcoin_wif_private_key",
                "5HueCGU8rMjxEXxiPuD5BDku4MkFqeZyd4dZ1jvhTVqvbTLvyTJ"
            ),
            "[PRIVATE KEY]"
        );
        // SSN follows the "ending in" convention, not full obscurity.
        let ssn = mask_value("us_ssn", "123-45-6789");
        assert!(ssn.ends_with("6789"), "got {ssn}");
        assert!(!ssn.contains("123") && !ssn.contains("45-"), "got {ssn}");
    }

    // ── generalize_value ───────────────────────────────────────────────
    #[test]
    fn generalize_date_to_year_any_separator() {
        assert_eq!(
            generalize_value("dob", "1987-03-12").as_deref(),
            Some("1987")
        );
        assert_eq!(
            generalize_value("date", "12/03/1987").as_deref(),
            Some("1987")
        );
        assert_eq!(
            generalize_value("date", "1987.03.12").as_deref(),
            Some("1987")
        );
    }

    #[test]
    fn generalize_zip_to_region_prefix() {
        assert_eq!(
            generalize_value("us_zip", "90210-1234").as_deref(),
            Some("902xx")
        );
    }

    #[test]
    fn generalize_phone_keeps_prefix_drops_line() {
        let g = generalize_value("phone", "+1 415-555-0199").unwrap();
        assert!(g.starts_with("+1 4"), "got {g}");
        assert!(!g.contains("555") && !g.contains("0199"), "got {g}");
    }

    #[test]
    fn generalize_number_rounds_to_two_sig_figs() {
        assert_eq!(
            generalize_value("amount", "142000").as_deref(),
            Some("~140,000")
        );
        assert_eq!(
            generalize_value("amount", "148500").as_deref(),
            Some("~150,000")
        );
        assert_eq!(generalize_value("number", "87").as_deref(), Some("~87"));
    }

    #[test]
    fn generalize_unknown_kind_is_none() {
        // No hierarchy → None → caller redacts (safe default).
        assert!(generalize_value("us_ssn", "123-45-6789").is_none());
        assert!(generalize_value("email", "a@b.com").is_none());
    }

    #[test]
    fn group_thousands_boundaries() {
        assert_eq!(group_thousands(0), "0");
        assert_eq!(group_thousands(90), "90");
        assert_eq!(group_thousands(1500), "1,500");
        assert_eq!(group_thousands(142000), "142,000");
        assert_eq!(group_thousands(1000000), "1,000,000");
    }
}
