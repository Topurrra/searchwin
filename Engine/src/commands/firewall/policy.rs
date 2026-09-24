//! Egress policy — decides, per detected entity kind, what happens to a span
//! before text leaves the device for a model.
//!
//! Ported (heavily trimmed) from KeepItLocal Memory's
//! `kip-memory-core/src/firewall/policy.rs`. Deliberately NOT ported:
//!
//!   * `KNOWN_ENTITY_KINDS` — a 90-entry catalog that exists to populate
//!     Memory's policy-editor table. Workspace has no policy editor yet, and
//!     the catalog's own doc warns it must stay "in lock-step with what the
//!     detector actually emits" — a synchronization debt with no payer here.
//!     `default_action` below makes it unnecessary for correctness.
//!   * The five vault presets (legal/HR/finance/source-code) — those are
//!     Memory's *vault* surfaces, not AI egress.
//!   * `CustomPattern` / `TermList` / `scan_all` / `validate` — user-authored
//!     regexes. Genuinely wanted later ("never send my employer's name"), but
//!     they need a catastrophic-backtracking gate first, which is why Memory
//!     deferred loading them from disk too.
//!
//! One deliberate divergence from the source: Memory's `Suggest` and
//! `AutoAccept` both redact at egress and differ only in its interactive vault
//! review step, which Workspace does not have. Two variants with identical
//! behavior invite miswiring, so they are collapsed into `Redact`.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// What the firewall does with a detected span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyAction {
    /// Replace with a masked preview. The default for everything.
    Redact,
    /// Disclose a coarsened but real value (date → year, ZIP+4 → region).
    /// Falls back to `Redact` when the kind has no generalization hierarchy.
    Generalize,
    /// Send through untouched. Only ever reached by explicit opt-in.
    Allow,
}

impl Default for PolicyAction {
    /// Fail closed. A `PolicyAction` materialized from absent/corrupt config
    /// redacts rather than discloses.
    fn default() -> Self {
        PolicyAction::Redact
    }
}

/// An explicit per-kind override.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRule {
    pub kind: String,
    pub action: PolicyAction,
}

impl EntityRule {
    pub fn new(kind: impl Into<String>, action: PolicyAction) -> Self {
        Self {
            kind: kind.into(),
            action,
        }
    }
}

/// A named egress policy.
///
/// Carries no display strings: labels and descriptions are copy, and copy lives
/// in `src/lib/i18n/locales/`. The frontend maps `id` to translated text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub id: String,
    /// Per-kind overrides, checked before `default_action`.
    #[serde(default)]
    pub rules: Vec<EntityRule>,
    /// Action for any kind not named in `rules`.
    ///
    /// This is the field that makes the firewall safe against detector drift:
    /// a kind the scanner gains tomorrow is redacted today, without anyone
    /// remembering to add it to a catalog. `#[serde(default)]` + the `Default`
    /// impl above mean a config file missing this field still fails closed.
    #[serde(default)]
    pub default_action: PolicyAction,
    /// Kinds the owner explicitly turned redaction OFF for. Checked first, so
    /// it beats every rule and the default.
    #[serde(default)]
    pub disabled_kinds: HashSet<String>,
}

impl Policy {
    /// Resolution order: owner-disabled → explicit rule → `default_action`.
    pub fn action_for(&self, kind: &str) -> PolicyAction {
        if self.disabled_kinds.contains(kind) {
            return PolicyAction::Allow;
        }
        match self.rules.iter().find(|r| r.kind == kind) {
            Some(r) => r.action,
            None => self.default_action,
        }
    }
}

/// Redact everything the detector finds. The default for any model egress.
pub fn egress_default() -> Policy {
    Policy {
        id: "egress_default".into(),
        rules: Vec::new(),
        default_action: PolicyAction::Redact,
        disabled_kinds: HashSet::new(),
    }
}

/// Coarsen quasi-identifiers instead of withholding them, so an aggregating
/// model gets the shape of a fact rather than the exact value. Everything
/// without a generalization hierarchy still redacts.
pub fn egress_generalize() -> Policy {
    Policy {
        id: "egress_generalize".into(),
        rules: vec![
            EntityRule::new("dob", PolicyAction::Generalize),
            EntityRule::new("date", PolicyAction::Generalize),
            EntityRule::new("us_zip", PolicyAction::Generalize),
            EntityRule::new("phone", PolicyAction::Generalize),
        ],
        default_action: PolicyAction::Redact,
        disabled_kinds: HashSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_kind_redacts_by_default() {
        // The drift guarantee: a kind nobody configured still redacts.
        let p = egress_default();
        assert_eq!(
            p.action_for("some_kind_invented_next_year"),
            PolicyAction::Redact
        );
    }

    #[test]
    fn explicit_rule_beats_default() {
        let p = Policy {
            id: "t".into(),
            rules: vec![EntityRule::new("email", PolicyAction::Allow)],
            default_action: PolicyAction::Redact,
            disabled_kinds: HashSet::new(),
        };
        assert_eq!(p.action_for("email"), PolicyAction::Allow);
        assert_eq!(p.action_for("credit_card"), PolicyAction::Redact);
    }

    #[test]
    fn disabled_kinds_beat_rules() {
        let p = Policy {
            id: "t".into(),
            rules: vec![EntityRule::new("email", PolicyAction::Redact)],
            default_action: PolicyAction::Redact,
            disabled_kinds: HashSet::from(["email".to_string()]),
        };
        assert_eq!(p.action_for("email"), PolicyAction::Allow);
    }

    #[test]
    fn generalize_preset_coarsens_only_quasi_identifiers() {
        let p = egress_generalize();
        assert_eq!(p.action_for("dob"), PolicyAction::Generalize);
        assert_eq!(p.action_for("us_ssn"), PolicyAction::Redact);
    }

    /// A persisted policy written before `default_action` existed must not
    /// deserialize into "allow everything".
    #[test]
    fn config_missing_default_action_fails_closed() {
        let p: Policy = serde_json::from_str(r#"{"id":"legacy"}"#).unwrap();
        assert_eq!(p.default_action, PolicyAction::Redact);
        assert_eq!(p.action_for("credit_card"), PolicyAction::Redact);
    }
}
