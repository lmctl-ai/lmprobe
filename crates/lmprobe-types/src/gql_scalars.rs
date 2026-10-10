//! Typed GraphQL scalars (Phase 4E §5.7).
//!
//! These let the schema carry semantics, not just strings. Agents
//! get input validation server-side at the GraphQL boundary; the
//! envelope (D4) still serializes them as plain strings/numbers so
//! the JSON round-trip is unchanged.
//!
//! Per arbitration D the policy is **validate-in-resolver, not
//! parse-time**. Scalar `parse` accepts the syntactic shape (40-hex
//! for ShAs, ≥1 for line numbers, recognised lang label for
//! `Language`); semantic validation that requires context (e.g.
//! "is this a valid ast-grep pattern *for the given language*")
//! happens inside the resolver where the language is known.

// Many scalars are scaffolding for resolvers landing in the next
// commits of Phase 4E-A; `dead_code` is the accurate state until
// they're consumed.
#![allow(dead_code)]

use async_graphql::InputObject;

use async_graphql::{Enum, InputValueError, InputValueResult, Scalar, ScalarType, Value};
use serde::{Deserialize, Serialize};

/// Re-export of the ast-grep-language enum for the Language scalar's
/// `From<SgLang>` / `Into<SgLang>` impls. `lmprobe-types` is the only
/// crate that needs this for its scalar machinery; downstream crates
/// import `SgLang` from `lmprobe-lang-core`.
pub use ast_grep_language::SupportLang as SgLang;

// ───────────────────────── BlobSha / CommitSha ─────────────────────

