//! Tree-sitter symbol extraction: the names of the things a file *defines*.
//!
//! ## Language scope, and why it stops where it does
//!
//! Every grammar is a multi-megabyte generated C file recompiled on every
//! clean build, so the set is chosen from what DevOS and its users actually
//! contain rather than from what tree-sitter offers:
//!
//! - **Rust** — the whole backend, every crate under `crates/`.
//! - **TypeScript** and **TSX** — the whole frontend.
//!   `tree-sitter-typescript` ships both parsers in one crate and its
//!   `build.rs` compiles both either way, so adding TSX on top of TypeScript
//!   costs no extra dependency and no extra compile time.
//! - **JavaScript / JSX** ride the TSX grammar. TypeScript is a syntactic
//!   superset of JavaScript and the TSX variant is the one that also
//!   understands JSX, so `.js`, `.jsx`, `.mjs` and `.cjs` are covered without
//!   a fourth grammar.
//!
//! Python, Go, Java and the rest are deliberately absent: none appear in this
//! repository, and each would add its own generated parser to every build for
//! a feature that is strictly additive. A file with no grammar yields no
//! symbols — [`extract`] returns an empty vector and the file is still
//! indexed lexically exactly as it was before symbols existed.
//!
//! ## Failure is always "no symbols", never an error
//!
//! [`extract`] has no error type on purpose. A missing grammar, a parse that
//! returns nothing, a name span that isn't valid text — each produces zero
//! symbols for that file, which is precisely the pre-symbol behaviour.
//! Malformed source is not even a special case: tree-sitter recovers around
//! `ERROR` nodes, so a file with a syntax error still contributes whatever
//! definitions it *could* be parsed into.

use std::path::Path;

use tree_sitter::{Language, Node, Parser};

/// Ceiling on symbols kept from one file. A generated or minified file can
/// otherwise contribute tens of thousands of rows that no one will ever
/// search for.
const MAX_SYMBOLS_PER_FILE: usize = 2_000;
/// Longer than any real identifier; a longer "name" means the span came from
/// error recovery, not from a declaration.
const MAX_NAME_LEN: usize = 128;

/// What a symbol is, as far as ranking cares. Stored as the string form so
/// the column stays readable in a SQLite browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Trait,
    Interface,
    Type,
}

impl SymbolKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Class => "class",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Trait => "trait",
            SymbolKind::Interface => "interface",
            SymbolKind::Type => "type",
        }
    }
}

/// One definition found in a file. `start_line` is 1-based, matching the
/// convention `index_chunks` already uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub start_line: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Grammar {
    Rust,
    TypeScript,
    Tsx,
}

impl Grammar {
    fn language(self) -> Language {
        match self {
            Grammar::Rust => tree_sitter_rust::LANGUAGE.into(),
            Grammar::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Grammar::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }
}

/// Grammar for a path, by extension. `None` — the answer for most files in
/// most projects — is the graceful path, not a failure.
fn grammar_for(path: &Path) -> Option<Grammar> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "rs" => Some(Grammar::Rust),
        // `.ts` uses the TypeScript grammar rather than TSX because only it
        // parses the `<T>value` cast form, which TSX must read as JSX.
        "ts" | "mts" | "cts" => Some(Grammar::TypeScript),
        "tsx" | "jsx" | "js" | "mjs" | "cjs" => Some(Grammar::Tsx),
        _ => None,
    }
}

/// True when this crate can extract symbols from `path` at all. Search uses
/// it for nothing; it exists so callers and tests can state the boundary.
pub fn is_supported(path: &Path) -> bool {
    grammar_for(path).is_some()
}

/// Definitions in `source`, in document order. Empty for an unsupported
/// extension, an unusable grammar, or a parse that produced nothing.
pub fn extract(path: &Path, source: &str) -> Vec<Symbol> {
    let Some(grammar) = grammar_for(path) else {
        return Vec::new();
    };
    let mut parser = Parser::new();
    if parser.set_language(&grammar.language()).is_err() {
        // An ABI mismatch between the core and a grammar. Nothing the user
        // can do and nothing worth failing an index run over.
        tracing::warn!(?grammar, "tree-sitter grammar could not be loaded");
        return Vec::new();
    }
    note_parse(path);
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    collect(tree.root_node(), source.as_bytes(), grammar, &mut out);
    out
}

