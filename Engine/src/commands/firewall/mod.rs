//! Disclosure firewall — the single seam through which text may reach a model.
//!
//! Ported from KeepItLocal Memory. Workspace supplies its own detector
//! (`super::sensitive_scan`, same lineage as Memory's, already consumed by
//! Clipboard History / Privacy Audit / Secret Scanner / the search index), so
//! only the *policy* and *masking* halves were new capability. See the module
//! docs in `policy.rs` for the full not-ported list.
//!
//! ## Why a newtype instead of a function everyone remembers to call
//!
//! [`Disclosed`] wraps a `String` whose only constructor is
//! [`Disclosure::disclosed`], which is only reachable from [`apply`]. The field
//! is private, so no other module — in this crate or outside it — can build one.
//! When the AI transport accepts only `Disclosed`, "raw text reached the model"
//! stops being a bug you review for and becomes a program that does not compile.
//!
//! Memory additionally exposed a `pub(crate) disclose()` escape hatch for its
//! recall producer. Workspace has no such caller, so it is not ported: `apply`
//! is the sole mint. Do not add one without a concrete need — it is the only
//! thing standing between this design and a convention.

pub mod mask;
pub mod policy;

use super::sensitive_scan::Finding;
use mask::{generalize_value, mask_value};
use policy::{Policy, PolicyAction};

/// The sole egress type for text leaving the device toward a model.
#[derive(Debug, Clone)]
pub struct Disclosed(String);

impl Disclosed {
    /// Borrow the firewalled text for serialization onto the wire.
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Consume into the owned firewalled String.
    pub fn into_inner(self) -> String {
        self.0
    }
}

/// The result of running the firewall over one piece of text.
#[derive(Debug, Clone)]
pub struct Disclosure {
    /// Content with redacted spans replaced by masked previews and generalized
    /// spans replaced by their coarsened value.
    pub text: String,
    /// Distinct kinds that were redacted, in document order. Drives the
    /// user-facing receipt ("3 secrets withheld").
    pub redacted_kinds: Vec<&'static str>,
    /// Distinct kinds that were *generalized* (coarsened, still disclosed), in
    /// document order. Disjoint from `redacted_kinds`: a Generalize finding
    /// with no hierarchy falls back to redaction and lands in the other list.
    pub generalized_kinds: Vec<&'static str>,
}

impl Disclosure {
    /// Mint the egress token. The only path to a [`Disclosed`] in the codebase.
    pub fn disclosed(&self) -> Disclosed {
        Disclosed(self.text.clone())
    }
    /// Whether anything was withheld or coarsened — for the "we redacted N
    /// things" affordance before a send.
    pub fn is_modified(&self) -> bool {
        !self.redacted_kinds.is_empty() || !self.generalized_kinds.is_empty()
    }
}

/// Transform every finding whose policy action is not `Allow`.
///
/// `Generalize` discloses a coarsened value (date → year, ZIP+4 → prefix); a
/// Generalize kind with no hierarchy falls back to redaction. Spans are byte
/// offsets into `content`; replacement runs back-to-front so earlier offsets
/// stay valid, and overlapping findings are skipped rather than double-applied.
pub fn apply(content: &str, findings: &[Finding], policy: &Policy) -> Disclosure {
    let mut to_process: Vec<&Finding> = findings
        .iter()
        .filter(|f| policy.action_for(f.kind) != PolicyAction::Allow)
        .collect();
    to_process.sort_by_key(|f| f.start);

    let mut redacted_kinds: Vec<&'static str> = Vec::new();
    let mut generalized_kinds: Vec<&'static str> = Vec::new();
    let push_distinct = |v: &mut Vec<&'static str>, k: &'static str| {
        if !v.contains(&k) {
            v.push(k);
        }
    };

    let mut text = content.to_string();
    let mut last_start = text.len();
    for f in to_process.iter().rev() {
        // Skip overlaps (a finding ending past where the previous one began)
        // and any span the detector reported outside the text.
        if f.end > last_start || f.end > text.len() || f.start >= f.end {
            continue;
        }
        if !content.is_char_boundary(f.start) || !content.is_char_boundary(f.end) {
            continue;
        }
        let raw = &content[f.start..f.end];
        let (preview, generalized) = match policy.action_for(f.kind) {
            PolicyAction::Generalize => match generalize_value(f.kind, raw) {
                Some(coarse) => (coarse, true),
                None => (mask_value(f.kind, raw), false),
            },
            _ => (mask_value(f.kind, raw), false),
        };
        if generalized {
            push_distinct(&mut generalized_kinds, f.kind);
        } else {
            push_distinct(&mut redacted_kinds, f.kind);
        }
        text.replace_range(f.start..f.end, &preview);
        last_start = f.start;
    }

    // Replacement ran back-to-front; restore document order for the receipt.
    redacted_kinds.reverse();
    generalized_kinds.reverse();

    Disclosure {
        text,
        redacted_kinds,
        generalized_kinds,
    }
}

