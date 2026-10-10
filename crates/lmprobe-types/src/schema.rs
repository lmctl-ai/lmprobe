//! Versioned result schemas (D4).
//!
//! Every sensor produces a single `serde::Serialize` shape declared
//! here. CLI markdown / JSON output and GraphQL (`query` / `serve`)
//! render from these shapes — they are the single source of truth.
//!
//! Each envelope carries:
//! - a `schema` string of the form `lmprobe.<sensor>.v<N>` so pinned
//!   GraphQL clients can detect breaking changes;
//! - a `warnings: Vec<Diagnostic>` channel (CC4: schema-first
//!   diagnostics — every stderr note is mirrored here so JSON
//!   consumers see the same signal as a TTY user);
//! - an `exit_status` enum (`matched | no_match | error`) the CLI
//!   maps to its process exit code (§0.7).
//!
//! The `warnings` and `exit_status` fields were added in Phase 0
//! Chunk 0B as additive v1 extensions: old serialized envelopes
//! deserialize cleanly via `#[serde(default)]`. Bumping `vN` is the
//! only allowed way to change a field's *meaning*; new optional
//! fields are additive.

// async-graphql derives are unconditional in 4E — GraphQL is the
// core surface, not opt-in.
use async_graphql::{
  ComplexObject, Context, Enum, InputObject, InputValueError, InputValueResult, Scalar, ScalarType,
  SimpleObject, Value,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::sync::Arc;

fn is_zero(value: &usize) -> bool {
  *value == 0
}

// ─────────────────────────── item shapes ──────────────────────────

/// A symbol or AST-node hit. Used by `search`, `def`, `callers`,
/// `imports`, `decorations`.
///
/// Phase 4E §5.1 composition: `#[graphql(complex)]` opts the type
/// in to a `#[ComplexObject]` impl in `cli.rs` that adds child
/// resolvers (`callers(limit)`, `callees(limit)`, `recentHistory
/// (limit)`, `enclosing`). The serde shape is unchanged — child
/// resolvers don't add serde fields.
#[derive(Debug, Clone, Default, Serialize, Deserialize, SimpleObject)]
#[graphql(name = "SymbolResult", complex)]
pub struct SymbolHit {
  pub name: String,
  pub kind: String,
  pub lang: String,
  pub file_path: String,
  pub line_start: u32,
  pub content: String,
  /// §4.7 — origin root tag when produced under `--workspace`.
  /// `None` for single-root invocations (back-compat). v1-additive.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub origin_root: Option<String>,
  /// §4.D — name of the enclosing declaration this hit lives
  /// inside (e.g. for a `Pattern::new(…)` call site, the
  /// surrounding `fn build(…)`'s name). Faithful AST-walk
  /// up to nearest declaration. v1-additive.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub enclosing_decl: Option<String>,
  /// Phase 4E §4E-C.5 — tree-sitter literal kind of the
  /// enclosing declaration (`"function_item"`, `"class_definition"`,
  /// etc.). Set when `enclosing_decl` is set; populated by the
  /// same `enclosing_decl_name` walk. v1-additive — `None` on
  /// older serialized envelopes deserializes cleanly.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub enclosing_decl_kind: Option<String>,
  /// Optional line-numbered source window requested by commands such as
  /// `def --around`.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub source_context: Vec<LineContext>,
}

/// Phase 4E §5.1 — typed shape returned by the `enclosing` child
/// resolver on `SymbolHit`. `name` mirrors the existing
/// `enclosing_decl: Option<String>` payload populated by the
/// callers walk; `kind` mirrors the §4E-C.5 follow-up
/// `enclosing_decl_kind: Option<String>` field — the tree-sitter
/// literal kind of the containing declaration node.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct EnclosingDecl {
  pub name: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub kind: Option<String>,
}

// ──────────────── SymbolHit child-resolver dispatch ────────────────
//
// Phase 5A workspace split: the `#[ComplexObject]` impl on
// `SymbolHit` must live in the same crate as the SimpleObject
// (orphan rule). The actual sensor work lives in `lmprobe-engine`.
// Bridge: `lmprobe-engine` defines a struct that implements
// `SymbolHitChildResolvers`; the binary registers an `Arc<dyn ...>`
// instance as schema `Context` data; this crate's resolvers fetch
// the impl via `ctx.data::<...>()` and delegate.
//
// `enclosing` is implemented inline because it's pure data
// extraction — no engine call needed.

/// Engine-side child-resolver implementations that the
/// `#[ComplexObject] impl SymbolHit` block delegates to.
#[async_trait::async_trait]
pub trait SymbolHitChildResolvers: Send + Sync {
  async fn callers(
    &self,
    ctx: &Context<'_>,
    hit: &SymbolHit,
    limit: Option<i32>,
    path: Option<String>,
  ) -> std::result::Result<CallersResult, async_graphql::Error>;
  async fn callees(
    &self,
    ctx: &Context<'_>,
    hit: &SymbolHit,
    limit: Option<i32>,
    path: Option<String>,
  ) -> std::result::Result<CalleesResult, async_graphql::Error>;
  async fn recent_history(
    &self,
    ctx: &Context<'_>,
    hit: &SymbolHit,
    limit: Option<i32>,
    path: Option<String>,
  ) -> std::result::Result<BlameResult, async_graphql::Error>;
}

fn child_resolvers<'a>(
  ctx: &'a Context<'_>,
) -> std::result::Result<&'a Arc<dyn SymbolHitChildResolvers>, async_graphql::Error> {
  ctx.data::<Arc<dyn SymbolHitChildResolvers>>().map_err(|_| {
    async_graphql::Error::new("internal: SymbolHit child resolvers not registered on this schema")
  })
}

#[ComplexObject]
impl SymbolHit {
  /// Who calls this hit's symbol? Defaults `limit` to 10 to
  /// keep response payloads bounded for AI agents that select
  /// `recentHistory + callers + decorations` in one round-trip.
  async fn callers(
    &self,
    ctx: &Context<'_>,
    limit: Option<i32>,
    path: Option<String>,
  ) -> std::result::Result<CallersResult, async_graphql::Error> {
    child_resolvers(ctx)?.callers(ctx, self, limit, path).await
  }
  /// What does this hit's symbol call? Same `limit=10` default
  /// rationale as `callers`.
  async fn callees(
    &self,
    ctx: &Context<'_>,
    limit: Option<i32>,
    path: Option<String>,
  ) -> std::result::Result<CalleesResult, async_graphql::Error> {
    child_resolvers(ctx)?.callees(ctx, self, limit, path).await
  }
  /// Last N commits touching this symbol. `limit` truncates
  /// post-walk; the AST-aware classifier needs to see all
  /// entries to determine LOGIC_CHANGE vs FORMATTING, so the
  /// limit is a pure render-time cap, not a walk-bound.
  async fn recent_history(
    &self,
    ctx: &Context<'_>,
    limit: Option<i32>,
    path: Option<String>,
  ) -> std::result::Result<BlameResult, async_graphql::Error> {
    child_resolvers(ctx)?
      .recent_history(ctx, self, limit, path)
      .await
  }
  /// The parent declaration containing this hit. Sourced from
  /// `enclosing_decl` (name) and `enclosing_decl_kind` (kind),
  /// both populated by the `callers` walk and `impact`'s
  /// upstream BFS via `enclosing_decl_name`. Other resolvers
  /// leave both unset and the field is absent from JSON.
  ///
  /// Pure-data resolver — no engine delegation needed.
  async fn enclosing(&self) -> Option<EnclosingDecl> {
    self.enclosing_decl.as_ref().map(|n| EnclosingDecl {
      name: n.clone(),
      kind: self.enclosing_decl_kind.clone(),
    })
  }
}

/// A textual line hit. Used by `grep` and (currently) `ref`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, SimpleObject)]
#[graphql(name = "GrepResult")]
pub struct GrepHit {
  pub file_path: String,
  pub line_number: usize,
  pub line_content: String,
  /// Surrounding source lines requested via `grep -A/-B/-C`.
  /// Empty by default and omitted from JSON to keep compact hits stable.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub before_context: Vec<LineContext>,
  /// Surrounding source lines requested via `grep -A/-B/-C`.
  /// Empty by default and omitted from JSON to keep compact hits stable.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub after_context: Vec<LineContext>,
  /// Enclosing declaration chain requested via `grep --parents`.
  /// Empty by default and omitted from JSON to keep compact hits stable.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub parents: Vec<StructuralParent>,
  /// Innermost enclosing structural body requested via `grep --body`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub body: Option<StructuralBody>,
  /// Mechanical local role requested via `grep --role` / `ref --role`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub role: Option<String>,
  /// §4.7 — origin root tag (workspace fan-out). v1-additive.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub origin_root: Option<String>,
}

/// A source line adjacent to a `GrepHit` when context lines are requested.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct LineContext {
  pub line_number: usize,
  pub line_content: String,
}

/// Enclosing structural declaration metadata requested via `grep --parents`.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct StructuralParent {
  pub kind: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub name: Option<String>,
  pub line_start: usize,
  pub line_end: usize,
}

/// Innermost enclosing structural body metadata requested via `grep --body`.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct StructuralBody {
  pub kind: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub name: Option<String>,
  pub line_start: usize,
  pub line_end: usize,
  pub content: String,
}

