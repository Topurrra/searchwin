//! Regex Tool backend helpers. Wave 3.4 (2026-05-27).
//!
//! Currently houses a single command: `regex_from_examples`, which
//! delegates to the `grex` crate to synthesize a regular expression
//! that matches every input in a user-supplied list. Useful for
//! "I have these five strings, give me a pattern that matches all of
//! them" — the friendliest path into regex for users who don't know
//! the syntax.
//!
//! The crate is pure-Rust + Apache-2.0; no external binary, no
//! native deps.

use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegexFromExamplesOptions {
    /// One example per entry. Empty strings are filtered out before
    /// being passed to grex — they otherwise produce a pattern with
    /// an awkward leading alternation.
    pub examples: Vec<String>,
    /// Apply grex's "compact pattern" conversions: character classes
    /// (digits → `\d`, words → `\w`, whitespace → `\s`) AND repetition
    /// detection (runs collapse to `\w+` / `{n,m}` instead of `\w\w\w`).
    /// On by default — almost always what the user means by "make a
    /// pattern that matches these".
    #[serde(default = "default_true")]
    pub convert_classes: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegexFromExamplesResult {
    pub pattern: String,
    /// How many examples actually contributed (after filtering empty).
    pub examples_used: usize,
}

#[tauri::command]
pub fn regex_from_examples(
    options: RegexFromExamplesOptions,
) -> Result<RegexFromExamplesResult, String> {
    use grex::RegExpBuilder;

    // Filter empties + strip outer whitespace so a stray newline at
    // the bottom of the textarea doesn't poison the pattern with an
    // accidental empty-string alternation.
    let inputs: Vec<String> = options
        .examples
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if inputs.is_empty() {
        return Err("Provide at least one non-empty example.".to_string());
    }

    // grex's `from(&inputs)` returns a builder we configure chain-style.
    let mut builder = RegExpBuilder::from(&inputs);
    if options.convert_classes {
        // Repetition detection collapses runs (`\w\w\w` → `\w+` / `\w{3}`)
        // instead of overfitting into a positional alternation; the class
        // conversions compact digits / words / whitespace. Together this is
        // the "smart, readable pattern" mode.
        builder.with_conversion_of_repetitions();
        builder.with_conversion_of_digits();
        builder.with_conversion_of_words();
        builder.with_conversion_of_whitespace();
    }
    // We deliberately do NOT call with_case_insensitive_matching(): grex emits
    // an inline `(?i)` prefix, which the tester's ECMAScript engine rejects
    // ("Invalid group"). Case-insensitivity is applied as the RegExp `i` flag
    // in the frontend instead.

    let pattern = builder.build();
    Ok(RegexFromExamplesResult {
        pattern,
        examples_used: inputs.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    /// The whole point of grex: the suggested pattern must match every example.
    /// Also guards the two bug fixes — repetition detection (no verbose
    /// `\w\w\w`) and no baked-in `(?i)` (which the JS tester rejects).
    #[test]
    fn suggests_a_clean_pattern_that_matches_all_examples() {
        let examples = ["one@example.com", "two@foo.org", "three@bar.net"];
        let res = regex_from_examples(RegexFromExamplesOptions {
            examples: examples.iter().map(|s| s.to_string()).collect(),
            convert_classes: true,
        })
        .unwrap();

        let re = Regex::new(&res.pattern).expect("grex output must be a valid regex");
        for ex in examples {
            assert!(re.is_match(ex), "pattern `{}` should match `{ex}`", res.pattern);
        }
        assert!(
            !res.pattern.contains("\\w\\w\\w"),
            "repetition detection should collapse runs, got `{}`",
            res.pattern
        );
        assert!(
            !res.pattern.contains("(?i)"),
            "must not bake in an inline (?i) flag, got `{}`",
            res.pattern
        );
    }
}