fn is_40_hex(s: &str) -> bool {
  s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// 40-character hex git blob SHA.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlobSha(pub String);

#[Scalar(name = "BlobSha")]
impl ScalarType for BlobSha {
  fn parse(value: Value) -> InputValueResult<Self> {
    match value {
      Value::String(s) if is_40_hex(&s) => Ok(BlobSha(s)),
      Value::String(s) => Err(InputValueError::custom(format!(
        "BlobSha must be 40 hex chars; got `{}` ({} chars)",
        s,
        s.len()
      ))),
      other => Err(InputValueError::custom(format!(
        "BlobSha expected a String, got {:?}",
        other
      ))),
    }
  }
  fn to_value(&self) -> Value {
    Value::String(self.0.clone())
  }
}

/// 40-character hex git commit SHA.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommitSha(pub String);

#[Scalar(name = "CommitSha")]
impl ScalarType for CommitSha {
  fn parse(value: Value) -> InputValueResult<Self> {
    match value {
      Value::String(s) if is_40_hex(&s) => Ok(CommitSha(s)),
      Value::String(s) => Err(InputValueError::custom(format!(
        "CommitSha must be 40 hex chars; got `{}` ({} chars)",
        s,
        s.len()
      ))),
      other => Err(InputValueError::custom(format!(
        "CommitSha expected a String, got {:?}",
        other
      ))),
    }
  }
  fn to_value(&self) -> Value {
    Value::String(self.0.clone())
  }
}

// ───────────────────────── LineNumber ──────────────────────────────

/// 1-based line number. Parse-time validation rejects 0 and negatives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LineNumber(pub u32);

#[Scalar(name = "LineNumber")]
impl ScalarType for LineNumber {
  fn parse(value: Value) -> InputValueResult<Self> {
    match value {
      Value::Number(n) => {
        let i = n.as_u64().ok_or_else(|| {
          InputValueError::custom(format!("LineNumber must be a positive integer; got {}", n))
        })?;
        if i == 0 || i > u32::MAX as u64 {
          Err(InputValueError::custom(format!(
            "LineNumber out of range (1..=u32::MAX); got {}",
            i
          )))
        } else {
          Ok(LineNumber(i as u32))
        }
      }
      other => Err(InputValueError::custom(format!(
        "LineNumber expected an integer ≥ 1, got {:?}",
        other
      ))),
    }
  }
  fn to_value(&self) -> Value {
    Value::Number(self.0.into())
  }
}

// ───────────────────────── FilePath ────────────────────────────────

/// Repository-relative or absolute file path. Stored as `String`;
/// kept as a typed scalar so the schema documents the semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FilePath(pub String);

#[Scalar(name = "FilePath")]
impl ScalarType for FilePath {
  fn parse(value: Value) -> InputValueResult<Self> {
    match value {
      Value::String(s) if !s.is_empty() => Ok(FilePath(s)),
      Value::String(_) => Err(InputValueError::custom("FilePath must be non-empty")),
      other => Err(InputValueError::custom(format!(
        "FilePath expected a String, got {:?}",
        other
      ))),
    }
  }
  fn to_value(&self) -> Value {
    Value::String(self.0.clone())
  }
}

// ───────────────────────── Pattern ─────────────────────────────────

/// Ast-grep pattern source. Validation is deferred to the resolver
/// (per arbitration D) since `Pattern::try_new(&str, lang)` requires
/// the language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Pattern(pub String);

#[Scalar(name = "Pattern")]
impl ScalarType for Pattern {
  fn parse(value: Value) -> InputValueResult<Self> {
    match value {
      Value::String(s) if !s.is_empty() => Ok(Pattern(s)),
      Value::String(_) => Err(InputValueError::custom("Pattern must be non-empty")),
      other => Err(InputValueError::custom(format!(
        "Pattern expected a String, got {:?}",
        other
      ))),
    }
  }
  fn to_value(&self) -> Value {
    Value::String(self.0.clone())
  }
}

// ───────────────────────── Language ────────────────────────────────

/// Supported language enum. Mirrors `SgLang` (`SupportLang` from
/// `ast_grep_language`); the GraphQL schema uses SCREAMING_SNAKE_CASE
/// per convention. Convert with `lang.into()` (Language → SgLang).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
#[allow(clippy::upper_case_acronyms)]
pub enum Language {
  Bash,
  C,
  Cpp,
  CSharp,
  Css,
  Dart,
  Elixir,
  Go,
  Haskell,
  Hcl,
  Html,
  Java,
  JavaScript,
  Json,
  Kotlin,
  Lua,
  Nix,
  Php,
  Python,
  Ruby,
  Rust,
  Scala,
  Solidity,
  Swift,
  Toml,
  Tsx,
  TypeScript,
  Xml,
  Yaml,
}

impl From<Language> for SgLang {
  fn from(l: Language) -> Self {
    match l {
      Language::Bash => SgLang::Bash,
      Language::C => SgLang::C,
      Language::Cpp => SgLang::Cpp,
      Language::CSharp => SgLang::CSharp,
      Language::Css => SgLang::Css,
      Language::Dart => SgLang::Dart,
      Language::Elixir => SgLang::Elixir,
      Language::Go => SgLang::Go,
      Language::Haskell => SgLang::Haskell,
      Language::Hcl => SgLang::Hcl,
      Language::Html => SgLang::Html,
      Language::Java => SgLang::Java,
      Language::JavaScript => SgLang::JavaScript,
      Language::Json => SgLang::Json,
      Language::Kotlin => SgLang::Kotlin,
      Language::Lua => SgLang::Lua,
      Language::Nix => SgLang::Nix,
      Language::Php => SgLang::Php,
      Language::Python => SgLang::Python,
      Language::Ruby => SgLang::Ruby,
      Language::Rust => SgLang::Rust,
      Language::Scala => SgLang::Scala,
      Language::Solidity => SgLang::Solidity,
      Language::Swift => SgLang::Swift,
      Language::Toml => SgLang::Toml,
      Language::Tsx => SgLang::Tsx,
      Language::TypeScript => SgLang::TypeScript,
      Language::Xml => SgLang::Xml,
      Language::Yaml => SgLang::Yaml,
    }
  }
}

impl From<SgLang> for Language {
  fn from(l: SgLang) -> Self {
    match l {
      SgLang::Bash => Language::Bash,
      SgLang::C => Language::C,
      SgLang::Cpp => Language::Cpp,
      SgLang::CSharp => Language::CSharp,
      SgLang::Css => Language::Css,
      SgLang::Dart => Language::Dart,
      SgLang::Elixir => Language::Elixir,
      SgLang::Go => Language::Go,
      SgLang::Haskell => Language::Haskell,
      SgLang::Hcl => Language::Hcl,
      SgLang::Html => Language::Html,
      SgLang::Java => Language::Java,
      SgLang::JavaScript => Language::JavaScript,
      SgLang::Json => Language::Json,
      SgLang::Kotlin => Language::Kotlin,
      SgLang::Lua => Language::Lua,
      SgLang::Nix => Language::Nix,
      SgLang::Php => Language::Php,
      SgLang::Python => Language::Python,
      SgLang::Ruby => Language::Ruby,
      SgLang::Rust => Language::Rust,
      SgLang::Scala => Language::Scala,
      SgLang::Solidity => Language::Solidity,
      SgLang::Swift => Language::Swift,
      SgLang::Toml => Language::Toml,
      SgLang::Tsx => Language::Tsx,
      SgLang::TypeScript => Language::TypeScript,
      SgLang::Xml => Language::Xml,
      SgLang::Yaml => Language::Yaml,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use async_graphql::ScalarType;

  #[test]
  fn blob_sha_accepts_40_hex() {
    let v = Value::String("a".repeat(40));
    BlobSha::parse(v).unwrap();
  }

  #[test]
  fn blob_sha_rejects_short() {
    let v = Value::String("a".repeat(8));
    assert!(BlobSha::parse(v).is_err());
  }

  #[test]
  fn line_number_rejects_zero() {
    let v = Value::Number(0u32.into());
    assert!(LineNumber::parse(v).is_err());
  }

  #[test]
  fn line_number_accepts_one() {
    let v = Value::Number(1u32.into());
    assert_eq!(LineNumber::parse(v).unwrap().0, 1);
  }

  #[test]
  fn language_round_trip() {
    for sg in SgLang::all_langs() {
      let lang: Language = (*sg).into();
      let back: SgLang = lang.into();
      assert_eq!(*sg, back);
    }
  }
}

// ───────────────────────── Mutation inputs ─────────────────────────
//
// Phase 4E §6 — input shapes for the `fix`, `trace`, and `untrace`
// mutations. The full design correction §6.1 ScopeInput surface has
// 8 alternatives (`pattern`, `symbol`, `callers`, `callees`,
// `decorated`, `raw`, `union`, `intersection`, `exclusion`); 4E-C
// ships `pattern` + `symbol` + `paths` and reserves the rest as
// schema fields that resolvers reject with NOT_IMPLEMENTED until
// the materialization layer lands.

/// Ast-grep pattern + language pre-filter inside a `ScopeInput`.
/// `p` is reserved for a future scope-pre-materialization step
/// (currently informational); `lang` filters the walker.
#[derive(Debug, Clone, InputObject)]
pub struct PatternQuery {
  /// Ast-grep pattern source. Reserved for a future
  /// pre-materialization step that uses the pattern to narrow
  /// the rewrite set before the top-level mutation pattern fires;
  /// today the resolver passes it through informationally.
  pub p: Option<String>,
  /// Language hint for the walker.
  pub lang: Option<Language>,
}

/// A specific symbol identity inside a `ScopeInput`. Mirrors the
/// design correction §6.1 shape; lmprobe currently uses only
/// `name` + `lang`; the rest are recorded for forward-compat.
#[derive(Debug, Clone, InputObject)]
pub struct SymbolRef {
  pub name: String,
  pub kind: Option<String>,
  pub file: Option<FilePath>,
  pub line_start: Option<LineNumber>,
  pub lang: Option<Language>,
}

/// Mutation scope (`fix` / `trace`). Per §6.1 oneof semantics:
/// resolvers must validate that exactly one of the variant fields
/// (`pattern`, `symbol`, `callers`, `callees`, `decorated`, `raw`,
/// `union`, `intersection`, `exclusion`) is set. `paths` is shared
/// across variants and constrains the walker root(s).
///
/// 4E-C ships only `pattern` + `symbol`; the remaining seven
/// variants are recorded in the schema for forward-compat and
/// reject with `NOT_IMPLEMENTED` at resolver time.
#[derive(Debug, Clone, InputObject)]
pub struct ScopeInput {
  /// AST-grep pattern + language pre-filter.
  pub pattern: Option<PatternQuery>,
  /// Specific symbol identity.
  pub symbol: Option<SymbolRef>,
  /// (Reserved) Materialize via `callers` query. NOT_IMPLEMENTED.
  pub callers: Option<NotImplementedScope>,
  /// (Reserved) Materialize via `callees` query. NOT_IMPLEMENTED.
  pub callees: Option<NotImplementedScope>,
  /// (Reserved) All symbols decorated with X. NOT_IMPLEMENTED.
  pub decorated: Option<NotImplementedScope>,
  /// (Reserved) Textual scope. NOT_IMPLEMENTED.
  pub raw: Option<NotImplementedScope>,
  /// (Reserved) Union of multiple scopes. NOT_IMPLEMENTED.
  pub union: Option<Vec<NotImplementedScope>>,
  /// (Reserved) Intersection. NOT_IMPLEMENTED.
  pub intersection: Option<Vec<NotImplementedScope>>,
  /// (Reserved) Scope minus scope. NOT_IMPLEMENTED.
  pub exclusion: Option<NotImplementedScope>,
  /// Filesystem path(s) to walk under. Defaults to `["."]`.
  pub paths: Option<Vec<String>>,
}

/// Placeholder InputObject for scope variants that don't ship in
/// 4E-C. Carries no fields beyond a marker; resolvers detect it
/// and emit `NOT_IMPLEMENTED`.
#[derive(Debug, Clone, InputObject)]
pub struct NotImplementedScope {
  /// Set to true to acknowledge this variant is reserved.
  pub reserved: Option<bool>,
}

/// Trace template per language. 4E-C uses `default` only; per-
/// language overrides land alongside multi-lang `trace` in the
/// post-1.0 surface.
#[derive(Debug, Clone, InputObject)]
pub struct TraceTemplate {
  /// Default template fired when no per-language override matches.
  pub default: Option<String>,
  /// Language-specific override (Rust). 4E-C: reserved.
  pub rust: Option<String>,
  /// Language-specific override (Python). 4E-C: reserved.
  pub python: Option<String>,
  /// Language-specific override (Go). 4E-C: reserved.
  pub go: Option<String>,
  /// Language-specific override (TypeScript). 4E-C: reserved.
  pub typescript: Option<String>,
}

/// Outcome of `ScopeInput` validation.
pub enum ScopeValidation {
  /// Exactly one variant is set. The string is the variant name
  /// for diagnostics ("pattern", "symbol", …).
  Ok(&'static str),
  /// No variant is set.
  Missing,
  /// More than one variant is set; the names are the offending fields.
  Ambiguous(Vec<&'static str>),
  /// Variant is set but reserved for post-1.0; the string names it.
  NotImplemented(&'static str),
}

impl ScopeInput {
  /// Validate that exactly one variant is set per §6.1 oneof
  /// semantics. The reserved variants surface as
  /// `NotImplemented(<name>)` so resolvers can emit the correct
  /// diagnostic code.
  pub fn validate_variant(&self) -> ScopeValidation {
    let mut set: Vec<&'static str> = Vec::new();
    let mut reserved: Option<&'static str> = None;
    if self.pattern.is_some() {
      set.push("pattern");
    }
    if self.symbol.is_some() {
      set.push("symbol");
    }
    if self.callers.is_some() {
      set.push("callers");
      reserved = Some("callers");
    }
    if self.callees.is_some() {
      set.push("callees");
      reserved = Some("callees");
    }
    if self.decorated.is_some() {
      set.push("decorated");
      reserved = Some("decorated");
    }
    if self.raw.is_some() {
      set.push("raw");
      reserved = Some("raw");
    }
    if self.union.is_some() {
      set.push("union");
      reserved = Some("union");
    }
    if self.intersection.is_some() {
      set.push("intersection");
      reserved = Some("intersection");
    }
    if self.exclusion.is_some() {
      set.push("exclusion");
      reserved = Some("exclusion");
    }
    match set.len() {
      0 => ScopeValidation::Missing,
      1 => match reserved {
        Some(name) => ScopeValidation::NotImplemented(name),
        None => ScopeValidation::Ok(set[0]),
      },
      _ => ScopeValidation::Ambiguous(set),
    }
  }

  /// Resolve `paths[0]` (or `.` default) into a `Path`. 4E-C does
  /// not support multi-path walks under one mutation; subsequent
  /// paths are dropped with a `MULTI_PATH_DROPPED` diagnostic
  /// (caller responsibility).
  pub fn primary_path(&self) -> std::path::PathBuf {
    self
      .paths
      .as_ref()
      .and_then(|v| v.first().cloned())
      .map(std::path::PathBuf::from)
      .unwrap_or_else(|| std::path::PathBuf::from("."))
  }

  /// Number of paths beyond the first that the resolver should
  /// surface as a diagnostic.
  pub fn extra_paths(&self) -> usize {
    self
      .paths
      .as_ref()
      .map(|v| v.len().saturating_sub(1))
      .unwrap_or(0)
  }
}

/// Marker validation regex per arbitration F: `[a-zA-Z0-9_-]{1,64}`.
/// Returns Ok(()) when valid; Err(reason) otherwise.
pub fn validate_marker(marker: &str) -> Result<(), String> {
  if marker.is_empty() {
    return Err("marker must be non-empty".into());
  }
  if marker.len() > 64 {
    return Err(format!("marker exceeds 64 chars (got {})", marker.len()));
  }
  if !marker
    .chars()
    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
  {
    return Err(format!(
      "marker `{}` must match /[a-zA-Z0-9_-]+/ (got non-conforming char)",
      marker
    ));
  }
  Ok(())
}

#[cfg(test)]
mod input_tests {
  use super::*;

  fn empty_scope() -> ScopeInput {
    ScopeInput {
      pattern: None,
      symbol: None,
      callers: None,
      callees: None,
      decorated: None,
      raw: None,
      union: None,
      intersection: None,
      exclusion: None,
      paths: None,
    }
  }

  #[test]
  fn scope_missing_when_empty() {
    assert!(matches!(
      empty_scope().validate_variant(),
      ScopeValidation::Missing
    ));
  }

  #[test]
  fn scope_ok_with_pattern() {
    let mut s = empty_scope();
    s.pattern = Some(PatternQuery {
      p: Some("println!($A)".into()),
      lang: None,
    });
    assert!(matches!(
      s.validate_variant(),
      ScopeValidation::Ok("pattern")
    ));
  }

  #[test]
  fn scope_ambiguous_with_two() {
    let mut s = empty_scope();
    s.pattern = Some(PatternQuery {
      p: None,
      lang: None,
    });
    s.symbol = Some(SymbolRef {
      name: "foo".into(),
      kind: None,
      file: None,
      line_start: None,
      lang: None,
    });
    match s.validate_variant() {
      ScopeValidation::Ambiguous(names) => {
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"pattern"));
        assert!(names.contains(&"symbol"));
      }
      other => panic!(
        "expected Ambiguous, got {:?}",
        std::mem::discriminant(&other)
      ),
    }
  }

  #[test]
  fn scope_not_implemented_for_reserved() {
    let mut s = empty_scope();
    s.callers = Some(NotImplementedScope {
      reserved: Some(true),
    });
    assert!(matches!(
      s.validate_variant(),
      ScopeValidation::NotImplemented("callers")
    ));
  }

  #[test]
  fn marker_accepts_alnum_dash_underscore() {
    validate_marker("perf-test_42").unwrap();
    validate_marker("a").unwrap();
  }

  #[test]
  fn marker_rejects_empty() {
    assert!(validate_marker("").is_err());
  }

  #[test]
  fn marker_rejects_too_long() {
    let s = "a".repeat(65);
    assert!(validate_marker(&s).is_err());
  }

  #[test]
  fn marker_rejects_special() {
    assert!(validate_marker("with space").is_err());
    assert!(validate_marker("dot.dot").is_err());
  }
}