/// A new-side changed line produced by `diff-context`.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct DiffContextAnchor {
  pub file_path: String,
  pub line_number: usize,
  pub line_content: String,
  pub anchor_kind: String,
  /// Old-side hunk start from the unified diff header.
  pub old_start: usize,
  /// Old-side hunk length from the unified diff header.
  pub old_lines: usize,
  /// New-side hunk start from the unified diff header.
  pub new_start: usize,
  /// New-side hunk length from the unified diff header.
  pub new_lines: usize,
  /// Optional line-numbered source window requested via `--around`.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub source_context: Vec<LineContext>,
  /// Enclosing declaration chain requested via `--parents`.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub parents: Vec<StructuralParent>,
  /// Innermost enclosing structural body requested via `--body`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub body: Option<StructuralBody>,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct ResourceCallSite {
  pub callee: String,
  pub line_number: usize,
  pub line_content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct ResourceSite {
  pub file_path: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub enclosing: Option<StructuralParent>,
  pub acquire_calls: Vec<ResourceCallSite>,
  pub release_calls: Vec<ResourceCallSite>,
  pub defer_calls: Vec<ResourceCallSite>,
  pub error_branches: Vec<ResourceCallSite>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub acquire_timing: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub release_lifecycle: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub optional_acquire: Option<bool>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub source_context: Vec<LineContext>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub parents: Vec<StructuralParent>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub body: Option<StructuralBody>,
  pub confidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct AsyncLeakSite {
  pub file_path: String,
  pub line_number: usize,
  pub line_content: String,
  pub callee: String,
  pub awaited: bool,
  pub returned: bool,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub source_context: Vec<LineContext>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub parents: Vec<StructuralParent>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub body: Option<StructuralBody>,
  pub confidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct CryptoGuardAnchor {
  pub match_text: String,
  pub line_number: usize,
  pub line_content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct CryptoSite {
  pub file_path: String,
  pub line_number: usize,
  pub line_content: String,
  pub algorithm: String,
  pub match_text: String,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub import_paths: Vec<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub nearby_guards: Vec<CryptoGuardAnchor>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub source_context: Vec<LineContext>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub parents: Vec<StructuralParent>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub body: Option<StructuralBody>,
  pub confidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct ConfigCodeAnchor {
  pub file_path: String,
  pub line_number: usize,
  pub line_content: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub role: Option<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub parents: Vec<StructuralParent>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub source_context: Vec<LineContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct ConfigKeyRecord {
  pub file_path: String,
  pub lang: String,
  pub key_path: String,
  pub key: String,
  pub line_number: usize,
  pub value_kind: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub value_summary: Option<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub code_anchors: Vec<ConfigCodeAnchor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct GuardCallSite {
  pub match_text: String,
  pub line_number: usize,
  pub line_content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct GuardSiteCandidate {
  pub file_path: String,
  pub sensitive: GuardCallSite,
  pub nearby_guards: Vec<GuardCallSite>,
  pub decorations: Vec<GuardCallSite>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub parents: Vec<StructuralParent>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub body: Option<StructuralBody>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub source_context: Vec<LineContext>,
  pub confidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct OpenApiOperationFact {
  pub file_path: String,
  pub method: String,
  pub path: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub operation_id: Option<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub required_params: Vec<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub response_schemas: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct RouteAnchor {
  pub file_path: String,
  pub method: String,
  pub path: String,
  pub line_number: usize,
  pub line_content: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub handler: Option<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub source_context: Vec<LineContext>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub parents: Vec<StructuralParent>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub body: Option<StructuralBody>,
  pub confidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct HttpClientCallAnchor {
  pub file_path: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub method: Option<String>,
  pub url: String,
  pub line_number: usize,
  pub line_content: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub client: Option<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub source_context: Vec<LineContext>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub parents: Vec<StructuralParent>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub body: Option<StructuralBody>,
  pub confidence: String,
}

/// A project-marker discovery record. Used by `discover`.
///
/// `children` is the ordered list of paths of projects nested under
/// this one. Empty for leaves. §2.1 added this field additively
/// (`lmprobe.discover.v1` stays at v1 — `#[serde(default)]` makes
/// pre-2A envelopes deserialize cleanly).
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
#[graphql(name = "ProjectInfo")]
pub struct Project {
  pub name: String,
  pub project_type: String,
  pub version: String,
  pub path: String,
  pub dependencies_count: usize,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub children: Vec<String>,
}

/// A semantic-evolution milestone. Used by `evolution`.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct Milestone {
  pub symbol_name: String,
  pub commit_hash: String,
  pub author: String,
  pub timestamp: String,
  pub change_type: String,
  pub signature: String,
}

/// A single trace event captured during `trace --exec`.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
#[graphql(name = "TraceResult")]
pub struct TraceEvent {
  pub timestamp: String,
  pub output: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub var: Option<String>,
  #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
  pub type_name: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub id: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub len: Option<usize>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub shape: Option<Vec<usize>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub repr: Option<String>,
}

// ─────────────────────────── diagnostics ──────────────────────────

/// Schema-side mirror of a stderr note. Codes are stable identifiers
/// (`NOT_IMPLEMENTED`, `QUOTED_INPUT`, `NO_TRACE_TEMPLATE`,
/// `DEPRECATED_CACHE_PATH`, `PATTERN_PARSE_FAILED`, `DIRTY_TREE`,
/// `NOT_GIT_REPO`, …). New codes can be added without bumping `vN`;
/// renaming or removing a code requires `v2`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, SimpleObject)]
pub struct Diagnostic {
  pub code: String,
  pub message: String,
  pub hint: Option<String>,
  pub lang: Option<String>,
}

impl Diagnostic {
  pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
    Self {
      code: code.into(),
      message: message.into(),
      hint: None,
      lang: None,
    }
  }
  pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
    self.hint = Some(hint.into());
    self
  }
  pub fn with_lang(mut self, lang: impl Into<String>) -> Self {
    self.lang = Some(lang.into());
    self
  }
}

/// CLI exit-status enum. The CLI dispatcher maps this to a process
/// exit code (matched=0, no_match=1, error=2). Some verbs may use
/// a verb-specific primary-path code for additive metadata such as
/// partial coverage; see the verb docs. `--exit-zero-on-
/// no-match` collapses 1 → 0 for callers that don't want to
/// distinguish the two.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, Enum)]
#[serde(rename_all = "snake_case")]
pub enum ExitStatus {
  #[default]
  Matched,
  NoMatch,
  Error,
}

impl ExitStatus {
  /// Heuristic for the common case where a sensor returns a Vec of
  /// hits: empty = `NoMatch`, non-empty = `Matched`.
  pub fn from_count(n: usize) -> Self {
    if n == 0 {
      Self::NoMatch
    } else {
      Self::Matched
    }
  }
}

const REDACTED_SECRET: &str = "REDACTED";

/// Secret material with redaction-safe formatting and serialization.
///
/// Raw access is explicit so scanners can keep plaintext confined to
/// live in-memory decision points; every default output surface emits a
/// redacted placeholder.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Secret(String);

impl Secret {
  pub fn new(raw: impl Into<String>) -> Self {
    Self(raw.into())
  }

  pub fn expose_raw(&self) -> &str {
    &self.0
  }

  pub fn redacted(&self) -> &'static str {
    REDACTED_SECRET
  }
}

impl fmt::Debug for Secret {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(REDACTED_SECRET)
  }
}

impl fmt::Display for Secret {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(REDACTED_SECRET)
  }
}

impl Serialize for Secret {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.serialize_str(REDACTED_SECRET)
  }
}

impl<'de> Deserialize<'de> for Secret {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    String::deserialize(deserializer).map(Self)
  }
}

#[Scalar(name = "Secret")]
impl ScalarType for Secret {
  fn parse(value: Value) -> InputValueResult<Self> {
    if let Value::String(raw) = &value {
      Ok(Self::new(raw.clone()))
    } else {
      Err(InputValueError::expected_type(value))
    }
  }

  fn to_value(&self) -> Value {
    Value::String(REDACTED_SECRET.into())
  }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, Enum)]
#[serde(rename_all = "kebab-case")]
pub enum AstContext {
  Comment,
  StringDoc,
  ImportPath,
  ConstEnumIdentifier,
  TestOrGeneratedPath,
  CodeString,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, SimpleObject)]
pub struct ScanRegion {
  pub file_path: String,
  pub start_line: usize,
  pub end_line: usize,
  pub start_column: usize,
  pub end_column: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, Enum)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticConfidence {
  ConfirmedCredentialFlow,
  BoundedSymbolFlow,
  ProviderSinkPattern,
  SourceEvidence,
  SyntaxEvidenceOnly,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, Enum)]
#[serde(rename_all = "kebab-case")]
pub enum SinkKind {
  MethodCall,
  AttributeAssignment,
  CredentialKwarg,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, SimpleObject)]
pub struct SourceEvidence {
  pub file_path: String,
  pub line: usize,
  pub column: usize,
  pub variable: String,
  pub literal: Secret,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, SimpleObject)]