/// Iterative depth-first walk. Iterative rather than recursive because the
/// depth of a syntax tree is bounded by the source, not by us — a deeply
/// nested expression in a 512 KB file must not overflow the stack.
fn collect(root: Node<'_>, source: &[u8], grammar: Grammar, out: &mut Vec<Symbol>) {
    let mut stack: Vec<(Node<'_>, bool)> = vec![(root, false)];
    while let Some((node, in_impl)) = stack.pop() {
        if out.len() >= MAX_SYMBOLS_PER_FILE {
            return;
        }
        if let Some(symbol) = symbol_at(node, source, grammar, in_impl) {
            out.push(symbol);
        }
        // Rust has no distinct node kind for a method: the same
        // `function_item` is a method when an `impl`/`trait` encloses it and
        // a free function otherwise, so the walk carries that context down.
        let child_in_impl = match node.kind() {
            "impl_item" | "trait_item" => true,
            // A `fn` declared inside another `fn`'s body is free again.
            "function_item" => false,
            _ => in_impl,
        };
        // Pushed in reverse so popping visits children in document order.
        for index in (0..node.named_child_count()).rev() {
            if let Some(child) = node.named_child(index) {
                stack.push((child, child_in_impl));
            }
        }
    }
}

fn symbol_at(node: Node<'_>, source: &[u8], grammar: Grammar, in_impl: bool) -> Option<Symbol> {
    match grammar {
        Grammar::Rust => named_symbol(node, source, rust_kind(node.kind(), in_impl)?),
        Grammar::TypeScript | Grammar::Tsx => {
            if node.kind() == "variable_declarator" {
                ts_variable_symbol(node, source)
            } else {
                named_symbol(node, source, ts_kind(node.kind())?)
            }
        }
    }
}

fn rust_kind(node_kind: &str, in_impl: bool) -> Option<SymbolKind> {
    Some(match node_kind {
        // A body-less `fn` is a separate node kind: it is what a `trait`
        // declares and what an `extern` block imports.
        "function_item" | "function_signature_item" if in_impl => SymbolKind::Method,
        "function_item" | "function_signature_item" => SymbolKind::Function,
        "struct_item" => SymbolKind::Struct,
        "enum_item" => SymbolKind::Enum,
        "trait_item" => SymbolKind::Trait,
        "type_item" => SymbolKind::Type,
        _ => return None,
    })
}

fn ts_kind(node_kind: &str) -> Option<SymbolKind> {
    Some(match node_kind {
        "function_declaration" | "generator_function_declaration" | "function_signature" => {
            SymbolKind::Function
        }
        "method_definition" | "method_signature" | "abstract_method_signature" => {
            SymbolKind::Method
        }
        "class_declaration" | "abstract_class_declaration" => SymbolKind::Class,
        "interface_declaration" => SymbolKind::Interface,
        "type_alias_declaration" => SymbolKind::Type,
        "enum_declaration" => SymbolKind::Enum,
        _ => return None,
    })
}

/// `const useThing = () => …` — how most of this codebase's TypeScript
/// functions are actually declared. Skipping it would mean extracting close
/// to nothing from a React file, so the declarator counts as a function
/// whenever its initializer is one.
fn ts_variable_symbol(node: Node<'_>, source: &[u8]) -> Option<Symbol> {
    let value = node.child_by_field_name("value")?;
    if !matches!(
        value.kind(),
        "arrow_function" | "function_expression" | "generator_function"
    ) {
        return None;
    }
    // Destructuring (`const { a } = …`) binds a pattern, not a name.
    if node.child_by_field_name("name")?.kind() != "identifier" {
        return None;
    }
    named_symbol(node, source, SymbolKind::Function)
}

/// Read the `name` field and reject anything that cannot be an identifier.
/// Error recovery can hand back spans covering half a file, and those must
/// not become searchable "symbols".
fn named_symbol(node: Node<'_>, source: &[u8], kind: SymbolKind) -> Option<Symbol> {
    let name = node.child_by_field_name("name")?.utf8_text(source).ok()?;
    if name.is_empty() || name.len() > MAX_NAME_LEN || name.chars().any(char::is_whitespace) {
        return None;
    }
    Some(Symbol {
        name: name.to_string(),
        kind,
        start_line: node.start_position().row as i64 + 1,
    })
}

// ---- parse accounting (tests only) ----
//
// The incremental guarantee — an unchanged file is never re-parsed — is only
// meaningful if it can be measured, and counting parses is the only direct
// way to measure it. The counter is keyed by path so tests running in
// parallel against their own `tempdir` cannot interfere with each other, and
// it compiles out entirely in a release build.

#[cfg(not(test))]
fn note_parse(_path: &Path) {}

#[cfg(test)]
fn note_parse(path: &Path) {
    *parse_log()
        .lock()
        .expect("parse log poisoned")
        .entry(log_key(path))
        .or_insert(0) += 1;
}

#[cfg(test)]
fn log_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
fn parse_log() -> &'static std::sync::Mutex<std::collections::HashMap<String, usize>> {
    static LOG: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, usize>>> =
        std::sync::OnceLock::new();
    LOG.get_or_init(Default::default)
}

