//! `lmprobe-secrets` — gitleaks-compatible rule model and secret-safe types.
//!
//! Phase 0 intentionally ships as a library crate only. It owns the
//! gitleaks TOML schema loader, Rust-regex compatibility shim, and
//! redaction invariants that later scanner phases will build on.

pub mod ast_context;
pub mod provider_db;
pub mod redaction;
pub mod regex_compat;
pub mod rules;
pub mod scanner;
pub mod semantic;
pub mod source_db;
pub mod source_matchers;

pub use ast_context::{apply_ast_fp_cut, AstFpCutMode, AstFpCutOutcome};
pub use lmprobe_types::schema::{
  ActiveCredentialOrigin, AstContext, ConfirmedCredentialFlowFinding, CredentialSourceEvidence,
  FlowStep, FlowStepKind, ScanRegion, SecretDiagnostic, SecretFinding, SecretsExitStatus,
  SecretsResult, SemanticConfidence, SinkEvidence, SinkEvidenceOnlyFinding, SinkKind,
  SourceEvidence, SuppressedSecretFinding, SymbolReachabilityFinding,
};
pub use provider_db::{
  compile_provider_prefilter, deprecated_field_diagnostic, generate_graphql_recipe,
  load_default_provider_db, load_provider_db_str, validate_provider_db, CallShape, CallShapeKind,
  Provider, ProviderDb, ProviderDbError, ProviderLanguage, ProviderLanguageId, DEFAULT_PROVIDER_DB,
  PROVIDER_DB_DEPRECATED_FIELD, PROVIDER_DB_SCHEMA_MISMATCH, PROVIDER_DB_UNKNOWN_FIELD,
  SUPPORTED_SCHEMA_VERSION,
};
pub use redaction::Secret;
pub use regex_compat::{
  compile_gitleaks_pattern, normalize_go_re2_literal_braces, CompiledPattern, PatternCompileError,
  PatternKind, PatternStats,
};
pub use rules::{
  load_gitleaks_config_str, Allowlist, CompiledPatternSet, Extend, GitleaksConfig, RequiredRule,
  Rule, RulePatternError, RulePatternRef,
};
pub use scanner::{
  shannon_entropy, FileScanInput, Layer1Engine, Layer1Error, DEFAULT_GITLEAKS_CONFIG,
};
pub use semantic::{
  analyze_python_source, analyze_source, analyze_sources_with_module_graph,
  content_has_module_graph_token, content_has_sink_token, content_has_source_token,
  PythonAttributeSink, PythonCallSink, PythonCredentialKwargSink, PythonSinkCatalog,
  SemanticScanResult, SemanticWarning, SinkEvidenceMode, SINK_CATALOG_VERSION,
};
pub use source_db::{
  compile_source_prefilter, deprecated_source_field_diagnostic, generate_source_graphql_recipe,
  load_default_source_db, load_source_db_str, validate_source_db, ReadShape, ReadShapeKind, Source,
  SourceCategory, SourceDb, SourceDbError, SourceLanguage, DEFAULT_SOURCE_DB,
  SOURCE_DB_DEPRECATED_FIELD, SOURCE_DB_GENERIC_FAST_PATH_TOKEN, SOURCE_DB_SCHEMA_MISMATCH,
  SOURCE_DB_UNKNOWN_FIELD, SUPPORTED_SOURCE_SCHEMA_VERSION,
};