pub struct SinkEvidence {
  pub file_path: String,
  pub line: usize,
  pub column: usize,
  pub callee_text: String,
  pub arg_position: Option<usize>,
  pub arg_name: Option<String>,
  pub sink_kind: SinkKind,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, Enum)]
#[serde(rename_all = "kebab-case")]
pub enum FlowStepKind {
  Definition,
  Source,
  Export,
  Import,
  Sink,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, SimpleObject)]
pub struct FlowStep {
  pub kind: FlowStepKind,
  pub file_path: String,
  pub line: usize,
  pub column: usize,
  pub symbol: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "origin", content = "value")]
pub enum ActiveCredentialOrigin {
  Literal(Secret),
  Source(CredentialSourceEvidence),
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, SimpleObject)]
#[serde(deny_unknown_fields)]
pub struct CredentialSourceEvidence {
  pub file_path: String,
  pub line: usize,
  pub column: usize,
  pub source_id: String,
  pub source_category: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub provider_id: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub env_var_name: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub key_name: Option<String>,
  pub sdk: String,
  pub variable: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, SimpleObject)]
pub struct SymbolReachabilityFinding {
  pub detector: String,
  pub confidence: SemanticConfidence,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub provider_id: Option<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub provider_rule_ids: Vec<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub credential_kind: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub sdk: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub provider_shape_id: Option<String>,
  pub source: SourceEvidence,
  pub sink: SinkEvidence,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub flow: Vec<FlowStep>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, SimpleObject)]
#[serde(deny_unknown_fields)]
pub struct ConfirmedCredentialFlowFinding {
  pub detector: String,
  pub confidence: SemanticConfidence,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub provider_id: Option<String>,
  pub source: CredentialSourceEvidence,
  pub sink: SinkEvidence,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub intermediate_vars: Vec<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub flow: Vec<FlowStep>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, SimpleObject)]
pub struct SinkEvidenceOnlyFinding {
  pub detector: String,
  pub confidence: SemanticConfidence,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub provider_id: Option<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub provider_rule_ids: Vec<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub credential_kind: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub sdk: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub provider_shape_id: Option<String>,
  pub sink: SinkEvidence,
  pub value_text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, SimpleObject)]
pub struct SecretFinding {
  pub detector: String,
  pub confidence: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub ast_context: Option<AstContext>,
  pub rule_id: String,
  pub description: String,
  pub file_path: String,
  pub line_number: usize,
  pub start_line: usize,
  pub end_line: usize,
  pub start_column: usize,
  pub end_column: usize,
  pub redacted_secret: Secret,
  pub match_text: Secret,
  pub entropy: f64,
  pub tags: Vec<String>,
  pub fingerprint: String,
  pub source_mode: String,
  pub evidence: Vec<ScanRegion>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, SimpleObject)]
pub struct SuppressedSecretFinding {
  #[serde(flatten)]
  pub finding: SecretFinding,
  pub suppressed_reason: AstContext,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, SimpleObject)]
pub struct CanonicalEvidence {
  pub detector: String,
  pub location: ScanRegion,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, SimpleObject)]
pub struct CanonicalFinding {
  pub detector: String,
  pub file_path: String,
  pub line_number: usize,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub rule_id: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub provider_id: Option<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub provider_rule_ids: Vec<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub credential_kind: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub sdk: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub provider_shape_id: Option<String>,
  pub redacted_secret: Secret,
  pub evidence: Vec<CanonicalEvidence>,
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize, SimpleObject)]
pub struct SecretsSummary {
  pub canonical: usize,
  pub layer1: usize,
  pub symbol_reachability: usize,
  #[serde(default, skip_serializing_if = "is_zero")]
  pub source_evidence: usize,
  #[serde(default, skip_serializing_if = "is_zero")]
  pub confirmed_flows: usize,
  pub sink_evidence: usize,
  pub suppressed: usize,
}

pub type SecretDiagnostic = Diagnostic;
pub type SecretsExitStatus = ExitStatus;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, Enum)]
#[serde(rename_all = "snake_case")]
pub enum SecretsSourceInput {
  #[default]
  Filesystem,
  Git,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, Enum)]
#[graphql(name = "AstFpCutMode")]
#[serde(rename_all = "kebab-case")]
pub enum SecretsAstFpCutMode {
  Off,
  #[default]
  Annotate,
  Suppress,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, Enum)]
#[graphql(name = "SinkEvidenceMode")]
#[serde(rename_all = "kebab-case")]
pub enum SecretsSinkEvidenceMode {
  #[default]
  Off,
  On,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct SecretsResult {
  pub schema: String,
  pub findings: Vec<SecretFinding>,
  #[serde(default)]
  pub suppressed: Vec<SuppressedSecretFinding>,
  #[serde(default)]
  pub ast_files_parsed: usize,
  #[serde(default)]
  pub semantic_findings: Vec<SymbolReachabilityFinding>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub source_evidence: Vec<CredentialSourceEvidence>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub confirmed_flows: Vec<ConfirmedCredentialFlowFinding>,
  #[serde(default)]
  pub sink_evidence: Vec<SinkEvidenceOnlyFinding>,
  #[serde(default)]
  pub canonical_findings: Vec<CanonicalFinding>,
  #[serde(default)]
  pub summary: SecretsSummary,
  pub sink_catalog_version: String,
  #[serde(default)]
  pub warnings: Vec<Diagnostic>,
  #[serde(default, skip_serializing_if = "is_false")]
  pub partial: bool,
  #[serde(default)]
  pub exit_status: ExitStatus,
}

impl SecretsResult {
  pub const SCHEMA: &'static str = "lmprobe.secrets.v1";

  pub fn new(findings: Vec<SecretFinding>) -> Self {
    Self::with_sink_catalog_version(findings, "")
  }

  pub fn with_sink_catalog_version(
    findings: Vec<SecretFinding>,
    sink_catalog_version: impl Into<String>,
  ) -> Self {
    let exit_status = ExitStatus::from_count(findings.len());
    Self {
      schema: Self::SCHEMA.into(),
      findings,
      suppressed: Vec::new(),
      ast_files_parsed: 0,
      semantic_findings: Vec::new(),
      source_evidence: Vec::new(),
      confirmed_flows: Vec::new(),
      sink_evidence: Vec::new(),
      canonical_findings: Vec::new(),
      summary: SecretsSummary::default(),
      sink_catalog_version: sink_catalog_version.into(),
      warnings: Vec::new(),
      partial: false,
      exit_status,
    }
  }

  pub fn push_warning(&mut self, warning: Diagnostic) {
    self.warnings.push(warning);
  }

