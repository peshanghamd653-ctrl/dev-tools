//! Secret detection and redaction for text about to reach an AI provider.
//!
//! DevOS's AI tools (`read_file`, `git_diff`, `run_command`, `search_code`,
//! …) hand real file content, diffs, and command output back into the
//! model's context. Any of that can contain a credential the user never
//! meant to share with a third party — an `.env` line, a pasted AWS key, a
//! JWT in a log line. [`redact`] is the one choke point every such tool
//! result passes through before it reaches `devos-ai` (wired in
//! `src-tauri/src/tools.rs`'s `ProjectTools::execute`, the same "one place,
//! not one arm per tool" shape that fixed SEC-002 for approval gating).
//!
//! ## What this is and is not
//!
//! This is pattern matching against known credential *shapes* (`sk-ant-…`,
//! `AKIA…`, `-----BEGIN … PRIVATE KEY-----`, `SOME_TOKEN=…`), not a secret
//! scanner with provenance or entropy analysis. It will miss a secret that
//! doesn't look like any of these — a bespoke internal token with no
//! recognizable prefix — and it will occasionally redact a config value
//! that happens to match a shape (`SECRET_NAME=my-application`). Both
//! failure directions exist; the second one is the side to err on, because
//! over-redacting a harmless value costs a little context, while
//! under-redacting a real key costs the credential.
//!
//! ## What is disclosed about a finding
//!
//! [`Finding`] carries a `kind` and a `line`, never the matched text — the
//! same principle `audit_log` already follows for tool calls: record that
//! something happened, never a value that could itself be the credential.

use std::sync::LazyLock;

use regex::Regex;

/// One thing that looked like a credential and was replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub kind: &'static str,
    /// 1-based, matching every other line-number convention in this codebase.
    pub line: usize,
}

/// The result of a redaction pass.
#[derive(Debug, Clone)]
pub struct Redacted {
    pub text: String,
    pub findings: Vec<Finding>,
}

impl Redacted {
    /// The redacted text, with a trailing note naming what was found — never
    /// what it was — when anything was. A model (and whoever reads the
    /// conversation afterward) sees that content was withheld rather than a
    /// silently altered file.
    pub fn into_text(self) -> String {
        if self.findings.is_empty() {
            return self.text;
        }
        let mut kinds: Vec<&str> = self.findings.iter().map(|f| f.kind).collect();
        kinds.sort_unstable();
        kinds.dedup();
        format!(
            "{}\n\n[DevOS redacted {} potential secret{} before sharing this with the model: {}]",
            self.text,
            self.findings.len(),
            if self.findings.len() == 1 { "" } else { "s" },
            kinds.join(", "),
        )
    }
}

/// Patterns in priority order: when two patterns would claim overlapping
/// text (an Anthropic key is also `sk-`-prefixed), the earlier one in this
/// list wins, so a key gets its specific label rather than a generic one.
static PATTERNS: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    let defs: &[(&str, &str)] = &[
        (
            "private-key",
            r"-----BEGIN (?:RSA |EC |OPENSSH |DSA |)PRIVATE KEY-----[\s\S]+?-----END (?:RSA |EC |OPENSSH |DSA |)PRIVATE KEY-----",
        ),
        ("anthropic-api-key", r"sk-ant-[A-Za-z0-9_-]{20,}"),
        ("stripe-key", r"sk_(?:live|test)_[A-Za-z0-9]{16,}"),
        ("generic-sk-api-key", r"sk-[A-Za-z0-9_-]{20,}"),
        ("github-token", r"gh[pousr]_[A-Za-z0-9]{36,}"),
        ("slack-token", r"xox[baprs]-[A-Za-z0-9-]{10,}"),
        ("google-api-key", r"AIza[0-9A-Za-z_-]{35}"),
        ("aws-access-key-id", r"AKIA[0-9A-Z]{16}"),
        (
            "jwt",
            r"eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
        ),
        (
            "env-style-secret",
            r#"(?i)\b[A-Z0-9_]*(?:API[_-]?KEY|SECRET|TOKEN|PASSWORD|PASSWD|ACCESS[_-]?KEY|PRIVATE[_-]?KEY)[A-Z0-9_]*\s*[:=]\s*['"]?[^\s'",]{8,}['"]?"#,
        ),
    ];
    defs.iter()
        .map(|(kind, pattern)| {
            (
                *kind,
                Regex::new(pattern).expect("static redaction pattern must compile"),
            )
        })
        .collect()
});