/// How many times tree-sitter has been handed the contents of `path`.
#[cfg(test)]
pub(crate) fn parse_count(path: &Path) -> usize {
    parse_log()
        .lock()
        .expect("parse log poisoned")
        .get(&log_key(path))
        .copied()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn names(symbols: &[Symbol]) -> Vec<(&str, &str)> {
        symbols
            .iter()
            .map(|s| (s.name.as_str(), s.kind.as_str()))
            .collect()
    }

    fn find<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
        symbols
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("no symbol named {name} in {:?}", names(symbols)))
    }

    #[test]
    fn rust_declarations_including_methods_and_trait_items() {
        let source = r#"
use std::fmt;

pub struct TokenStore {
    inner: Vec<String>,
}

pub enum Grant { Read, Write }

pub trait Verifier {
    fn verify(&self, token: &str) -> bool;
}

pub type TokenId = u64;

impl TokenStore {
    pub fn new() -> Self {
        fn nested_helper() -> u8 { 1 }
        Self { inner: Vec::new() }
    }

    pub fn revoke_token(&mut self, id: TokenId) -> bool {
        let _ = id;
        false
    }
}

pub fn free_function(x: i32) -> i32 { x }
"#;
        let symbols = extract(&PathBuf::from("store.rs"), source);
        let found = names(&symbols);

        assert!(found.contains(&("TokenStore", "struct")), "{found:?}");
        assert!(found.contains(&("Grant", "enum")), "{found:?}");
        assert!(found.contains(&("Verifier", "trait")), "{found:?}");
        assert!(found.contains(&("TokenId", "type")), "{found:?}");
        assert!(found.contains(&("free_function", "function")), "{found:?}");

        // The load-bearing case: functions inside an `impl` are methods, and
        // a bare signature inside a `trait` is one too.
        assert!(found.contains(&("new", "method")), "{found:?}");
        assert!(found.contains(&("revoke_token", "method")), "{found:?}");
        assert!(found.contains(&("verify", "method")), "{found:?}");
        // ...but a `fn` nested in a method body is a free function again.
        assert!(found.contains(&("nested_helper", "function")), "{found:?}");

        // Lines are 1-based and point at the declaration, not the file.
        assert_eq!(find(&symbols, "free_function").start_line, 28);
        assert_eq!(find(&symbols, "TokenStore").start_line, 4);
    }

    #[test]
    fn typescript_declarations_including_class_methods() {
        let source = r#"export interface IndexHit {
  file: string;
}

export type Ranking = "lexical" | "semantic";

export enum Leg { Lexical, Semantic }

export class SearchClient {
  constructor(private url: string) {}

  async runSearch(query: string): Promise<IndexHit[]> {
    return [];
  }
}

export function fuseRankings(a: number[], b: number[]): number[] {
  return [...a, ...b];
}

export const useIndexSearch = (query: string) => {
  return query.trim();
};

