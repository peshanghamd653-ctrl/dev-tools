//! `devos-plugin.toml` — the declared surface of a plugin.
//!
//! Everything a plugin may do is written down here, in data, before any code
//! runs. Parsing is deliberately strict: a manifest that does not validate is
//! not loaded at all, because every later guarantee in this crate (which host
//! functions get defined, which tables the plugin may touch, which hosts it
//! may reach) is derived from these fields.

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("could not parse devos-plugin.toml: {0}")]
    Parse(String),
    #[error("invalid plugin id {id:?}: {reason}")]
    Id { id: String, reason: String },
    #[error("invalid entry {entry:?}: {reason}")]
    Entry { entry: String, reason: String },
    #[error("contribution {contributed:?} is not under the plugin's own id {id:?}")]
    ForeignContribution { id: String, contributed: String },
    #[error("invalid network permission {host:?}: {reason}")]
    NetHost { host: String, reason: String },
    #[error("filesystem permissions are not supported: {0:?} requested")]
    FsRequested(Vec<String>),
    #[error("{field} must not be empty")]
    Empty { field: &'static str },
}

pub type ManifestResult<T> = Result<T, ManifestError>;

/// What the plugin is allowed to reach. Absent fields mean "nothing", never
/// "everything" — `#[serde(default)]` on a `Vec` or on [`DbAccess::None`] is
/// what makes an under-specified manifest the *least* privileged one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permissions {
    #[serde(default)]
    pub db: DbAccess,
    /// Bare hostnames the plugin may reach over HTTPS. No schemes, no paths,
    /// no wildcards.
    #[serde(default)]
    pub net: Vec<String>,
    /// Present so a manifest requesting it is *rejected loudly* rather than
    /// silently ignored. See [`ManifestError::FsRequested`].
    #[serde(default)]
    pub fs: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DbAccess {
    #[default]
    None,
    /// Tables under the plugin's own [`PluginManifest::table_prefix`].
    OwnTables,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contributions {
    #[serde(default)]
    pub commands: Vec<CommandContribution>,
    #[serde(default)]
    pub panels: Vec<PanelContribution>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandContribution {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelContribution {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub entry: String,
    #[serde(default)]
    pub contributes: Contributions,
    #[serde(default)]
    pub permissions: Permissions,
}

impl PluginManifest {
    /// Parse and validate. There is no way to obtain an unvalidated
    /// `PluginManifest` from outside this module by parsing — `parse` is the
    /// only constructor that reads untrusted input, and it always validates.
    pub fn parse(toml_src: &str) -> ManifestResult<Self> {
        let manifest: PluginManifest =
            toml::from_str(toml_src).map_err(|e| ManifestError::Parse(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// The SQL table prefix this plugin owns. `acme.todo` →
    /// `plugin_acme_todo_`. Derived from the id rather than declared, so a
    /// plugin cannot name a prefix belonging to a core module (`ai_`,
    /// `index_`, `api_`, `db_`, `monitor*`) — every derived prefix starts
    /// with `plugin_`.
    pub fn table_prefix(&self) -> String {
        let flat: String = self
            .id
            .chars()
            .map(|c| if c == '.' || c == '-' { '_' } else { c })
            .collect();
        format!("plugin_{flat}_")
    }

    fn validate(&self) -> ManifestResult<()> {
        validate_id(&self.id)?;
        if self.name.trim().is_empty() {
            return Err(ManifestError::Empty { field: "name" });
        }
        if self.version.trim().is_empty() {
            return Err(ManifestError::Empty { field: "version" });
        }
        validate_entry(&self.entry)?;

        // A plugin contributes only under its own id. Without this, `acme.todo`
        // could contribute a command called `git.commit` and take over a core
        // command in the palette.
        let own = format!("{}.", self.id);
        for command in &self.contributes.commands {
            if !command.id.starts_with(&own) {
                return Err(ManifestError::ForeignContribution {
                    id: self.id.clone(),
                    contributed: command.id.clone(),
                });
            }
        }
        for panel in &self.contributes.panels {
            if !panel.id.starts_with(&own) {
                return Err(ManifestError::ForeignContribution {
                    id: self.id.clone(),
                    contributed: panel.id.clone(),
                });
            }
        }

        for host in &self.permissions.net {
            validate_net_host(host)?;
        }
        if !self.permissions.fs.is_empty() {
            return Err(ManifestError::FsRequested(self.permissions.fs.clone()));
        }
        Ok(())
    }
}

/// Reverse-DNS-ish: at least two dot-separated lowercase segments. The id is
/// load-bearing — it namespaces tables, key-value keys, commands and panels —
/// so it is constrained to characters that are safe in all of those without
/// escaping.
fn validate_id(id: &str) -> ManifestResult<()> {
    let reject = |reason: &str| {
        Err(ManifestError::Id {
            id: id.to_string(),
            reason: reason.to_string(),
        })
    };
    if id.is_empty() {
        return reject("must not be empty");
    }
    let segments: Vec<&str> = id.split('.').collect();
    if segments.len() < 2 {
        return reject("must have at least two dot-separated segments, e.g. acme.todo");
    }
    for segment in segments {
        if segment.is_empty() {
            return reject("segments must not be empty");
        }
        if !segment
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return reject("segments may contain only a-z, 0-9 and '-'");
        }
        if segment.starts_with('-') || segment.ends_with('-') {
            return reject("segments must not start or end with '-'");
        }
    }
    Ok(())
}

/// The entry is joined to the plugin's own directory, so it must be a plain
/// file name. This is the same instinct as `src-tauri/src/pathsafe.rs`: refuse
/// the shape rather than canonicalize and hope.
fn validate_entry(entry: &str) -> ManifestResult<()> {
    let reject = |reason: &str| {
        Err(ManifestError::Entry {
            entry: entry.to_string(),
            reason: reason.to_string(),
        })
    };
    if entry.is_empty() {
        return reject("must not be empty");
    }
    if entry.contains('/') || entry.contains('\\') {
        return reject("must be a bare file name in the plugin directory");
    }
    if entry.contains("..") {
        return reject("must not contain '..'");
    }
    if !entry.ends_with(".wasm") {
        return reject("must be a .wasm file");
    }
    Ok(())
}

/// A bare hostname. Rejecting schemes and paths here means the allowlist check
/// at call time compares a host to a host, with no parsing ambiguity about
/// what `https://api.acme.com@evil.example.com/` means.
fn validate_net_host(host: &str) -> ManifestResult<()> {
    let reject = |reason: &str| {
        Err(ManifestError::NetHost {
            host: host.to_string(),
            reason: reason.to_string(),
        })
    };
    if host.is_empty() {
        return reject("must not be empty");
    }
    if host.contains('*') {
        return reject("wildcards are not allowed; list each host");
    }
    if host.contains("://") || host.contains('/') {
        return reject("must be a bare hostname, without scheme or path");
    }
    if host.contains('@') || host.contains(':') {
        return reject("must not contain credentials or a port");
    }
    if host != host.to_ascii_lowercase() {
        return reject("must be lowercase");
    }
    if !host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return reject("may contain only a-z, 0-9, '.' and '-'");
    }
    if !host.contains('.') {
        return reject("must be a fully qualified hostname");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The manifest exactly as sketched in docs/plugin-api.md, so the document
    /// and the parser cannot drift apart silently.
    const DOC_EXAMPLE: &str = r#"
id = "acme.todo"
name = "Todo Tracker"
version = "0.1.0"
entry = "plugin.wasm"

[contributes]
commands = [{ id = "acme.todo.add", title = "Add Todo" }]
panels   = [{ id = "acme.todo.panel", title = "Todos", icon = "check" }]

[permissions]
db = "own-tables"
net = ["api.acme.com"]
fs = []
"#;

    #[test]
    fn parses_the_documented_example() {
        let manifest = PluginManifest::parse(DOC_EXAMPLE).expect("doc example is valid");
        assert_eq!(manifest.id, "acme.todo");
        assert_eq!(manifest.entry, "plugin.wasm");
        assert_eq!(manifest.permissions.db, DbAccess::OwnTables);
        assert_eq!(manifest.permissions.net, vec!["api.acme.com"]);
        assert_eq!(manifest.contributes.commands.len(), 1);
        assert_eq!(
            manifest.contributes.panels[0].icon.as_deref(),
            Some("check")
        );
    }

    #[test]
    fn omitted_permissions_grant_nothing() {
        let manifest = PluginManifest::parse(
            r#"
id = "acme.minimal"
name = "Minimal"
version = "0.1.0"
entry = "plugin.wasm"
"#,
        )
        .expect("a manifest may omit permissions entirely");
        assert_eq!(manifest.permissions.db, DbAccess::None);
        assert!(manifest.permissions.net.is_empty());
    }

    #[test]
    fn table_prefix_is_derived_and_always_namespaced() {
        let manifest = PluginManifest::parse(DOC_EXAMPLE).unwrap();
        assert_eq!(manifest.table_prefix(), "plugin_acme_todo_");
        // A plugin cannot derive a prefix that collides with a core module's.
        for core in ["ai_", "index_", "api_", "db_", "monitor"] {
            assert!(!manifest.table_prefix().starts_with(core));
        }
    }

    #[test]
    fn rejects_contributions_outside_the_plugins_own_namespace() {
        let err = PluginManifest::parse(
            r#"
id = "acme.todo"
name = "Todo"
version = "0.1.0"
entry = "plugin.wasm"
[contributes]
commands = [{ id = "git.commit", title = "Commit" }]
"#,
        )
        .unwrap_err();
        assert_eq!(
            err,
            ManifestError::ForeignContribution {
                id: "acme.todo".into(),
                contributed: "git.commit".into(),
            }
        );
    }

    #[test]
    fn rejects_a_panel_hijacking_another_namespace() {
        let err = PluginManifest::parse(
            r#"
id = "acme.todo"
name = "Todo"
version = "0.1.0"
entry = "plugin.wasm"
[contributes]
panels = [{ id = "ai.panel", title = "AI" }]
"#,
        )
        .unwrap_err();
        assert!(matches!(err, ManifestError::ForeignContribution { .. }));
    }

    #[test]
    fn rejects_entry_paths_that_escape_the_plugin_directory() {
        for entry in [
            "../../../windows/system32/evil.wasm",
            "sub/plugin.wasm",
            "sub\\plugin.wasm",
            "plugin.exe",
            "",
        ] {
            // A TOML *literal* string (single quotes) so backslashes reach the
            // validator instead of being rejected earlier as a bad escape.
            let src =
                format!("id = \"acme.todo\"\nname = \"T\"\nversion = \"1\"\nentry = '{entry}'\n");
            assert!(
                matches!(
                    PluginManifest::parse(&src),
                    Err(ManifestError::Entry { .. })
                ),
                "entry {entry:?} should have been rejected"
            );
        }
    }

    #[test]
    fn rejects_bad_ids() {
        for id in ["todo", "", "Acme.Todo", "acme..todo", "acme.todo!", "-a.b"] {
            let src = format!("id = \"{id}\"\nname = \"T\"\nversion = \"1\"\nentry = \"p.wasm\"\n");
            assert!(
                matches!(PluginManifest::parse(&src), Err(ManifestError::Id { .. })),
                "id {id:?} should have been rejected"
            );
        }
    }

    #[test]
    fn rejects_net_entries_that_are_not_bare_hosts() {
        for host in [
            "*",
            "*.acme.com",
            "https://api.acme.com",
            "api.acme.com/v1",
            "api.acme.com:443",
            "user@api.acme.com",
            "API.acme.com",
            "localhost",
        ] {
            let src = format!(
                "id = \"acme.todo\"\nname = \"T\"\nversion = \"1\"\nentry = \"p.wasm\"\n[permissions]\nnet = [\"{host}\"]\n"
            );
            assert!(
                matches!(
                    PluginManifest::parse(&src),
                    Err(ManifestError::NetHost { .. })
                ),
                "net host {host:?} should have been rejected"
            );
        }
    }

    #[test]
    fn filesystem_access_is_refused_loudly_not_ignored() {
        let err = PluginManifest::parse(
            r#"
id = "acme.todo"
name = "T"
version = "1"
entry = "p.wasm"
[permissions]
fs = ["/"]
"#,
        )
        .unwrap_err();
        assert_eq!(err, ManifestError::FsRequested(vec!["/".to_string()]));
    }

    #[test]
    fn unknown_fields_are_a_parse_error() {
        // A typo'd or invented permission key must not be silently dropped —
        // `net_all = true` quietly ignored would read as granted to an author.
        let err = PluginManifest::parse(
            r#"
id = "acme.todo"
name = "T"
version = "1"
entry = "p.wasm"
[permissions]
net_all = true
"#,
        )
        .unwrap_err();
        assert!(matches!(err, ManifestError::Parse(_)));
    }
}
