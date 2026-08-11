//! `{{VAR}}` substitution from an active environment into a request spec —
//! the piece that turns a saved request from "one URL" into "one request,
//! any environment", the thing this module's own top-level doc comment has
//! named as deferred since the day it was written.

use crate::{ApiEnvVar, ApiHeader, ApiRequestSpec};

/// Replace every `{{KEY}}` in `text` with the matching variable's value.
///
/// An unknown key is left as the literal `{{KEY}}` text rather than removed
/// or substituted with an empty string. Vanishing an unresolved placeholder
/// would turn a typo'd variable name (`{{API_UR}}`) into a URL that looks
/// plausible and points nowhere the user intended — leaving the literal
/// `{{...}}` in the request is the version of this failure that is visible
/// immediately, in the URL bar or the sent headers, rather than discovered
/// later as a mysterious 404.
///
/// Case-sensitive and exact-match only: `{{API_URL}}` matches a variable
/// named `API_URL`, not `api_url` — the same convention shell environment
/// variables use, which is what this feature is modeled on.
pub fn substitute(text: &str, vars: &[ApiEnvVar]) -> String {
    let mut out = text.to_string();
    for var in vars {
        out = out.replace(&format!("{{{{{}}}}}", var.key), &var.value);
    }
    out
}

/// Apply [`substitute`] to every templated part of a request: the URL,
/// every header's value (not its name — templating a header name is not a
/// pattern this supports, matching what Postman's own UI defaults to), and
/// the body if present.
pub fn substitute_spec(spec: &ApiRequestSpec, vars: &[ApiEnvVar]) -> ApiRequestSpec {
    ApiRequestSpec {
        method: spec.method.clone(),
        url: substitute(&spec.url, vars),
        headers: spec
            .headers
            .iter()
            .map(|h| ApiHeader {
                name: h.name.clone(),
                value: substitute(&h.value, vars),
            })
            .collect(),
        body: spec.body.as_ref().map(|b| substitute(b, vars)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(key: &str, value: &str) -> ApiEnvVar {
        ApiEnvVar {
            id: String::new(),
            key: key.into(),
            value: value.into(),
            secret: false,
        }
    }

    #[test]
    fn replaces_a_known_variable() {
        let vars = vec![var("API_URL", "https://api.example.com")];
        assert_eq!(
            substitute("{{API_URL}}/users", &vars),
            "https://api.example.com/users"
        );
    }

    #[test]
    fn leaves_an_unknown_variable_as_literal_text_rather_than_vanishing_it() {
        let vars = vec![var("API_URL", "https://api.example.com")];
        assert_eq!(
            substitute("{{API_URL}}/{{MISSING}}", &vars),
            "https://api.example.com/{{MISSING}}"
        );
    }

    #[test]
    fn is_case_sensitive() {
        let vars = vec![var("API_URL", "https://api.example.com")];
        assert_eq!(substitute("{{api_url}}", &vars), "{{api_url}}");
    }

    #[test]
    fn replaces_every_occurrence_of_the_same_variable() {
        let vars = vec![var("HOST", "example.com")];
        assert_eq!(
            substitute("https://{{HOST}}/api?redirect={{HOST}}", &vars),
            "https://example.com/api?redirect=example.com"
        );
    }

    #[test]
    fn plain_text_with_no_placeholders_is_unchanged() {
        assert_eq!(
            substitute("https://example.com/fixed", &[]),
            "https://example.com/fixed"
        );
    }

    #[test]
    fn substitute_spec_covers_url_header_values_and_body_but_not_header_names() {
        let spec = ApiRequestSpec {
            method: "POST".into(),
            url: "{{API_URL}}/users".into(),
            headers: vec![ApiHeader {
                name: "{{HEADER_NAME}}".into(),
                value: "Bearer {{TOKEN}}".into(),
            }],
            body: Some(r#"{"host":"{{API_URL}}"}"#.into()),
        };
        let vars = vec![
            var("API_URL", "https://api.example.com"),
            var("TOKEN", "secret-abc"),
        ];

        let resolved = substitute_spec(&spec, &vars);

        assert_eq!(resolved.url, "https://api.example.com/users");
        assert_eq!(
            resolved.headers[0].name, "{{HEADER_NAME}}",
            "header names are not templated"
        );
        assert_eq!(resolved.headers[0].value, "Bearer secret-abc");
        assert_eq!(
            resolved.body.as_deref(),
            Some(r#"{"host":"https://api.example.com"}"#)
        );
    }

    #[test]
    fn an_empty_environment_leaves_everything_as_authored() {
        let spec = ApiRequestSpec {
            method: "GET".into(),
            url: "{{API_URL}}/x".into(),
            headers: vec![],
            body: None,
        };
        let resolved = substitute_spec(&spec, &[]);
        assert_eq!(resolved.url, "{{API_URL}}/x");
    }
}