  pub fn rebuild_canonical_findings(&mut self) {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<(String, usize, String), CanonicalFinding> = BTreeMap::new();
    for finding in &self.semantic_findings {
      let key = (
        finding.sink.file_path.clone(),
        finding.sink.line,
        finding.source.literal.expose_raw().to_string(),
      );
      upsert_canonical_finding(
        &mut map,
        CanonicalUpsert {
          key,
          detector: finding.detector.to_string(),
          file_path: finding.sink.file_path.clone(),
          line_number: finding.sink.line,
          redacted_secret: finding.source.literal.clone(),
          evidence: CanonicalEvidence {
            detector: finding.detector.to_string(),
            location: ScanRegion {
              file_path: finding.sink.file_path.clone(),
              start_line: finding.sink.line,
              end_line: finding.sink.line,
              start_column: finding.sink.column,
              end_column: finding.sink.column,
            },
          },
          meta: CanonicalProviderMeta {
            rule_id: None,
            provider_id: finding.provider_id.clone(),
            provider_rule_ids: finding.provider_rule_ids.clone(),
            credential_kind: finding.credential_kind.clone(),
            sdk: finding.sdk.clone(),
            provider_shape_id: finding.provider_shape_id.clone(),
          },
        },
      );
    }
    for finding in &self.sink_evidence {
      let key = (
        finding.sink.file_path.clone(),
        finding.sink.line,
        finding.value_text.clone(),
      );
      upsert_canonical_finding(
        &mut map,
        CanonicalUpsert {
          key,
          detector: finding.detector.to_string(),
          file_path: finding.sink.file_path.clone(),
          line_number: finding.sink.line,
          redacted_secret: Secret::new(finding.value_text.clone()),
          evidence: CanonicalEvidence {
            detector: finding.detector.to_string(),
            location: ScanRegion {
              file_path: finding.sink.file_path.clone(),
              start_line: finding.sink.line,
              end_line: finding.sink.line,
              start_column: finding.sink.column,
              end_column: finding.sink.column,
            },
          },
          meta: CanonicalProviderMeta {
            rule_id: None,
            provider_id: finding.provider_id.clone(),
            provider_rule_ids: finding.provider_rule_ids.clone(),
            credential_kind: finding.credential_kind.clone(),
            sdk: finding.sdk.clone(),
            provider_shape_id: finding.provider_shape_id.clone(),
          },
        },
      );
    }
    for finding in &self.confirmed_flows {
      let key = (
        finding.sink.file_path.clone(),
        finding.sink.line,
        source_origin_key(&finding.source),
      );
      upsert_canonical_finding(
        &mut map,
        CanonicalUpsert {
          key,
          detector: finding.detector.to_string(),
          file_path: finding.sink.file_path.clone(),
          line_number: finding.sink.line,
          redacted_secret: Secret::new(source_origin_key(&finding.source)),
          evidence: CanonicalEvidence {
            detector: finding.detector.to_string(),
            location: ScanRegion {
              file_path: finding.sink.file_path.clone(),
              start_line: finding.sink.line,
              end_line: finding.sink.line,
              start_column: finding.sink.column,
              end_column: finding.sink.column,
            },
          },
          meta: CanonicalProviderMeta {
            rule_id: None,
            provider_id: finding.provider_id.clone(),
            provider_rule_ids: Vec::new(),
            credential_kind: None,
            sdk: Some(finding.source.sdk.clone()),
            provider_shape_id: None,
          },
        },
      );
    }
    for finding in &self.findings {
      let layer1_region = finding.evidence.first().cloned().unwrap_or(ScanRegion {
        file_path: finding.file_path.clone(),
        start_line: finding.start_line,
        end_line: finding.end_line,
        start_column: finding.start_column,
        end_column: finding.end_column,
      });
      let matching_semantic_keys: Vec<_> = self
        .semantic_findings
        .iter()
        .filter(|semantic| {
          semantic.source.file_path == finding.file_path
            && semantic.source.line == finding.start_line
            && semantic.source.literal.expose_raw() == finding.redacted_secret.expose_raw()
        })
        .map(|semantic| {
          (
            semantic.sink.file_path.clone(),
            semantic.sink.line,
            semantic.source.literal.expose_raw().to_string(),
          )
        })
        .collect();
      let keys = if matching_semantic_keys.is_empty() {
        vec![(
          finding.file_path.clone(),
          finding.start_line,
          finding.redacted_secret.expose_raw().to_string(),
        )]
      } else {
        matching_semantic_keys
      };
      for key in keys {
        upsert_canonical_finding(
          &mut map,
          CanonicalUpsert {
            key,
            detector: finding.detector.to_string(),
            file_path: finding.file_path.clone(),
            line_number: finding.start_line,
            redacted_secret: finding.redacted_secret.clone(),
            evidence: CanonicalEvidence {
              detector: finding.detector.to_string(),
              location: layer1_region.clone(),
            },
            meta: CanonicalProviderMeta {
              rule_id: Some(finding.rule_id.clone()),
              provider_id: None,
              provider_rule_ids: Vec::new(),
              credential_kind: None,
              sdk: None,
              provider_shape_id: None,
            },
          },
        );
      }
    }
    self.canonical_findings = map.into_values().collect();
    self.summary = SecretsSummary {
      canonical: self.canonical_findings.len(),
      layer1: self.findings.len(),
      symbol_reachability: self.semantic_findings.len(),
      source_evidence: self.source_evidence.len(),
      confirmed_flows: self.confirmed_flows.len(),
      sink_evidence: self.sink_evidence.len(),
      suppressed: self.suppressed.len(),
    };
  }
}

fn detector_priority(detector: &str) -> u8 {
  match detector {
    "gitleaks-rule" => 6,
    "confirmed-credential-flow" => 5,
    "symbol-reachability" => 4,
    "provider-sink" => 3,
    "source-evidence" => 2,
    "sink-evidence" => 1,
    _ => 0,
  }
}

fn source_origin_key(source: &CredentialSourceEvidence) -> String {
  format!(
    "source:{}:{}:{}",
    source.file_path, source.line, source.variable
  )
}

struct CanonicalProviderMeta {
  rule_id: Option<String>,
  provider_id: Option<String>,
  provider_rule_ids: Vec<String>,
  credential_kind: Option<String>,
  sdk: Option<String>,
  provider_shape_id: Option<String>,
}

struct CanonicalUpsert {
  key: (String, usize, String),
  detector: String,
  file_path: String,
  line_number: usize,
  redacted_secret: Secret,
  evidence: CanonicalEvidence,
  meta: CanonicalProviderMeta,
}

fn upsert_canonical_finding(
  map: &mut std::collections::BTreeMap<(String, usize, String), CanonicalFinding>,
  upsert: CanonicalUpsert,
) {
  let finding = map.entry(upsert.key).or_insert_with(|| CanonicalFinding {
    detector: upsert.detector.clone(),
    file_path: upsert.file_path,
    line_number: upsert.line_number,
    rule_id: None,
    provider_id: None,
    provider_rule_ids: Vec::new(),
    credential_kind: None,
    sdk: None,
    provider_shape_id: None,
    redacted_secret: upsert.redacted_secret,
    evidence: Vec::new(),
  });
  if upsert.meta.rule_id.is_some() {
    finding.rule_id = upsert.meta.rule_id;
  }
  if upsert.meta.provider_id.is_some() {
    finding.provider_id = upsert.meta.provider_id;
  }
  if !upsert.meta.provider_rule_ids.is_empty() {
    finding.provider_rule_ids = upsert.meta.provider_rule_ids;
  }
  if upsert.meta.credential_kind.is_some() {
    finding.credential_kind = upsert.meta.credential_kind;
  }
  if upsert.meta.sdk.is_some() {
    finding.sdk = upsert.meta.sdk;
  }
  if upsert.meta.provider_shape_id.is_some() {
    finding.provider_shape_id = upsert.meta.provider_shape_id;
  }
  if detector_priority(&upsert.detector) > detector_priority(&finding.detector) {
    finding.detector = upsert.detector;
  }
  finding.evidence.push(upsert.evidence);
}

/// Lightweight scan counters for no-match grep/search responses.
/// `files_considered` counts file entries yielded by the walker after
/// root/glob/ignore filtering; `files_scanned` counts files actually
/// read by the sensor; `files_skipped` counts considered files skipped
/// before scanning, such as `FILE_TOO_LARGE`.
///
/// `files_skipped` does NOT count files the walker excluded via
/// `.gitignore`/`.ignore`/global-ignore rules — those never reach the
/// walker callback at all, so they were never "considered" in the
/// first place. Dogfood finding (2026-09): a caller who checks
/// `scan_stats` on a `no_match` response and sees `files_skipped: 0`
/// reasonably reads that as "nothing was skipped" — a false
/// reassurance when `.gitignore` silently hid the file that would have
/// matched. `files_ignored` is the separate, honest counter for that
/// population. It is `None` for sensors that don't yet compute it
/// (an honest "not measured," not a claimed zero) and `Some(n)` when
/// a sensor does — currently only `grep`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, SimpleObject)]
pub struct ScanStats {
  pub files_considered: usize,
  pub files_scanned: usize,
  pub files_skipped: usize,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub files_ignored: Option<usize>,
}

/// Lightweight counters for evidence scanners whose primary output is
/// grouped anchors rather than plain grep hits.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, SimpleObject)]
pub struct AnchorScanStats {
  pub files_scanned: usize,
  pub anchors_found: usize,
  pub guard_hits: usize,
  pub unsupported_files: usize,
}

// ──────────────────────── result envelopes ────────────────────────
//
// Envelopes wrap the item Vec with a versioned `schema` string,
// the diagnostic channel, and the exit_status. CLI `--format json`
// serializes the envelope; markdown rendering reads the items and
// the diagnostic channel directly. GraphQL resolvers (in-process via
// `lmprobe query` and over HTTP via `serve`) return the same envelope.

/// Macro to declare a result envelope with the standard fields
/// (`schema`, `warnings`, `exit_status`) plus a sensor-specific item
/// vector. Keeps the boilerplate symmetric across sensors.
macro_rules! envelope {
  ($name:ident, $schema:expr, $items_field:ident: Vec<$item:ty>) => {
    #[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
    pub struct $name {
      pub schema: String,
      pub $items_field: Vec<$item>,
      #[serde(default)]
      pub warnings: Vec<Diagnostic>,
      #[serde(default)]
      pub exit_status: ExitStatus,
    }
    impl $name {
      pub const SCHEMA: &'static str = $schema;
      pub fn new($items_field: Vec<$item>) -> Self {
        let exit_status = ExitStatus::from_count($items_field.len());
        Self {
          schema: Self::SCHEMA.into(),
          $items_field,
          warnings: Vec::new(),
          exit_status,
        }
      }
      #[allow(dead_code)]
      pub fn with_warnings(mut self, warnings: Vec<Diagnostic>) -> Self {
        self.warnings = warnings;
        self
      }
      #[allow(dead_code)]
      pub fn with_status(mut self, exit_status: ExitStatus) -> Self {
        self.exit_status = exit_status;
        self
      }
      #[allow(dead_code)]
      pub fn push_warning(&mut self, w: Diagnostic) {
        self.warnings.push(w);
      }
    }
  };
}

macro_rules! envelope_with_scan_stats {
  ($name:ident, $schema:expr, $items_field:ident: Vec<$item:ty>) => {
    #[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
    pub struct $name {
      pub schema: String,
      pub $items_field: Vec<$item>,
      #[serde(default)]
      pub warnings: Vec<Diagnostic>,
      #[serde(default)]
      pub exit_status: ExitStatus,
      #[serde(default, skip_serializing_if = "is_false")]
      pub partial: bool,
      #[serde(default, skip_serializing_if = "Option::is_none")]
      pub scan_stats: Option<ScanStats>,
    }
    impl $name {
      pub const SCHEMA: &'static str = $schema;
      pub fn new($items_field: Vec<$item>) -> Self {
        let exit_status = ExitStatus::from_count($items_field.len());
        Self {
          schema: Self::SCHEMA.into(),
          $items_field,
          warnings: Vec::new(),
          exit_status,
          partial: false,
          scan_stats: None,
        }
      }
      #[allow(dead_code)]
      pub fn with_warnings(mut self, warnings: Vec<Diagnostic>) -> Self {
        self.warnings = warnings;
        self
      }
      #[allow(dead_code)]
      pub fn with_status(mut self, exit_status: ExitStatus) -> Self {
        self.exit_status = exit_status;
        self
      }
      #[allow(dead_code)]
      pub fn push_warning(&mut self, w: Diagnostic) {
        self.warnings.push(w);
      }
    }
  };
}

fn is_false(value: &bool) -> bool {
  !*value
}

/// `list-kinds <lang>` per-portable-kind row. Used by `ListKindsResult`.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct PortableKindRow {
  /// Portable-kind name (function / method / class / …).
  pub portable: String,
  /// Tree-sitter literal kinds the language maps to.
  pub kinds: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct PySliceCandidate {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub candidate_id: Option<String>,
  pub file_path: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub enclosing_class: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub enclosing_function: Option<String>,
  pub sliced_base_variable: String,
  pub derived_sliced_variable: String,
  pub slice_expression: String,
  pub related_unsliced_variable: String,
  pub call_expression_line: String,
  pub slice_line_number: usize,
  pub call_line_number: usize,
  pub slice_source_line: String,
  pub call_source_line: String,
  pub confidence: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub trace_plan: Option<PySliceTracePlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct PySliceTracePlan {
  pub file_path: String,
  pub statement_pattern: String,
  pub watch_variables: Vec<String>,
  pub suggested_line_number: usize,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub enclosing_function: Option<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct CallsiteKeywordArg {
  pub name: String,
  pub value: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, Enum)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CallsiteKind {
  #[default]
  Call,
  New,
  Struct,
  Assignment,
}

fn is_default_callsite_kind(kind: &CallsiteKind) -> bool {
  *kind == CallsiteKind::Call
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Enum)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CallsiteLanguage {
  Python,
  Javascript,
  Typescript,
  Go,
  Java,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Enum)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CallsiteRole {
  Positional,
  Keyword,
  ObjectKey,
  ObjectPath,
  StructField,
  AssignmentValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct CallsiteArgument {
  pub position: Option<usize>,
  pub name: Option<String>,
  pub text: String,
  pub role: CallsiteRole,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, InputObject)]
pub struct CallsiteFilterInput {
  /// Non-null fields are ANDed within one filter entry; values within
  /// the field are ORed. Multiple filter entries are ORed.
  #[serde(default)]
  pub arg_names: Option<Vec<String>>,
  /// Positional argument indexes that may satisfy this filter.
  #[serde(default)]
  pub positional_indices: Option<Vec<usize>>,
  #[serde(default)]
  pub object_keys: Option<Vec<String>>,
  #[serde(default)]
  pub object_paths: Option<Vec<String>>,
  #[serde(default)]
  pub struct_fields: Option<Vec<String>>,
  #[serde(default)]
  pub assignment_targets: Option<Vec<String>>,
  #[serde(default)]
  pub import_hints: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Enum)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SourceReadLanguage {
  Python,
  Javascript,
  Typescript,
  Go,
  Java,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Enum)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SourceReadKind {
  EnvRead,
  KvGet,
  SdkCall,
  DotenvLoad,
  ConfigLookup,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Enum)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SourceReadRole {
  EnvKey,
  SecretPath,
  SecretId,
  ConfigKey,
  LoaderCall,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, InputObject)]
pub struct SourceReadFilterInput {
  /// Non-null fields are ANDed within one filter entry; values within
  /// the field are ORed. Multiple filter entries are ORed.
  #[serde(default)]
  pub source_ids: Option<Vec<String>>,
  #[serde(default)]
  pub provider_ids: Option<Vec<String>>,
  #[serde(default)]
  pub categories: Option<Vec<SourceReadKind>>,
  #[serde(default)]
  pub env_var_names: Option<Vec<String>>,
  #[serde(default)]
  pub key_names: Option<Vec<String>>,
  #[serde(default)]
  pub access_paths: Option<Vec<String>>,
  #[serde(default)]
  pub callees: Option<Vec<String>>,
  #[serde(default)]
  pub object_paths: Option<Vec<String>>,
  #[serde(default)]
  pub import_hints: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct SourceReadHit {
  pub file_path: String,
  pub line_number: usize,
  pub column: usize,
  pub source_id: String,
  pub source_category: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub provider_id: Option<String>,
  pub sdk: String,
  pub read_text: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub key_name: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub env_var_name: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub access_path: Option<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub import_evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
#[graphql(complex)]
pub struct CallsiteArgsHit {
  pub file_path: String,
  pub line_number: usize,
  pub callee_text: String,
  pub positional_args: Vec<String>,
  pub keyword_args: Vec<CallsiteKeywordArg>,
  #[serde(default, skip_serializing_if = "is_default_callsite_kind")]
  pub sink_kind: CallsiteKind,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub import_evidence: Vec<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub structured_args: Vec<CallsiteArgument>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub source_context: Vec<LineContext>,
}

#[ComplexObject]
impl CallsiteArgsHit {
  async fn arguments(
    &self,
    name: Option<String>,
    position: Option<usize>,
    role: Option<CallsiteRole>,
  ) -> Vec<CallsiteArgument> {
    let mut args = if self.structured_args.is_empty() {
      callsite_arguments_from_legacy(self)
    } else {
      self.structured_args.clone()
    };
    args.retain(|arg| {
      name
        .as_ref()
        .is_none_or(|name| arg.name.as_deref() == Some(name.as_str()))
        && position.is_none_or(|position| arg.position == Some(position))
        && role.is_none_or(|role| arg.role == role)
    });
    args
  }
}

fn callsite_arguments_from_legacy(hit: &CallsiteArgsHit) -> Vec<CallsiteArgument> {
  let mut args = Vec::new();
  args.extend(
    hit
      .positional_args
      .iter()
      .enumerate()
      .map(|(position, text)| CallsiteArgument {
        position: Some(position),
        name: None,
        text: text.clone(),
        role: CallsiteRole::Positional,
      }),
  );
  args.extend(hit.keyword_args.iter().map(|arg| CallsiteArgument {
    position: None,
    name: Some(arg.name.clone()),
    text: arg.value.clone(),
    role: CallsiteRole::Keyword,
  }));
  args
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct PrTriageTarget {
  pub owner: String,
  pub repo: String,
  pub number: u64,
  pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct PrTriageStatus {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub title: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub author: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub state: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub mergeable: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub review_decision: Option<String>,
  pub draft: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, SimpleObject)]
pub struct PrCheckSummary {
  pub total: usize,
  pub passing: usize,
  pub failing: usize,
  pub pending: usize,
  pub skipped_neutral: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct PrCheckEvidence {
  pub name: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub status: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub conclusion: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub state: Option<String>,
  pub bucket: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct PrReviewEvidence {
  pub kind: String,
  pub author: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub author_association: Option<String>,
  pub created_at: String,
  pub body_excerpt: String,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub action_terms: Vec<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub review_state: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub line: Option<usize>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub url: Option<String>,
}

envelope!(ListKindsResult,     "lmprobe.list_kinds.v1",  rows: Vec<PortableKindRow>);
envelope_with_scan_stats!(SearchResult, "lmprobe.search.v1", hits: Vec<SymbolHit>);
envelope!(DefResult,           "lmprobe.def.v1",         hits: Vec<SymbolHit>);
envelope!(RefResult,           "lmprobe.ref.v1",         hits: Vec<GrepHit>);
envelope!(CallersResult,       "lmprobe.callers.v1",     hits: Vec<SymbolHit>);
envelope!(ImportsResult,       "lmprobe.imports.v1",     hits: Vec<SymbolHit>);
envelope!(DecorationsResult,   "lmprobe.decorations.v1", hits: Vec<SymbolHit>);
envelope!(DiscoverResult,      "lmprobe.discover.v1",    projects: Vec<Project>);
envelope_with_scan_stats!(GrepResultEnvelope, "lmprobe.grep.v1", hits: Vec<GrepHit>);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Enum)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
  File,
  Directory,
  Symlink,
  Other,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, InputObject)]
#[serde(deny_unknown_fields)]
pub struct GitRemotePredicateInput {
  pub host: Option<String>,
  pub owner: Option<String>,
  pub repo: Option<String>,
  /// Optional remote name, such as origin or upstream.
  pub remote: Option<String>,
}

/// Clauses combine with AND; all/any/not compose nested filesystem predicates.
#[derive(Debug, Clone, Default, Serialize, Deserialize, InputObject)]
#[serde(deny_unknown_fields)]
pub struct FilePredicateInput {
  pub kind: Option<FileKind>,
  pub name_regex: Option<String>,
  pub path_regex: Option<String>,
  /// A direct child entry name, never a relative path or recursive search.
  pub has_entry: Option<String>,
  pub git_remote: Option<GitRemotePredicateInput>,
  pub all: Option<Vec<FilePredicateInput>>,
  pub any: Option<Vec<FilePredicateInput>>,
  pub not: Option<Box<FilePredicateInput>>,
}

/// Snapshot of a selected filesystem entry. Paths remain separate for v1 clients.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject, PartialEq)]
pub struct FileEntry {
  pub path: String,
  pub absolute_path: String,
  pub file_type: String,
  #[serde(default)]
  pub has_symlink_parent: bool,
  pub size_bytes: u64,
  pub modified_at: Option<chrono::DateTime<chrono::Utc>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub changed_at: Option<chrono::DateTime<chrono::Utc>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub device: Option<u64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub inode: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct FileDeleteEntry {
  pub file: FileEntry,
  pub status: String,
  pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct FileDeleteResult {
  pub schema: String,
  pub mode: String,
  pub files: Vec<FileDeleteEntry>,
  pub deleted_count: usize,
  pub audit_log_path: Option<String>,
  #[serde(default)]
  pub warnings: Vec<Diagnostic>,
  #[serde(default)]
  pub exit_status: ExitStatus,
  #[serde(default, skip_serializing_if = "is_false")]
  pub partial: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct FindResult {
  pub schema: String,
  pub paths: Vec<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub files: Vec<FileEntry>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub action: Option<FileDeleteResult>,
  #[serde(default)]
  pub warnings: Vec<Diagnostic>,
  #[serde(default)]
  pub exit_status: ExitStatus,
  #[serde(default, skip_serializing_if = "is_false")]
  pub partial: bool,
}

impl FindResult {
  pub const SCHEMA: &'static str = "lmprobe.find.v1";
  pub fn new(paths: Vec<String>) -> Self {
    let exit_status = ExitStatus::from_count(paths.len());
    Self {
      schema: Self::SCHEMA.into(),
      paths,
      files: Vec::new(),
      action: None,
      warnings: Vec::new(),
      exit_status,
      partial: false,
    }
  }
  pub fn with_warnings(mut self, warnings: Vec<Diagnostic>) -> Self {
    self.warnings = warnings;
    self
  }
  pub fn with_status(mut self, exit_status: ExitStatus) -> Self {
    self.exit_status = exit_status;
    self
  }
  pub fn push_warning(&mut self, warning: Diagnostic) {
    self.warnings.push(warning);
  }
}
envelope!(PySliceCandidatesResult, "lmprobe.py_slice_candidates.v1", candidates: Vec<PySliceCandidate>);
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct CallsitesResult {
  pub schema: String,
  pub callsites: Vec<CallsiteArgsHit>,
  #[serde(default)]
  pub arg_values_redacted: bool,
  #[serde(default)]
  pub warnings: Vec<Diagnostic>,
  #[serde(default)]
  pub exit_status: ExitStatus,
}

impl CallsitesResult {
  pub const SCHEMA: &'static str = "lmprobe.callsites.v1";
  pub fn new(callsites: Vec<CallsiteArgsHit>) -> Self {
    let exit_status = ExitStatus::from_count(callsites.len());
    Self {
      schema: Self::SCHEMA.into(),
      callsites,
      arg_values_redacted: false,
      warnings: Vec::new(),
      exit_status,
    }
  }

  #[allow(dead_code)]
  pub fn with_warnings(mut self, warnings: Vec<Diagnostic>) -> Self {
    self.warnings = warnings;
    self
  }

  #[allow(dead_code)]
  pub fn with_status(mut self, exit_status: ExitStatus) -> Self {
    self.exit_status = exit_status;
    self
  }

  #[allow(dead_code)]
  pub fn push_warning(&mut self, w: Diagnostic) {
    self.warnings.push(w);
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct SourceReadsResult {
  pub schema: String,
  pub source_reads: Vec<SourceReadHit>,
  #[serde(default)]
  pub arg_values_redacted: bool,
  #[serde(default)]
  pub warnings: Vec<Diagnostic>,
  #[serde(default)]
  pub exit_status: ExitStatus,
}

impl SourceReadsResult {
  pub const SCHEMA: &'static str = "lmprobe.source_reads.v1";
  pub fn new(source_reads: Vec<SourceReadHit>) -> Self {
    let exit_status = ExitStatus::from_count(source_reads.len());
    Self {
      schema: Self::SCHEMA.into(),
      source_reads,
      arg_values_redacted: false,
      warnings: Vec::new(),
      exit_status,
    }
  }

  pub fn push_warning(&mut self, w: Diagnostic) {
    self.warnings.push(w);
  }
}
envelope!(DiffContextResult, "lmprobe.diff_context.v1", anchors: Vec<DiffContextAnchor>);
envelope!(ResourceSitesResult, "lmprobe.resource_sites.v1", sites: Vec<ResourceSite>);
envelope!(AsyncLeaksResult, "lmprobe.async_leaks.v1", sites: Vec<AsyncLeakSite>);
envelope!(CryptoSitesResult, "lmprobe.crypto_sites.v1", sites: Vec<CryptoSite>);
envelope!(ConfigKeysResult, "lmprobe.config_keys.v1", keys: Vec<ConfigKeyRecord>);
envelope!(OpenApiFactsResult, "lmprobe.openapi_facts.v1", operations: Vec<OpenApiOperationFact>);
envelope!(RouteAnchorsResult, "lmprobe.route_anchors.v1", routes: Vec<RouteAnchor>);
envelope!(HttpClientCallsResult, "lmprobe.http_client_calls.v1", calls: Vec<HttpClientCallAnchor>);

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct PrTriageResult {
  pub schema: String,
  pub target: PrTriageTarget,
  pub since: String,
  pub compact: bool,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub status: Option<PrTriageStatus>,
  pub check_summary: PrCheckSummary,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub checks: Vec<PrCheckEvidence>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub evidence: Vec<PrReviewEvidence>,
  #[serde(default)]
  pub warnings: Vec<Diagnostic>,
  #[serde(default)]
  pub exit_status: ExitStatus,
}

impl PrTriageResult {
  pub const SCHEMA: &'static str = "lmprobe.pr_triage.v1";

  pub fn new(target: PrTriageTarget, since: impl Into<String>, compact: bool) -> Self {
    Self {
      schema: Self::SCHEMA.into(),
      target,
      since: since.into(),
      compact,
      status: None,
      check_summary: PrCheckSummary::default(),
      checks: Vec::new(),
      evidence: Vec::new(),
      warnings: Vec::new(),
      exit_status: ExitStatus::Matched,
    }
  }

  pub fn push_warning(&mut self, w: Diagnostic) {
    self.warnings.push(w);
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct GuardSitesResult {
  pub schema: String,
  pub candidates: Vec<GuardSiteCandidate>,
  #[serde(default)]
  pub warnings: Vec<Diagnostic>,
  #[serde(default)]
  pub exit_status: ExitStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub scan_stats: Option<AnchorScanStats>,
}

impl GuardSitesResult {
  pub const SCHEMA: &'static str = "lmprobe.guard_sites.v1";

  pub fn new(candidates: Vec<GuardSiteCandidate>) -> Self {
    let exit_status = ExitStatus::from_count(candidates.len());
    Self {
      schema: Self::SCHEMA.into(),
      candidates,
      warnings: Vec::new(),
      exit_status,
      scan_stats: None,
    }
  }

  #[allow(dead_code)]
  pub fn with_warnings(mut self, warnings: Vec<Diagnostic>) -> Self {
    self.warnings = warnings;
    self
  }

  #[allow(dead_code)]
  pub fn with_status(mut self, exit_status: ExitStatus) -> Self {
    self.exit_status = exit_status;
    self
  }

  #[allow(dead_code)]
  pub fn push_warning(&mut self, w: Diagnostic) {
    self.warnings.push(w);
  }
}

// ─────────────────── Phase 4E: `query` envelope ───────────────────
//
// `lmprobe query '<gql>'` wraps the async-graphql `Response` in the
// standard D4 envelope. The `data` and `errors` fields mirror the
// GraphQL spec; `warnings` and `exit_status` are lmprobe's CLI-side
// channels (warnings includes a one-line mirror of each error so
// `-q` semantics work uniformly).
//
// Not a `SimpleObject` — this envelope is the CLI-side output
// surface only; it is never returned by a resolver. The opaque
// `data` field carries `serde_json::Value` so any selection set
// shape round-trips faithfully.

/// One GraphQL error from `query` execution. Mirrors the GraphQL spec
/// shape (message + path + locations + extensions.code).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryError {
  pub message: String,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub path: Vec<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub locations: Vec<QueryErrorLocation>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryErrorLocation {
  pub line: u32,
  pub column: u32,
}

/// Envelope for `lmprobe query`. Carries a faithful GraphQL response
/// (`data` + `errors`) inside the standard D4 frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
  pub schema: String,
  /// GraphQL `data` payload. `null` on a top-level parse / validate
  /// failure; otherwise present (possibly with field-level nulls).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub data: Option<serde_json::Value>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub errors: Vec<QueryError>,
  #[serde(default)]
  pub warnings: Vec<Diagnostic>,
  #[serde(default)]
  pub exit_status: ExitStatus,
  #[serde(default, skip_serializing_if = "is_false")]
  pub partial: bool,
}

impl QueryResult {
  pub const SCHEMA: &'static str = "lmprobe.query.v1";
}

/// Stable program-facing transport; sensor schemas stay independently versioned.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProgramStatus {
  Ok,
  NoMatch,
  Partial,
  Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PayloadFormat {
  List,
  Rows,
  Hierarchical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramResult {
  pub schema: String,
  pub source_schema: String,
  pub status: ProgramStatus,
  pub payload_format: PayloadFormat,
  /// list: {items: []}; rows: {columns: [], rows: []}; hierarchical: {data: ...}.
  pub payload: serde_json::Value,
  /// Remaining command-specific information, without duplicate primary rows.
  pub metadata: serde_json::Value,
  pub diagnostics: Vec<Diagnostic>,
}

impl ProgramResult {
  pub const SCHEMA: &'static str = "lmprobe.result.v1";
}

// ─────────────────── Phase 3A: history family ────────────────────
//
// One envelope per `lmprobe history <verb>`. Each item shape is
// declared above in the item-shapes section as a typed struct so
// JSON output is stable and serde_json::Value isn't needed.

/// One contributor entry in `history blame`'s contributors timeline.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct BlameEntry {
  pub commit_hash: String,
  pub author: String,
  pub timestamp: String,
  /// "INTRODUCED" | "LOGIC_CHANGE" | "FORMATTING" | "RENAMED".
  pub change_type: String,
  pub file_path: String,
  /// 1-based line range of the symbol in the blob at this commit.
  /// `None` (= JSON `null`) means "line range unrecoverable for this
  /// entry; commit is real" — happens at cross-rename hops or when
  /// the cached blob signature pre-dates the line-range cache
  /// extension (Phase 6B #3, sensor_version=2). Phase 6A and earlier
  /// would have surfaced `0` here as a sentinel; chapter 17's
  /// `LineNumber ≥ 1` doctrine says null is the correct shape for
  /// "absent."
  pub line_start: Option<u32>,
  pub line_end: Option<u32>,
  /// Brief excerpt — body-elided declaration line for symbol-form
  /// blame; the literal source line for line-form blame.
  pub excerpt: String,
}

/// Phase 4B §4.E — one rename hop along the file's history when
/// `history blame --follow` is set. `from` / `to` are paths
/// relative to the repo root; `commit` is the SHA of the
/// rename-detection point.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct FileRename {
  pub from: String,
  pub to: String,
  pub commit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct BlameResult {
  pub schema: String,
  pub entries: Vec<BlameEntry>,
  /// Optional line-numbered source window for line-form history blame.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub source_context: Vec<LineContext>,
  /// Phase 4B §4.E — populated when `--follow` is set; lists each
  /// rename hop the AST-aware walk crossed. Empty otherwise.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub follows: Vec<FileRename>,
  #[serde(default)]
  pub warnings: Vec<Diagnostic>,
  #[serde(default)]
  pub exit_status: ExitStatus,
}
impl BlameResult {
  pub const SCHEMA: &'static str = "lmprobe.history_blame.v1";
  pub fn new(entries: Vec<BlameEntry>) -> Self {
    let exit_status = ExitStatus::from_count(entries.len());
    Self {
      schema: Self::SCHEMA.into(),
      entries,
      source_context: Vec::new(),
      follows: Vec::new(),
      warnings: Vec::new(),
      exit_status,
    }
  }
  /// Reserved for future warning emitters on the blame path
  /// (e.g. RENAME_DETECTION_LIMIT for cross-FILE-and-symbol-name
  /// renames, scoped post-1.0).
  #[allow(dead_code)]
  pub fn push_warning(&mut self, w: Diagnostic) {
    self.warnings.push(w);
  }
}

envelope!(IntroducedResult,   "lmprobe.history_introduced.v1",   entries: Vec<BlameEntry>);

/// One commit hit for `history commit -p` / `history diff -p`.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct CommitHit {
  pub commit_hash: String,
  pub author: String,
  pub timestamp: String,
  pub message: String,
  /// File paths touched in this commit that matched the filter
  /// (pattern in diff, etc.). Empty for message-only matches.
  #[serde(default)]
  pub matched_files: Vec<String>,
}

envelope!(HistoryCommitResult, "lmprobe.history_commit.v1",      hits: Vec<CommitHit>);

/// One diff snippet for `history diff -p`.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct DiffHit {
  pub commit_hash: String,
  pub author: String,
  pub timestamp: String,
  pub file_path: String,
  pub before: String,
  pub after: String,
}

envelope!(HistoryDiffResult,  "lmprobe.history_diff.v1",         hits: Vec<DiffHit>);

/// One churn entry — file or symbol with its commit count.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct ChurnEntry {
  pub path: String,
  pub commit_count: u32,
  pub last_touched: String,
}

envelope!(HistoryChurnResult, "lmprobe.history_churn.v1",        rows: Vec<ChurnEntry>);

/// One co-change entry — a file that often changed alongside the
/// queried file, with the count of co-occurring commits.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct CoChangeEntry {
  pub path: String,
  pub co_occurrences: u32,
  /// Jaccard-ish similarity score in [0, 1].
  pub score: f32,
}

envelope!(HistoryCoChangeResult, "lmprobe.history_co_change.v1", rows: Vec<CoChangeEntry>);

/// One TODO/FIXME entry surfaced by `history todos`.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct TodoEntry {
  pub commit_hash: String,
  pub author: String,
  pub timestamp: String,
  pub file_path: String,
  pub line_number: u32,
  /// "TODO" | "FIXME" | "XXX" | "HACK".
  pub marker: String,
  pub text: String,
}

envelope!(HistoryTodosResult, "lmprobe.history_todos.v1",        hits: Vec<TodoEntry>);

/// Phase 4B §4.1 — one diff hunk produced by `fix --dry-run`.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct DiffHunk {
  pub line_start: u32,
  pub line_count: u32,
  pub before: String,
  pub after: String,
}

/// Phase 4B §4.1 — per-file diff produced by `fix --dry-run`.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct FileDiff {
  pub file_path: String,
  pub hunks: Vec<DiffHunk>,
  pub matches_count: u32,
}

/// `lmprobe fix <pattern> <replacement>` — dry-run by default; the
/// 4C `--apply` mode adds patch_path / audit_log_path / applied_files_count.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct FixResult {
  pub schema: String,
  /// "dry_run" | "applied".
  pub mode: String,
  pub diffs: Vec<FileDiff>,
  pub matches_count: u32,
  pub files_count: u32,
  /// §4.1 4C — populated in apply mode. v1-additive.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub patch_path: Option<String>,
  /// §4.1 4C — files actually written by --apply (after worktree
  /// patch-application). v1-additive.
  #[serde(default)]
  pub applied_files_count: u32,
  /// §4.1 4C — locator of the per-invocation audit log (mirrors
  /// trace's audit-log pattern per Lead arbitration item iv).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub audit_log_path: Option<String>,
  #[serde(default)]
  pub warnings: Vec<Diagnostic>,
  #[serde(default)]
  pub exit_status: ExitStatus,
}
impl FixResult {
  pub const SCHEMA: &'static str = "lmprobe.fix.v1";
  pub fn new(mode: &str, diffs: Vec<FileDiff>) -> Self {
    let matches_count: u32 = diffs.iter().map(|d| d.matches_count).sum();
    let files_count = diffs.len() as u32;
    let exit_status = if matches_count > 0 {
      ExitStatus::Matched
    } else {
      ExitStatus::NoMatch
    };
    Self {
      schema: Self::SCHEMA.into(),
      mode: mode.to_string(),
      diffs,
      matches_count,
      files_count,
      patch_path: None,
      applied_files_count: 0,
      audit_log_path: None,
      warnings: Vec::new(),
      exit_status,
    }
  }
  pub fn push_warning(&mut self, w: Diagnostic) {
    self.warnings.push(w);
  }
}

/// §4.8 — one node-level structural diff entry.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct NodeDiff {
  /// Tree-sitter node kind (e.g. `function_item`, `struct_item`).
  pub kind: String,
  /// Symbol name extracted via NameExtractor; empty if anonymous.
  pub name: String,
  /// "ADDED" | "REMOVED" | "MODIFIED" | "UNCHANGED".
  pub change_type: String,
  pub from_signature: String,
  pub to_signature: String,
  pub from_line: u32,
  pub to_line: u32,
}

envelope!(AstDiffResult, "lmprobe.ast_diff.v1", nodes: Vec<NodeDiff>);

// ─────────────── Phase 3B: graph primitives + impact + context ──────
//
// §3.3 — five symmetric verbs (callees / supertypes / subtypes /
// overrides / exports). Reuses SymbolHit shape so callers can flow
// directly into the same renderers.

envelope!(CalleesResult,    "lmprobe.callees.v1",    hits: Vec<SymbolHit>);
envelope!(SupertypesResult, "lmprobe.supertypes.v1", hits: Vec<SymbolHit>);
envelope!(SubtypesResult,   "lmprobe.subtypes.v1",   hits: Vec<SymbolHit>);
envelope!(OverridesResult,  "lmprobe.overrides.v1",  hits: Vec<SymbolHit>);
envelope!(ExportsResult,    "lmprobe.exports.v1",    hits: Vec<SymbolHit>);

/// One node in the impact DAG. `depth` is the number of hops from
/// the queried symbol; `confidence` is depth-banded
/// ("high" 1, "medium" 2-5, "low" >5) per Lead arbitration.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct ImpactNode {
  pub name: String,
  pub kind: String,
  pub file_path: String,
  pub line_start: u32,
  pub depth: u32,
  pub confidence: String,
}

