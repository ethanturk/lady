//! Custom-command templates (PH3-009): parse `{name:kind}` placeholders and
//! build a safe argument vector by substituting values *after* tokenizing the
//! template. Values are never concatenated into a shell string, so user input
//! cannot inject extra commands or arguments.

use std::collections::{HashMap, HashSet};

use lady_proto::{Placeholder, PlaceholderKind};

/// Map a placeholder kind keyword to its enum.
fn kind_of(s: &str) -> Option<PlaceholderKind> {
    match s {
        "text" => Some(PlaceholderKind::Text),
        "branch" => Some(PlaceholderKind::Branch),
        "file" => Some(PlaceholderKind::File),
        _ => None,
    }
}

/// Parse the distinct `{name:kind}` placeholders from `template`, in first-seen
/// order. Unknown kinds and malformed braces are ignored.
pub fn parse_placeholders(template: &str) -> Vec<Placeholder> {
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else { break };
        let inner = &after[..end];
        if let Some((name, kind)) = inner.split_once(':') {
            if let Some(kind) = kind_of(kind) {
                if seen.insert(name.to_string()) {
                    out.push(Placeholder {
                        name: name.to_string(),
                        kind,
                    });
                }
            }
        }
        rest = &after[end + 1..];
    }
    out
}

/// Substitute placeholders inside one whitespace-delimited token. A `{name:kind}`
/// is replaced by `values[name]` (empty when absent); text that is not a valid
/// placeholder is preserved verbatim.
fn substitute_token(token: &str, values: &HashMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = token;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            // Unterminated brace: keep the rest literally.
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = &after[..end];
        match inner.split_once(':') {
            Some((name, kind)) if kind_of(kind).is_some() => {
                out.push_str(values.get(name).map(String::as_str).unwrap_or(""));
            }
            // Not a valid placeholder — keep `{...}` literally.
            _ => {
                out.push('{');
                out.push_str(inner);
                out.push('}');
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Build a safe argv from `template` and `values`. The template is split on
/// whitespace first; each token then has its placeholders substituted, so a
/// value containing spaces or shell metacharacters stays a single argument.
/// Tokens that resolve to empty are dropped.
pub fn build_argv(template: &str, values: &HashMap<String, String>) -> Vec<String> {
    template
        .split_whitespace()
        .map(|tok| substitute_token(tok, values))
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vals(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn parses_typed_placeholders_in_order_deduped() {
        let p = parse_placeholders("git log {rev:text} {b:branch} {rev:text} {f:file}");
        assert_eq!(p.len(), 3, "rev deduped");
        assert_eq!(p[0].name, "rev");
        assert_eq!(p[0].kind, PlaceholderKind::Text);
        assert_eq!(p[1].kind, PlaceholderKind::Branch);
        assert_eq!(p[2].kind, PlaceholderKind::File);
    }

    #[test]
    fn unknown_kind_is_ignored() {
        assert!(parse_placeholders("git {x:weird} log").is_empty());
    }

    #[test]
    fn build_argv_substitutes_into_argument_vector() {
        let argv = build_argv(
            "git log {rev:text} --author={who:text}",
            &vals(&[("rev", "HEAD~3"), ("who", "Ada")]),
        );
        assert_eq!(argv, vec!["git", "log", "HEAD~3", "--author=Ada"]);
    }

    #[test]
    fn value_with_spaces_stays_one_argument() {
        let argv = build_argv("git log --grep={q:text}", &vals(&[("q", "fix the bug")]));
        assert_eq!(argv, vec!["git", "log", "--grep=fix the bug"]);
    }

    #[test]
    fn injection_attempt_stays_a_single_argument() {
        // A shell metacharacter payload must remain one argv element — never a
        // second command — because we tokenize the template, not the value.
        let argv = build_argv("git log {rev:text}", &vals(&[("rev", "x; rm -rf /")]));
        assert_eq!(argv, vec!["git", "log", "x; rm -rf /"]);
        assert_eq!(argv.len(), 3, "no extra argv elements from the payload");
    }

    #[test]
    fn missing_value_drops_to_empty_token() {
        let argv = build_argv("git {a:text} log", &vals(&[]));
        assert_eq!(argv, vec!["git", "log"], "empty placeholder token dropped");
    }
}