const notAFunction = 42;
"#;
        let symbols = extract(&PathBuf::from("search.ts"), source);
        let found = names(&symbols);

        assert!(found.contains(&("IndexHit", "interface")), "{found:?}");
        assert!(found.contains(&("Ranking", "type")), "{found:?}");
        assert!(found.contains(&("Leg", "enum")), "{found:?}");
        assert!(found.contains(&("SearchClient", "class")), "{found:?}");
        assert!(found.contains(&("fuseRankings", "function")), "{found:?}");
        // A method inside a class, not just top-level declarations.
        assert!(found.contains(&("runSearch", "method")), "{found:?}");
        // The arrow-function const form, which is most of a React codebase.
        assert!(found.contains(&("useIndexSearch", "function")), "{found:?}");
        // A plain value binding is not a symbol.
        assert!(
            !found.iter().any(|(name, _)| *name == "notAFunction"),
            "{found:?}"
        );
        assert_eq!(find(&symbols, "runSearch").start_line, 12);
    }

    #[test]
    fn tsx_and_js_ride_the_same_grammar() {
        let tsx = r#"import { useState } from "react";

export function SearchPanel({ query }: { query: string }) {
  const [open, setOpen] = useState(false);
  return <div onClick={() => setOpen(!open)}>{query}</div>;
}

export const ResultRow = ({ hit }: { hit: string }) => <li>{hit}</li>;
"#;
        let tsx_symbols = extract(&PathBuf::from("SearchPanel.tsx"), tsx);
        let found = names(&tsx_symbols);
        assert!(found.contains(&("SearchPanel", "function")), "{found:?}");
        assert!(found.contains(&("ResultRow", "function")), "{found:?}");

        // Plain JavaScript, including JSX, goes through the TSX grammar
        // rather than a fourth dependency.
        let js = "export function legacyHelper(a) { return a; }\nclass Legacy { run() {} }\n";
        let js_symbols = extract(&PathBuf::from("legacy.js"), js);
        let found = names(&js_symbols);
        assert!(found.contains(&("legacyHelper", "function")), "{found:?}");
        assert!(found.contains(&("Legacy", "class")), "{found:?}");
        assert!(found.contains(&("run", "method")), "{found:?}");
    }

    #[test]
    fn unsupported_extensions_yield_nothing_and_never_parse() {
        for name in ["notes.md", "config.toml", "data.json", "Makefile", "x.py"] {
            let path = PathBuf::from(name);
            assert!(!is_supported(&path), "{name} must have no grammar");
            assert!(extract(&path, "fn main() {}\nclass Foo:\n  pass\n").is_empty());
            assert_eq!(parse_count(&path), 0, "{name} must not be parsed at all");
        }
    }

    /// Genuinely broken source, not merely unusual source. tree-sitter
    /// recovers instead of failing, so the surviving declarations are still
    /// extracted and the caller never sees an error.
    #[test]
    fn malformed_source_degrades_instead_of_failing() {
        let broken = r#"
fn good_one() -> u8 { 1 }

fn (((  unterminated <<< ,,, {{{

impl impl impl for for {

struct 42Nonsense }}}

pub fn survivor(x: i32) -> i32 { x }
"#;
        let symbols = extract(&PathBuf::from("broken.rs"), broken);
        let found = names(&symbols);
        assert!(
            found.contains(&("good_one", "function")),
            "declarations before the damage survive: {found:?}"
        );
        // Whatever else recovery produced, nothing may be a non-identifier.
        for symbol in &symbols {
            assert!(!symbol.name.is_empty());
            assert!(!symbol.name.contains(char::is_whitespace), "{symbol:?}");
            assert!(symbol.start_line >= 1);
        }

        // Broken TypeScript is the same story, and an empty file is fine.
        let broken_ts = "export function ( { { const = = =>\nclass {{{";
        assert!(extract(&PathBuf::from("broken.ts"), broken_ts).len() < 5);
        assert!(extract(&PathBuf::from("empty.rs"), "").is_empty());
        assert!(extract(&PathBuf::from("blank.tsx"), "\n\n\n").is_empty());
    }

    #[test]
    fn every_supported_extension_actually_loads_its_grammar() {
        // A grammar that fails `set_language` (an ABI mismatch) returns no
        // symbols silently, which would make the whole feature a no-op with
        // nothing but a log line to show for it.
        for (name, source) in [
            ("a.rs", "fn alpha() {}"),
            ("a.ts", "function alpha() {}"),
            ("a.mts", "function alpha() {}"),
            ("a.cts", "function alpha() {}"),
            ("a.tsx", "function alpha() {}"),
            ("a.jsx", "function alpha() {}"),
            ("a.js", "function alpha() {}"),
            ("a.mjs", "function alpha() {}"),
            ("a.cjs", "function alpha() {}"),
        ] {
            let symbols = extract(&PathBuf::from(name), source);
            assert_eq!(
                names(&symbols),
                vec![("alpha", "function")],
                "{name} produced {symbols:?}"
            );
        }
    }

    #[test]
    fn pathological_files_are_bounded() {
        let many = (0..MAX_SYMBOLS_PER_FILE + 500)
            .map(|i| format!("fn f{i}() {{}}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            extract(&PathBuf::from("huge.rs"), &many).len(),
            MAX_SYMBOLS_PER_FILE
        );

        // Deep nesting must not overflow the stack: the walk is iterative.
        let deep = format!(
            "fn deep() {{ {} 1 {} }}",
            "(".repeat(2_000),
            ")".repeat(2_000)
        );
        assert_eq!(
            names(&extract(&PathBuf::from("deep.rs"), &deep)),
            vec![("deep", "function")]
        );
    }
}