/// One directed edge in the impact DAG.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct ImpactEdge {
  pub from: String,
  pub to: String,
  /// "calls" | "called_by".
  pub edge_type: String,
}

/// `lmprobe impact`: structured DAG envelope. Custom-written like
/// `EvolutionResult` so direction/origin context stays first-class.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct ImpactResult {
  pub schema: String,
  pub origin: String,
  pub direction: String,
  pub max_depth: u32,
  pub nodes: Vec<ImpactNode>,
  pub edges: Vec<ImpactEdge>,
  #[serde(default)]
  pub warnings: Vec<Diagnostic>,
  #[serde(default)]
  pub exit_status: ExitStatus,
}
impl ImpactResult {
  pub const SCHEMA: &'static str = "lmprobe.impact.v1";
  pub fn new(
    origin: String,
    direction: String,
    max_depth: u32,
    nodes: Vec<ImpactNode>,
    edges: Vec<ImpactEdge>,
  ) -> Self {
    let exit_status = ExitStatus::from_count(nodes.len());
    Self {
      schema: Self::SCHEMA.into(),
      origin,
      direction,
      max_depth,
      nodes,
      edges,
      warnings: Vec::new(),
      exit_status,
    }
  }
  pub fn push_warning(&mut self, w: Diagnostic) {
    self.warnings.push(w);
  }
}