/// Scan `content` with the shared detector and firewall it under `policy`.
/// The normal entry point — callers should not hand-roll the scan/apply pair.
pub fn firewall(content: &str, policy: &Policy) -> Disclosure {
    // include_low = true: an email or IP is exactly the kind of quiet
    // identifier that matters at egress even when it is noise in a code audit.
    let findings = super::sensitive_scan::scan_text_detailed(content, true);
    apply(content, &findings, policy)
}

#[cfg(test)]
mod tests {
    use super::policy::{egress_default, egress_generalize, EntityRule, Policy, PolicyAction};
    use super::*;
    use std::collections::HashSet;

    /// End-to-end through the real detector: the secret must not survive.
    #[test]
    fn real_aws_key_does_not_survive_the_firewall() {
        let text = "deploy with AKIAIOSFODNN7EXAMPLE and you are done";
        let d = firewall(text, &egress_default());
        assert!(
            !d.text.contains("AKIAIOSFODNN7EXAMPLE"),
            "secret survived: {}",
            d.text
        );
        assert!(d.text.contains("deploy with"), "context lost: {}", d.text);
        assert!(d.is_modified());
    }

    /// The whole point of `default_action`. A kind with no rule anywhere still
    /// gets redacted — no catalog to keep in sync, no drift.
    #[test]
    fn unconfigured_kind_is_still_redacted() {
        let findings = [Finding {
            kind: "a_kind_no_policy_mentions",
            tier: crate::commands::sensitive_scan::Tier::High,
            start: 0,
            end: 6,
        }];
        let d = apply("SECRET rest", &findings, &egress_default());
        assert!(!d.text.starts_with("SECRET"), "got {}", d.text);
        assert_eq!(d.redacted_kinds, vec!["a_kind_no_policy_mentions"]);
    }

    #[test]
    fn allow_lets_a_kind_through_untouched() {
        let p = Policy {
            id: "t".into(),
            rules: vec![EntityRule::new("email", PolicyAction::Allow)],
            default_action: PolicyAction::Redact,
            disabled_kinds: HashSet::new(),
        };
        let d = firewall("reach me at dev@example.com", &p);
        assert!(d.text.contains("dev@example.com"), "got {}", d.text);
        assert!(!d.is_modified());
    }

    /// Multiple findings, back-to-front replacement: every span must land on
    /// the right text despite earlier replacements changing the length.
    #[test]
    fn multiple_spans_all_replaced_correctly() {
        let text = "key AKIAIOSFODNN7EXAMPLE then mail a@b.com end";
        let d = firewall(text, &egress_default());
        assert!(!d.text.contains("AKIAIOSFODNN7EXAMPLE"), "got {}", d.text);
        assert!(!d.text.contains("a@b.com"), "got {}", d.text);
        assert!(d.text.starts_with("key "), "got {}", d.text);
        assert!(d.text.ends_with(" end"), "got {}", d.text);
    }

    #[test]
    fn generalize_coarsens_but_redaction_still_wins_for_secrets() {
        let findings = [
            Finding {
                kind: "dob",
                tier: crate::commands::sensitive_scan::Tier::Medium,
                start: 0,
                end: 10,
            },
            Finding {
                kind: "us_ssn",
                tier: crate::commands::sensitive_scan::Tier::High,
                start: 14,
                end: 25,
            },
        ];
        let d = apply("1987-03-12 on 123-45-6789", &findings, &egress_generalize());
        assert!(d.text.contains("1987"), "date not coarsened: {}", d.text);
        assert!(!d.text.contains("1987-03-12"), "exact date leaked: {}", d.text);
        assert!(!d.text.contains("123-45-6789"), "ssn leaked: {}", d.text);
        assert_eq!(d.generalized_kinds, vec!["dob"]);
        assert_eq!(d.redacted_kinds, vec!["us_ssn"]);
    }

    /// Multi-byte input must not panic or slice mid-character.
    #[test]
    fn utf8_content_is_not_sliced_mid_character() {
        let text = "გამარჯობა AKIAIOSFODNN7EXAMPLE მადლობა";
        let d = firewall(text, &egress_default());
        assert!(!d.text.contains("AKIAIOSFODNN7EXAMPLE"), "got {}", d.text);
        assert!(d.text.contains("გამარჯობა"), "got {}", d.text);
        assert!(d.text.contains("მადლობა"), "got {}", d.text);
    }

    #[test]
    fn clean_text_passes_through_unchanged() {
        let text = "just some ordinary notes about the weather";
        let d = firewall(text, &egress_default());
        assert_eq!(d.text, text);
        assert!(!d.is_modified());
    }
}