/// Scan `text` for anything that looks like a credential and replace each
/// match with `[REDACTED:<kind>]`.
pub fn redact(text: &str) -> Redacted {
    let mut claimed: Vec<(usize, usize, &'static str)> = Vec::new();

    for (kind, regex) in PATTERNS.iter() {
        for m in regex.find_iter(text) {
            let overlaps = claimed
                .iter()
                .any(|&(start, end, _)| m.start() < end && start < m.end());
            if !overlaps {
                claimed.push((m.start(), m.end(), kind));
            }
        }
    }
    claimed.sort_by_key(|&(start, ..)| start);

    let mut out = String::with_capacity(text.len());
    let mut findings = Vec::with_capacity(claimed.len());
    let mut cursor = 0;
    for (start, end, kind) in claimed {
        out.push_str(&text[cursor..start]);
        out.push_str("[REDACTED:");
        out.push_str(kind);
        out.push(']');
        findings.push(Finding {
            kind,
            line: 1 + text[..start].matches('\n').count(),
        });
        cursor = end;
    }
    out.push_str(&text[cursor..]);

    Redacted {
        text: out,
        findings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(redacted: &Redacted) -> Vec<&str> {
        redacted.findings.iter().map(|f| f.kind).collect()
    }

    #[test]
    fn text_with_nothing_secret_passes_through_unchanged() {
        let text = "fn main() {\n    println!(\"hello\");\n}\n";
        let redacted = redact(text);
        assert_eq!(redacted.text, text);
        assert!(redacted.findings.is_empty());
    }

    #[test]
    fn an_openai_style_key_is_redacted() {
        let redacted = redact("OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz123456");
        assert!(redacted.text.contains("[REDACTED:"));
        assert!(!redacted.text.contains("abcdefghijklmnopqrstuvwxyz"));
    }

    #[test]
    fn an_anthropic_key_gets_its_own_specific_label_not_the_generic_one() {
        let redacted = redact("sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789");
        assert_eq!(kinds(&redacted), vec!["anthropic-api-key"]);
    }

    #[test]
    fn an_aws_access_key_id_is_redacted() {
        let redacted = redact("aws_access_key_id = AKIAIOSFODNN7EXAMPLE");
        // Matches both the specific AWS pattern *and* the generic
        // env-style pattern (`..._key_id = AKIA...`) — the AWS one is
        // listed first, so it wins and claims the value alone.
        assert!(kinds(&redacted).contains(&"aws-access-key-id"));
        assert!(!redacted.text.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn a_github_token_is_redacted() {
        let redacted = redact("token: ghp_1234567890abcdefghijklmnopqrstuvwxyz");
        assert!(kinds(&redacted).contains(&"github-token"));
    }

    #[test]
    fn a_generic_database_password_assignment_is_redacted() {
        let redacted = redact("DATABASE_PASSWORD=correcthorsebatterystaple");
        assert_eq!(kinds(&redacted), vec!["env-style-secret"]);
        assert!(!redacted.text.contains("correcthorsebatterystaple"));
    }

    #[test]
    fn a_jwt_is_redacted() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let redacted = redact(&format!("Authorization: Bearer {jwt}"));
        assert_eq!(kinds(&redacted), vec!["jwt"]);
    }

    #[test]
    fn a_private_key_block_is_redacted_as_one_finding_not_many() {
        let pem =
            "-----BEGIN RSA PRIVATE KEY-----\nMIIBOgIBAAJBAK...\n-----END RSA PRIVATE KEY-----";
        let redacted = redact(pem);
        assert_eq!(redacted.findings.len(), 1);
        assert_eq!(redacted.findings[0].kind, "private-key");
    }

    #[test]
    fn a_short_config_value_is_left_alone() {
        // Under the 8-character floor — real secrets are essentially never
        // this short, and treating every `_TOKEN=` as suspect regardless of
        // length would redact things like `RETRY_TOKEN=3`.
        let redacted = redact("RETRY_TOKEN=3");
        assert!(redacted.findings.is_empty());
    }

    #[test]
    fn findings_report_the_line_they_were_found_on() {
        let redacted = redact("line one\nline two\nAPI_KEY=abcdefghijklmnop\nline four");
        assert_eq!(redacted.findings.len(), 1);
        assert_eq!(redacted.findings[0].line, 3);
    }

    #[test]
    fn multiple_distinct_secrets_are_each_found_and_all_named_in_the_summary() {
        let redacted = redact(
            "OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz123456\n\
             aws_secret_token = AKIAIOSFODNN7EXAMPLE\n",
        );
        assert_eq!(redacted.findings.len(), 2);
        let summary = redacted.into_text();
        assert!(summary.contains("DevOS redacted 2 potential secrets"));
    }

    #[test]
    fn into_text_adds_no_note_when_nothing_was_found() {
        let redacted = redact("nothing interesting here");
        assert_eq!(redacted.into_text(), "nothing interesting here");
    }

    #[test]
    fn overlapping_matches_are_claimed_once_by_the_higher_priority_pattern() {
        // "sk-ant-..." matches both the Anthropic-specific pattern and the
        // generic env-style pattern (`SECRET_KEY = sk-ant-...`). The output
        // must show one redaction marker, not two overlapping ones.
        let redacted = redact("SECRET_KEY = sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789");
        assert_eq!(redacted.findings.len(), 1);
        assert_eq!(redacted.text.matches("[REDACTED:").count(), 1);
    }
}