/// `lmprobe context <symbol>`: composite bundle. Each field is
/// optional so verbs the user disables (or that fail individually)
/// don't break the whole envelope.
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct ContextResult {
  pub schema: String,
  pub symbol: String,
  pub depth: u32,
  /// First definition matched (skeleton + file path).
  pub definition: Option<SymbolHit>,
  /// 1-hop callers.
  #[serde(default)]
  pub callers: Vec<SymbolHit>,
  /// 1-hop callees.
  #[serde(default)]
  pub callees: Vec<SymbolHit>,
  /// File-level imports (the def-containing file).
  #[serde(default)]
  pub imports: Vec<SymbolHit>,
  /// Last 3 commits touching the symbol.
  #[serde(default)]
  pub recent_history: Vec<BlameEntry>,
  /// TODO/FIXME markers found inside the def-containing region.
  #[serde(default)]
  pub todos_inline: Vec<TodoEntry>,
  /// File outline of the def-containing file.
  #[serde(default)]
  pub file_outline: Vec<SymbolHit>,
  /// Usage/call/reference anchors requested via `context --usage`.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub usages: Vec<GrepHit>,
  /// Optional line-numbered source window around the resolved context anchor.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub source_context: Vec<LineContext>,
  #[serde(default)]
  pub warnings: Vec<Diagnostic>,
  #[serde(default)]
  pub exit_status: ExitStatus,
}
impl ContextResult {
  pub const SCHEMA: &'static str = "lmprobe.context.v1";
  pub fn new(symbol: String, depth: u32) -> Self {
    Self {
      schema: Self::SCHEMA.into(),
      symbol,
      depth,
      definition: None,
      callers: Vec::new(),
      callees: Vec::new(),
      imports: Vec::new(),
      recent_history: Vec::new(),
      todos_inline: Vec::new(),
      file_outline: Vec::new(),
      usages: Vec::new(),
      source_context: Vec::new(),
      warnings: Vec::new(),
      exit_status: ExitStatus::default(),
    }
  }
  pub fn push_warning(&mut self, w: Diagnostic) {
    self.warnings.push(w);
  }
}

// Evolution and Trace carry richer shapes than a single Vec; declare
// them by hand so the symbol / exec context stays first-class.

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct EvolutionResult {
  pub schema: String,
  pub symbol: String,
  pub milestones: Vec<Milestone>,
  #[serde(default)]
  pub warnings: Vec<Diagnostic>,
  #[serde(default)]
  pub exit_status: ExitStatus,
}
impl EvolutionResult {
  pub const SCHEMA: &'static str = "lmprobe.evolution.v1";
  pub fn new(symbol: String, milestones: Vec<Milestone>) -> Self {
    let exit_status = ExitStatus::from_count(milestones.len());
    Self {
      schema: Self::SCHEMA.into(),
      symbol,
      milestones,
      warnings: Vec::new(),
      exit_status,
    }
  }
  #[allow(dead_code)]
  pub fn with_warnings(mut self, warnings: Vec<Diagnostic>) -> Self {
    self.warnings = warnings;
    self
  }
  #[allow(dead_code)]
  pub fn with_status(mut self, exit_status: ExitStatus) -> Self {
    self.exit_status = exit_status;
    self
  }
  pub fn push_warning(&mut self, w: Diagnostic) {
    self.warnings.push(w);
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct TraceRunResult {
  pub schema: String,
  pub events: Vec<TraceEvent>,
  /// Phase 4B §4.2 — populated when `trace --worktree` ran the
  /// instrumented session in a fresh git worktree. v1-additive.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub worktree_path: Option<String>,
  /// Phase 4B §4.2 — populated when `trace --keep-patch` captured
  /// the diff before rollback. v1-additive.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub patch_path: Option<String>,
  #[serde(default)]
  pub warnings: Vec<Diagnostic>,
  #[serde(default)]
  pub exit_status: ExitStatus,
}
impl TraceRunResult {
  pub const SCHEMA: &'static str = "lmprobe.trace.v1";
  pub fn new(events: Vec<TraceEvent>) -> Self {
    let exit_status = ExitStatus::from_count(events.len());
    Self {
      schema: Self::SCHEMA.into(),
      events,
      worktree_path: None,
      patch_path: None,
      warnings: Vec::new(),
      exit_status,
    }
  }
  #[allow(dead_code)]
  pub fn with_warnings(mut self, warnings: Vec<Diagnostic>) -> Self {
    self.warnings = warnings;
    self
  }
  #[allow(dead_code)]
  pub fn with_status(mut self, exit_status: ExitStatus) -> Self {
    self.exit_status = exit_status;
    self
  }
  pub fn push_warning(&mut self, w: Diagnostic) {
    self.warnings.push(w);
  }
  /// Convenience: an envelope reporting a refusal (e.g. dirty tree,
  /// non-git directory, no template). Always carries
  /// `ExitStatus::Error`.
  pub fn refusal(diag: Diagnostic) -> Self {
    Self {
      schema: Self::SCHEMA.into(),
      events: Vec::new(),
      worktree_path: None,
      patch_path: None,
      warnings: vec![diag],
      exit_status: ExitStatus::Error,
    }
  }
}

// ─────────────────────── Phase 4E §6.4: untrace ───────────────────

/// One reverted insertion site recorded by `untrace`. Mirrors the
/// shape of an entry in the `trace-<marker>.audit.json` log so the
/// reverse operation is straightforward to audit.
#[derive(Debug, Clone, Serialize, Deserialize, Default, SimpleObject)]
pub struct RevertedSite {
  pub file_path: String,
  pub line_number: u32,
  /// The literal text that was removed from the file at this line.
  pub reverted_text: String,
}

/// `lmprobe query 'mutation { untrace(marker: ...) }'` envelope.
/// First version (`lmprobe.untrace.v1`).
#[derive(Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct UntraceResult {
  pub schema: String,
  /// The marker the operation reverted.
  pub marker: String,
  /// Number of inserted lines deleted.
  pub reverted_count: u32,
  /// Per-site detail of each deletion.
  pub reverted_sites: Vec<RevertedSite>,
  /// "audit_log" when revert used the JSON audit log;
  /// "regex_fallback" when the audit log was missing and the
  /// `[TRACE-<marker>]` marker scan was used (per arbitration F).
  pub mode: String,
  #[serde(default)]
  pub warnings: Vec<Diagnostic>,
  #[serde(default)]
  pub exit_status: ExitStatus,
}
#[allow(dead_code)] // Helpers consumed by the `untrace` resolver in 4E-C.4.
impl UntraceResult {
  pub const SCHEMA: &'static str = "lmprobe.untrace.v1";
  pub fn new(marker: String, mode: &str, sites: Vec<RevertedSite>) -> Self {
    let reverted_count = sites.len() as u32;
    let exit_status = if reverted_count > 0 {
      ExitStatus::Matched
    } else {
      ExitStatus::NoMatch
    };
    Self {
      schema: Self::SCHEMA.into(),
      marker,
      reverted_count,
      reverted_sites: sites,
      mode: mode.to_string(),
      warnings: Vec::new(),
      exit_status,
    }
  }
  pub fn push_warning(&mut self, w: Diagnostic) {
    self.warnings.push(w);
  }
  /// Refusal envelope (e.g. dirty-tree, invalid marker). Always
  /// `ExitStatus::Error`.
  pub fn refusal(marker: String, diag: Diagnostic) -> Self {
    Self {
      schema: Self::SCHEMA.into(),
      marker,
      reverted_count: 0,
      reverted_sites: Vec::new(),
      mode: "refused".into(),
      warnings: vec![diag],
      exit_status: ExitStatus::Error,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::FlowStepKind;

  #[test]
  fn flow_step_kind_source_serializes_as_additive_schema_variant() {
    let json = serde_json::to_string(&FlowStepKind::Source).expect("serialize flow step kind");
    assert_eq!(json, "\"source\"");
  }
}
