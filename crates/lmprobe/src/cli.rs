//! CLI surface and dispatcher.
//!
//! Owns the `Cli` / `Commands` clap definitions, the dispatcher, and
//! the GraphQL `Query` object exposed via `lmprobe serve`. Sensors
//! are called via [`crate::sensors`] and their results are rendered
//! via [`crate::output`].
//!
//! ### Phase 0 Chunk 0B
//! - Two global flags: `--exit-zero-on-no-match` (collapses §0.7
//!   `no_match`→0) and `--no-cache` (D6/§0.10 cache opt-out, threaded
//!   into `evolution`).
//! - Per-verb additions: `def --regex` (§0.6 opt-in to old substring
//!   behaviour), `trace --force` (§0.9 mutation-safety override).
//! - The dispatcher derives the process exit code from the envelope's
//!   `exit_status` (§0.7) and renders `warnings` to stderr.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process;

use ast_grep_language::Language;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

use crate::error::Result;
use crate::output::{
  render_context_markdown, render_delete_markdown, render_diagnostics_stderr,
  render_discover_markdown, render_discover_tree, render_envelope_github, render_envelope_json,
  render_envelope_ndjson, render_envelope_sarif, render_evolution_markdown, render_find_columns,
  render_find_paths_only, render_grep_hits_markdown, render_impact_dot, render_impact_markdown,
  render_impact_mermaid, render_pr_triage_markdown, render_ref_hits_markdown,
  render_symbol_hits_markdown, render_trace_markdown, stdout_line, ContextLines, FileColumn,
  OutputFormat,
};
use crate::schema::LineContext;
use lmprobe_engine::file_actions;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum AstFpCutArg {
  Off,
  Annotate,
  Suppress,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum SinkEvidenceArg {
  Off,
  On,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum SecretsSourceArg {
  Filesystem,
  Git,
}

#[derive(Subcommand)]
enum SecretsRecipesCmd {
  /// List credential sink recipes.
  #[command(after_help = "Example:\n  lmprobe secrets-recipes list --provider aws")]
  List {
    /// Limit rows to a provider id such as `aws` or `stripe`.
    #[clap(long)]
    provider: Option<String>,
    /// Limit rows to a language such as `python`, `javascript`, `go`, or `java`.
    #[clap(long)]
    language: Option<String>,
  },
  /// Show a generated credential sink GraphQL recipe.
  #[command(after_help = "Example:\n  lmprobe secrets-recipes show aws/python")]
  Show {
    /// Recipe key: <provider>/<language>[/<sdk>].
    key: String,
  },
}

#[derive(Subcommand)]
enum SecretsSourcesCmd {
  /// List credential source recipes.
  #[command(after_help = "Example:\n  lmprobe secrets-sources list --category env")]
  List {
    /// Limit rows to a source id such as `env-aws` or `dotenv-loader`.
    #[clap(long)]
    source: Option<String>,
    /// Limit rows to a language such as `python`, `javascript`, `go`, or `java`.
    #[clap(long)]
    language: Option<String>,
    /// Limit rows to a source category such as `env`, `vault`, or `dotenv`.
    #[clap(long)]
    category: Option<String>,
    /// Limit rows to a provider id such as `aws`, `stripe`, or `github`.
    #[clap(long)]
    provider: Option<String>,
  },
  /// Show a generated credential source GraphQL recipe.
  #[command(after_help = "Example:\n  lmprobe secrets-sources show env-aws/python")]
  Show {
    /// Recipe key: <source>/<language>[/<sdk>].
    key: String,
  },
}

impl From<SecretsSourceArg> for crate::secrets::SecretsSourceMode {
  fn from(value: SecretsSourceArg) -> Self {
    match value {
      SecretsSourceArg::Filesystem => Self::Filesystem,
      SecretsSourceArg::Git => Self::Git,
    }
  }
}

impl From<SinkEvidenceArg> for lmprobe_secrets::SinkEvidenceMode {
  fn from(value: SinkEvidenceArg) -> Self {
    match value {
      SinkEvidenceArg::Off => Self::Off,
      SinkEvidenceArg::On => Self::On,
    }
  }
}

impl From<AstFpCutArg> for lmprobe_secrets::AstFpCutMode {
  fn from(value: AstFpCutArg) -> Self {
    match value {
      AstFpCutArg::Off => Self::Off,
      AstFpCutArg::Annotate => Self::Annotate,
      AstFpCutArg::Suppress => Self::Suppress,
    }
  }
}

/// §1.6 — single render-by-format dispatcher. Per-verb markdown
/// renderers are wrapped in a closure; everything else is generic
/// over `serde::Serialize`.
///
/// §4.4 — Mermaid / DOT requested on a non-graph verb fall through
/// to a `FORMAT_NOT_APPLICABLE` stderr message and exit 2 (handled
/// by the caller emitting the diagnostic). Graph-shaped verbs use
/// `render_by_format_with_graph` instead, which routes Mermaid/DOT
/// to verb-specific graph renderers.
/// §4.7 — refuse mutation verbs under `--workspace` and exit 2.
/// Returns Ok(()) if no workspace is set; Err with the diagnostic
/// (already rendered to stderr) when the verb is read-only-only.
fn refuse_mutation_under_workspace(
  workspace: Option<&crate::workspace::WorkspaceCtx>,
  verb: &str,
) -> Result<()> {
  if workspace.is_some() {
    if crate::program_output::enabled() {
      return Err(crate::error::anyhow!(
        "WORKSPACE_READ_ONLY: mutation verbs are not allowed in --workspace mode"
      ));
    }
    eprintln!(
      "lmprobe: {} [WORKSPACE_READ_ONLY]: mutation verbs are not allowed in --workspace mode",
      verb
    );
    std::process::exit(2);
  }
  Ok(())
}

/// §4.7 — fan-out helper for `Vec<SymbolHit>`-shaped envelopes.
/// Runs `op(root)` for each workspace root, tags each hit's
/// `origin_root` with the root's label, and merges into one Vec.
fn fan_out_symbol_hits<F>(
  workspace: &crate::workspace::WorkspaceCtx,
  mut op: F,
) -> Result<(
  Vec<crate::schema::SymbolHit>,
  Vec<crate::schema::Diagnostic>,
)>
where
  F: FnMut(
    &std::path::Path,
  ) -> Result<(
    Vec<crate::schema::SymbolHit>,
    Vec<crate::schema::Diagnostic>,
  )>,
{
  let mut all_hits = Vec::new();
  let mut all_warnings = Vec::new();
  let mut seen_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
  for root in &workspace.roots {
    let (hits, warnings) = op(&root.path)?;
    for mut h in hits {
      h.origin_root = Some(root.label.clone());
      // §4.7 cross-root dedup by canonical absolute path.
      let canonical = std::fs::canonicalize(&h.file_path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| h.file_path.clone());
      if !seen_paths.insert(canonical.clone()) {
        continue;
      }
      all_hits.push(h);
    }
    all_warnings.extend(warnings);
  }
  Ok((all_hits, all_warnings))
}

fn collect_read_paths(path: Option<PathBuf>, paths: Vec<PathBuf>) -> Vec<PathBuf> {
  let mut roots = Vec::new();
  if let Some(path) = path {
    roots.push(path);
  }
  roots.extend(paths);
  if roots.is_empty() {
    roots.push(PathBuf::from("."));
  }
  roots
}

fn explicit_missing_read_path_diagnostics(
  path: Option<&PathBuf>,
  paths: &[PathBuf],
) -> Vec<ResultDiagnostic> {
  path
    .into_iter()
    .chain(paths.iter())
    .filter(|p| !p.exists())
    .map(|p| {
      ResultDiagnostic::new(
        "PATH_NOT_FOUND",
        format!("path does not exist: {}", p.display()),
      )
      .with_hint("check the path spelling or remove the explicit path argument")
    })
    .collect()
}

fn should_render_diagnostics_stderr(
  format: OutputFormat,
  status: ExitStatus,
  partial: bool,
  quiet: bool,
  verbose: bool,
) -> bool {
  if quiet {
    return false;
  }
  if verbose {
    return true;
  }
  !(format.resolve() == OutputFormat::Json && !matches!(status, ExitStatus::Error) && !partial)
}

fn search_name_positional_as_path_hint(path: &PathBuf) -> &'static str {
  if path.is_file() {
    "with --name, this file path was accepted as PATH; use `lmprobe search --name <X> --path <FILE>` to avoid this warning"
  } else if path.is_dir() {
    "with --name, this directory path was accepted as PATH; use `lmprobe search --name <X> --path <DIR>` or `cd <DIR> && lmprobe search --name <X>` to avoid this warning"
  } else {
    "with --name, this positional was accepted as PATH; use `lmprobe search --name <X> --path <PATH>` if this is a path"
  }
}

fn grep_pattern_parse_hint(args: &[OsString]) -> Option<&'static str> {
  const GREP_HINT: &str =
    "`grep` keeps PATTERN positional in v1; rerun as `lmprobe grep <PATTERN> [PATH]`";
  const FIND_HINT: &str =
    "`find` keeps PATTERN positional, like `grep`/`search`; rerun as `lmprobe find <PATTERN> [PATH]`";

  let (command, command_idx) = actual_top_level_subcommand(args)?;
  let hint = match command.as_str() {
    "grep" => GREP_HINT,
    "find" => FIND_HINT,
    _ => return None,
  };

  for arg in args
    .iter()
    .skip(command_idx + 1)
    .map(|arg| arg.to_string_lossy())
  {
    if arg == "--" {
      break;
    }
    if arg == "-p" || arg == "--pattern" || arg.starts_with("--pattern=") {
      return Some(hint);
    }
  }
  None
}

fn multiple_positional_paths_parse_hint(args: &[OsString]) -> Option<&'static str> {
  let (verb, command_idx) = actual_top_level_subcommand(args)?;
  if !matches!(verb.as_str(), "grep" | "find") {
    return None;
  }

  let positional_count = grep_find_positional_count(args, command_idx);
  if positional_count < 3 {
    return None;
  }

  match verb.as_str() {
    "grep" => Some(
      "`grep` accepts at most one positional PATH; for multiple roots use repeatable `--path <PATH>`: `lmprobe grep <PATTERN> --path <PATH1> --path <PATH2>`",
    ),
    "find" => Some(
      "`find` accepts at most one positional PATH; for multiple roots use repeatable `--path <PATH>`: `lmprobe find <REGEX> --path <PATH1> --path <PATH2>`",
    ),
    _ => None,
  }
}

fn actual_top_level_subcommand(args: &[OsString]) -> Option<(String, usize)> {
  let mut iter = args.iter().enumerate().skip(1).peekable();
  while let Some((idx, arg)) = iter.next() {
    let arg = arg.to_string_lossy();
    if arg == "--" {
      return None;
    }
    if is_top_level_subcommand(&arg) {
      return Some((arg.into_owned(), idx));
    }
    if option_takes_value(&arg) && !arg.contains('=') {
      let _ = iter.next();
    }
  }
  None
}

fn grep_find_positional_count(args: &[OsString], command_idx: usize) -> usize {
  let mut count = 0;
  let mut after_double_dash = false;
  let mut iter = args.iter().skip(command_idx + 1).peekable();
  while let Some(arg) = iter.next() {
    let arg = arg.to_string_lossy();
    if after_double_dash {
      count += 1;
      continue;
    }
    if arg == "--" {
      after_double_dash = true;
      continue;
    }
    if arg.starts_with('-') {
      if option_takes_value(&arg) && !arg.contains('=') {
        let _ = iter.next();
      }
      continue;
    }
    count += 1;
  }
  count
}

fn is_top_level_subcommand(arg: &str) -> bool {
  matches!(
    arg,
    "discover"
      | "search"
      | "def"
      | "ref"
      | "list-kinds"
      | "callers"
      | "imports"
      | "decorations"
      | "evolution"
      | "callees"
      | "supertypes"
      | "subtypes"
      | "overrides"
      | "exports"
      | "impact"
      | "context"
      | "history"
      | "trace"
      | "ast-diff"
      | "fix"
      | "secrets"
      | "grep"
      | "find"
      | "delete"
      | "serve"
      | "query"
  )
}

fn option_takes_value(arg: &str) -> bool {
  let option = arg.split_once('=').map(|(name, _)| name).unwrap_or(arg);
  matches!(
    option,
    "--lang"
      | "-l"
      | "--globs"
      | "-G"
      | "--exclude-glob"
      | "--threads"
      | "-j"
      | "--after"
      | "-A"
      | "--before"
      | "-B"
      | "--context"
      | "-C"
      | "--limit"
      | "--max-filesize"
      | "--type"
      | "-t"
      | "--format"
      | "--path"
      | "--display"
      | "--older-than"
      | "--action"
      | "--filter"
      | "--prune"
      | "--config"
      | "--max-target-megabytes"
  )
}

fn parse_recovery_hint(err: &clap::Error, args: &[OsString]) -> Option<&'static str> {
  grep_pattern_parse_hint(args).or_else(|| {
    matches!(err.kind(), clap::error::ErrorKind::UnknownArgument)
      .then(|| multiple_positional_paths_parse_hint(args))
      .flatten()
  })
}

fn render_parse_error(err: clap::Error, args: &[OsString]) -> ! {
  if let Some(hint) = parse_recovery_hint(&err, args) {
    let rendered = err.to_string();
    if let Some((body, _)) = rendered.rsplit_once("\nFor more information, try '--help'.") {
      eprintln!("{body}\n\n  hint: {hint}\n\nFor more information, try '--help'.");
    } else {
      eprint!("{rendered}");
      if !rendered.ends_with('\n') {
        eprintln!();
      }
      eprintln!("\n  hint: {hint}");
    }
    process::exit(err.exit_code());
  }
  err.exit()
}

fn render_by_format<E: serde::Serialize>(
  env: &E,
  format: OutputFormat,
  verb: &str,
  md: impl FnOnce(&E),
) {
  match format.resolve() {
    OutputFormat::Markdown => md(env),
    OutputFormat::Json => render_envelope_json(env),
    OutputFormat::Ndjson => render_envelope_ndjson(env),
    OutputFormat::Sarif => render_envelope_sarif(env, verb),
    OutputFormat::Github => render_envelope_github(env, verb),
    OutputFormat::Mermaid | OutputFormat::Dot => {
      eprintln!(
                "lmprobe: {}: --format {:?} is only valid on graph-shaped verbs (impact, supertypes, subtypes, callers, callees, context); falling back to JSON",
                verb, format
            );
      render_envelope_json(env);
    }
    OutputFormat::Auto => unreachable!(),
  }
}

/// §4.4 — graph-shaped dispatcher: extends `render_by_format` with
/// Mermaid + DOT renderers. Used by impact / supertypes / subtypes /
/// callers / callees.
fn render_by_format_with_graph<E: serde::Serialize>(
  env: &E,
  format: OutputFormat,
  verb: &str,
  md: impl FnOnce(&E),
  mermaid: impl FnOnce(&E),
  dot: impl FnOnce(&E),
) {
  match format.resolve() {
    OutputFormat::Markdown => md(env),
    OutputFormat::Json => render_envelope_json(env),
    OutputFormat::Ndjson => render_envelope_ndjson(env),
    OutputFormat::Sarif => render_envelope_sarif(env, verb),
    OutputFormat::Github => render_envelope_github(env, verb),
    OutputFormat::Mermaid => mermaid(env),
    OutputFormat::Dot => dot(env),
    OutputFormat::Auto => unreachable!(),
  }
}

fn scope_code_only(scope: Scope) -> bool {
  matches!(scope, Scope::Code)
}

/// §1.2 — combine the seven relational flags into a single composite
/// YAML rule. If only one flag is set, return its YAML (already in
/// the right form). If multiple, wrap them in `all: [ … ]`. If `-r
/// FILE` is set, that file's content takes precedence — it's a full
/// SerializableRule (no further wrapping).
fn compose_relational_yaml(
  has: Option<&str>,
  inside: Option<&str>,
  precedes: Option<&str>,
  follows: Option<&str>,
  not: Option<&str>,
  all: &[String],
  any: &[String],
  rule: Option<&std::path::Path>,
) -> Result<Option<String>> {
  if let Some(p) = rule {
    let s = std::fs::read_to_string(p)
      .map_err(|e| crate::error::anyhow!("failed to read {}: {}", p.display(), e))?;
    return Ok(Some(s));
  }

  fn wrap(key: &str, body: &str) -> String {
    // `body` is either "k: v" shorthand or "{k: v}" flow YAML.
    // Wrap so we get `<key>: <body>` as a SerializableRule field.
    let trimmed = body.trim();
    if trimmed.starts_with('{') || trimmed.contains('\n') {
      format!("{}: {}", key, trimmed)
    } else {
      format!("{}: {{ {} }}", key, trimmed)
    }
  }

  let mut clauses: Vec<String> = Vec::new();
  if let Some(b) = has {
    clauses.push(wrap("has", b));
  }
  if let Some(b) = inside {
    clauses.push(wrap("inside", b));
  }
  if let Some(b) = precedes {
    clauses.push(wrap("precedes", b));
  }
  if let Some(b) = follows {
    clauses.push(wrap("follows", b));
  }
  if let Some(b) = not {
    let trimmed = b.trim();
    let body = if trimmed.starts_with('{') || trimmed.contains('\n') {
      trimmed.to_string()
    } else {
      format!("{{ {} }}", trimmed)
    };
    clauses.push(format!("not: {}", body));
  }
  for a in all {
    let trimmed = a.trim();
    let body = if trimmed.starts_with('{') || trimmed.contains('\n') {
      trimmed.to_string()
    } else {
      format!("{{ {} }}", trimmed)
    };
    // Each --all clause is itself a SerializableRule. Compose by
    // wrapping each in an entry and aggregating below.
    clauses.push(body);
    let _ = "_all_marker";
  }
  let any_yaml: Option<String> = if !any.is_empty() {
    let inner: Vec<String> = any
      .iter()
      .map(|y| {
        let t = y.trim();
        if t.starts_with('{') || t.contains('\n') {
          t.to_string()
        } else {
          format!("{{ {} }}", t)
        }
      })
      .collect();
    Some(format!("any: [{}]", inner.join(", ")))
  } else {
    None
  };

  if clauses.is_empty() && any_yaml.is_none() {
    return Ok(None);
  }
  if clauses.len() == 1 && any_yaml.is_none() {
    return Ok(Some(clauses.into_iter().next().unwrap()));
  }
  // Multiple: wrap in `all`. each clause becomes `{ <k>: <v> }`.
  let mut combined = clauses
    .into_iter()
    .map(|c| format!("{{ {} }}", c))
    .collect::<Vec<_>>();
  if let Some(y) = any_yaml {
    combined.push(format!("{{ {} }}", y));
  }
  Ok(Some(format!("all: [{}]", combined.join(", "))))
}
use crate::langs::{PortableKind, SgLang};
use crate::schema::Diagnostic as ResultDiagnostic;
use crate::schema::ExitStatus;
use crate::sensors::{self, PatternMode, SearchConstraints, WalkOpts};
use lmprobe_registry::taxonomy_for;

// GraphQL is part of the core surface (Phase 4E). The HTTP daemon
// imports (axum, async-graphql-axum) stay opt-in under `--features serve`.
use async_graphql::{EmptySubscription, Error as GqlError, ErrorExtensions, Object, Schema};
#[cfg(feature = "serve")]
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
#[cfg(feature = "serve")]
use axum::{extract::State, routing::post, Router};

#[cfg(feature = "serve")]
fn is_loopback_addr(addr: &str) -> bool {
  // Strip the port if present.
  let host = addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr);
  let host = host.trim_start_matches('[').trim_end_matches(']');
  matches!(host, "127.0.0.1" | "::1" | "localhost")
}

/// Phase 6B M4 — clap value parser for `--recent-history-limit`.
/// Rejects `0` and negatives; accepts any usize ≥ 1.
fn parse_recent_history_limit(s: &str) -> std::result::Result<usize, String> {
  let n: usize = s
    .parse()
    .map_err(|e| format!("expected a positive integer, got `{}`: {}", s, e))?;
  if n < 1 {
    return Err("must be ≥ 1 (use `--recent-history-limit 1` for a single entry)".to_string());
  }
  Ok(n)
}

fn build_walk_opts(common: &CommonArgs) -> WalkOpts {
  WalkOpts {
    threads: common.threads,
    no_ignore: common.no_ignore,
    no_default_excludes: common.no_default_excludes,
    quick_project_roots: false,
    max_filesize: Some(common.max_filesize),
    result_limit: common.limit,
    globs: common.globs.clone(),
    exclude_globs: common.exclude_globs.clone(),
    type_filter: common.r#type.clone(),
    lang_filter: common
      .lang
      .as_deref()
      // For sensors other than `search` (which uses lang for the
      // pattern engine), --lang doubles as a walker include
      // filter so `lmprobe ref Foo -l rust` ignores .py files.
      .and_then(parse_lang),
  }
}

fn attach_grep_context(hits: &mut [crate::schema::GrepHit], ctx: ContextLines) {
  if ctx.is_off() {
    return;
  }
  use std::collections::HashMap;
  let mut file_cache: HashMap<String, Vec<String>> = HashMap::new();

  for hit in hits {
    let lines = file_cache.entry(hit.file_path.clone()).or_insert_with(|| {
      std::fs::read_to_string(&hit.file_path)
        .unwrap_or_default()
        .lines()
        .map(|s| s.to_string())
        .collect()
    });

    let target_idx = hit.line_number.saturating_sub(1);
    let before_start = target_idx.saturating_sub(ctx.before);
    hit.before_context = (before_start..target_idx)
      .filter_map(|i| {
        lines.get(i).map(|line_content| LineContext {
          line_number: i + 1,
          line_content: line_content.clone(),
        })
      })
      .collect();

    let after_end = (target_idx + ctx.after + 1).min(lines.len());
    hit.after_context = ((target_idx + 1)..after_end)
      .filter_map(|i| {
        lines.get(i).map(|line_content| LineContext {
          line_number: i + 1,
          line_content: line_content.clone(),
        })
      })
      .collect();
  }
}

fn attach_symbol_source_context(hits: &mut [crate::schema::SymbolHit], around: Option<usize>) {
  let Some(around) = around else {
    return;
  };
  use std::collections::HashMap;
  let mut file_cache: HashMap<String, Vec<String>> = HashMap::new();

  for hit in hits {
    let lines = file_cache.entry(hit.file_path.clone()).or_insert_with(|| {
      std::fs::read_to_string(&hit.file_path)
        .unwrap_or_default()
        .lines()
        .map(|s| s.to_string())
        .collect()
    });

    let target_idx = hit.line_start.saturating_sub(1) as usize;
    let start = target_idx.saturating_sub(around);
    let end = (target_idx + around + 1).min(lines.len());
    hit.source_context = (start..end)
      .filter_map(|idx| {
        lines.get(idx).map(|line_content| LineContext {
          line_number: idx + 1,
          line_content: line_content.clone(),
        })
      })
      .collect();
  }
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum Strictness {
  Cst,
  Smart,
  Ast,
  Relaxed,
  Signature,
  Template,
}

impl From<Strictness> for ast_grep_core::MatchStrictness {
  fn from(s: Strictness) -> Self {
    match s {
      Strictness::Cst => ast_grep_core::MatchStrictness::Cst,
      Strictness::Smart => ast_grep_core::MatchStrictness::Smart,
      Strictness::Ast => ast_grep_core::MatchStrictness::Ast,
      Strictness::Relaxed => ast_grep_core::MatchStrictness::Relaxed,
      Strictness::Signature => ast_grep_core::MatchStrictness::Signature,
      Strictness::Template => ast_grep_core::MatchStrictness::Template,
    }
  }
}

/// CLI-side wrapper that re-exports `lmprobe_engine::impact::ImpactDirection`
/// with a `clap::ValueEnum` derive on top.
///
/// Phase 5A workspace split: the canonical type lives in engine
/// (it's a sensor arg + a GraphQL Enum). clap::ValueEnum can't
/// be derived across crates, so the binary holds a parallel enum
/// + conversion. Two variants only — cheap to keep in sync.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImpactDirection {
  /// "Who calls me?" — recurse callers.
  Upstream,
  /// "What do I call?" — recurse callees.
  Downstream,
}

impl From<ImpactDirection> for lmprobe_engine::impact::ImpactDirection {
  fn from(d: ImpactDirection) -> Self {
    match d {
      ImpactDirection::Upstream => lmprobe_engine::impact::ImpactDirection::Upstream,
      ImpactDirection::Downstream => lmprobe_engine::impact::ImpactDirection::Downstream,
    }
  }
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrTriageSinceArg {
  /// Include PR comments/reviews after the author's latest issue comment.
  LastAuthorComment,
  /// Include all fetched PR comments/reviews.
  All,
}

impl From<PrTriageSinceArg> for lmprobe_engine::pr_triage::PrTriageSince {
  fn from(value: PrTriageSinceArg) -> Self {
    match value {
      PrTriageSinceArg::LastAuthorComment => {
        lmprobe_engine::pr_triage::PrTriageSince::LastAuthorComment
      }
      PrTriageSinceArg::All => lmprobe_engine::pr_triage::PrTriageSince::All,
    }
  }
}

fn impact_file_filter(
  tests_only: bool,
  exclude_tests: bool,
) -> lmprobe_engine::impact::ImpactFileFilter {
  match (tests_only, exclude_tests) {
    (true, false) => lmprobe_engine::impact::ImpactFileFilter::TestsOnly,
    (false, true) => lmprobe_engine::impact::ImpactFileFilter::ExcludeTests,
    _ => lmprobe_engine::impact::ImpactFileFilter::All,
  }
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum Scope {
  Code,
  All,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum RouteFramework {
  Fastapi,
  Generic,
}

impl RouteFramework {
  fn as_str(self) -> &'static str {
    match self {
      RouteFramework::Fastapi => "fastapi",
      RouteFramework::Generic => "generic",
    }
  }
}

fn parse_lang(s: &str) -> Option<SgLang> {
  match s.to_ascii_lowercase().as_str() {
    "bash" | "sh" => Some(SgLang::Bash),
    "c" => Some(SgLang::C),
    "cpp" | "c++" | "cxx" => Some(SgLang::Cpp),
    "csharp" | "cs" => Some(SgLang::CSharp),
    "css" => Some(SgLang::Css),
    "dart" => Some(SgLang::Dart),
    "elixir" | "ex" | "exs" => Some(SgLang::Elixir),
    "go" | "golang" => Some(SgLang::Go),
    "haskell" | "hs" => Some(SgLang::Haskell),
    "hcl" | "tf" | "terraform" => Some(SgLang::Hcl),
    "html" => Some(SgLang::Html),
    "java" => Some(SgLang::Java),
    "javascript" | "js" => Some(SgLang::JavaScript),
    "json" => Some(SgLang::Json),
    "kotlin" | "kt" => Some(SgLang::Kotlin),
    "lua" => Some(SgLang::Lua),
    "nix" => Some(SgLang::Nix),
    "php" => Some(SgLang::Php),
    "python" | "py" => Some(SgLang::Python),
    "ruby" | "rb" => Some(SgLang::Ruby),
    "rust" | "rs" => Some(SgLang::Rust),
    "scala" => Some(SgLang::Scala),
    "solidity" | "sol" => Some(SgLang::Solidity),
    "swift" => Some(SgLang::Swift),
    "toml" => Some(SgLang::Toml),
    "tsx" => Some(SgLang::Tsx),
    "typescript" | "ts" => Some(SgLang::TypeScript),
    "xml" => Some(SgLang::Xml),
    "yaml" | "yml" => Some(SgLang::Yaml),
    _ => None,
  }
}

fn parse_inside_kind_filter(s: &str) -> crate::error::Result<Vec<String>> {
  let kinds = s
    .split(',')
    .map(|kind| kind.trim().to_ascii_lowercase())
    .filter(|kind| !kind.is_empty())
    .collect::<Vec<_>>();
  if kinds.is_empty() {
    return Err(crate::error::anyhow!(
      "`--inside-kind` must include at least one kind, e.g. `for_statement,while_statement`"
    ));
  }
  Ok(kinds)
}

fn infer_pattern_lang_from_single_file(path: &Path) -> Option<SgLang> {
  if path.is_file() {
    SgLang::from_path(path)
  } else {
    None
  }
}

/// §1.5 — curated `sg`-style argument group, shared by every
/// result-producing verb. Every field is `global = true` so it
/// appears in every subcommand's `--help` and is parsed at any
/// position. Defined ONCE; never copy-pasted into each variant.
#[derive(clap::Args, Clone, Debug, Default)]
pub(crate) struct CommonArgs {
  /// Force the parser language; use --type to filter files by extension.
  #[clap(short = 'l', long, global = true, help_heading = "Common options")]
  pub lang: Option<String>,
  /// Include file globs (repeatable or comma-separated; prefix ! to exclude).
  #[clap(
    short = 'G',
    long,
    global = true,
    help_heading = "Common options",
    value_delimiter = ','
  )]
  pub globs: Vec<String>,
  /// Exclude file globs (repeatable or comma-separated).
  #[clap(
    long = "exclude-glob",
    global = true,
    help_heading = "Common options",
    value_delimiter = ','
  )]
  pub exclude_globs: Vec<String>,
  /// Include files excluded by .gitignore and .ignore.
  #[clap(long, global = true, help_heading = "Common options")]
  pub no_ignore: bool,
  /// Include dependency, build, VCS, and cache directories.
  #[clap(long, global = true, help_heading = "Common options")]
  pub no_default_excludes: bool,
  /// Worker thread count (default: num_cpus).
  #[clap(short = 'j', long, global = true, help_heading = "Common options")]
  pub threads: Option<usize>,
  /// Lines after grep/ref hits; grep JSON includes structured context.
  #[clap(short = 'A', long, global = true, help_heading = "Common options")]
  pub after: Option<usize>,
  /// Lines before grep/ref hits; grep JSON includes structured context.
  #[clap(short = 'B', long, global = true, help_heading = "Common options")]
  pub before: Option<usize>,
  /// Lines around grep/ref hits; grep JSON includes structured context.
  #[clap(short = 'C', long, global = true, help_heading = "Common options")]
  pub context: Option<usize>,
  /// Suppress non-error stderr (warnings stay in the envelope).
  #[clap(short = 'q', long, global = true, help_heading = "Common options")]
  pub quiet: bool,
  /// Also print incidental and non-error JSON diagnostics to stderr.
  #[clap(short = 'v', long, global = true, help_heading = "Common options")]
  pub verbose: bool,
  /// Cap the primary result list; composite sub-lists may need their own limit.
  #[clap(long, global = true, help_heading = "Common options")]
  pub limit: Option<usize>,
  /// Skip larger files (bytes); report FILE_TOO_LARGE.
  #[clap(
    long,
    global = true,
    help_heading = "Common options",
    default_value_t = 5_242_880
  )]
  pub max_filesize: u64,
  /// Case-insensitive matching for textual filters (grep, name).
  #[clap(short = 'i', long, global = true, help_heading = "Common options")]
  pub ignore_case: bool,
  /// Filter files by language extension without forcing a parser.
  #[clap(short = 't', long, global = true, help_heading = "Common options")]
  pub r#type: Option<String>,
  /// Emit one versioned program JSON result, including errors.
  #[clap(long, global = true, help_heading = "Common options")]
  pub json: bool,
  /// Atomically write program JSON to this file; stdout is quiet on success.
  #[clap(
    long,
    global = true,
    value_name = "FILE",
    help_heading = "Common options"
  )]
  pub outfile: Option<PathBuf>,
  /// Load a full GraphQL document (implies query and program JSON); - reads stdin.
  #[clap(
    long,
    visible_alias = "intfile",
    global = true,
    value_name = "FILE",
    help_heading = "Common options"
  )]
  pub infile: Option<PathBuf>,
  /// Output format (auto: markdown in a terminal, NDJSON when piped).
  #[clap(long, value_enum, global = true, help_heading = "Common options")]
  pub format: Option<OutputFormat>,
}

// Phase 6A #2: --version emits `lmprobe <semver> (commit <sha7>)`.
// CARGO_PKG_VERSION is built-in; LMPROBE_GIT_SHA is from build.rs.
const LMPROBE_VERSION: &str = concat!(
  env!("CARGO_PKG_VERSION"),
  " (commit ",
  env!("LMPROBE_GIT_SHA"),
  ")"
);

#[derive(Parser)]
#[command(
  name = "lmprobe",
  about = "Structural code search, Git history, and runtime evidence.",
  after_help = "Examples:\n  lmprobe search --name Parser .\n  lmprobe def --body handle_request src/\n  lmprobe grep 'TODO|FIXME' src/\n\nRun lmprobe <COMMAND> --help for options and an example.\nMost paths default to .; -l forces a parser, --type filters files.\nUse --format json for a single machine-readable envelope.",
  version = LMPROBE_VERSION
)]
struct Cli {
  /// Return exit 0 instead of 1 when no results match.
  #[clap(long, global = true, help_heading = "Common options")]
  exit_zero_on_no_match: bool,
  /// Bypass history cache where supported.
  #[clap(long, global = true, help_heading = "Common options")]
  no_cache: bool,
  /// Read across repositories listed in a roots: YAML file; mutations refuse this mode.
  #[clap(long, global = true, help_heading = "Common options")]
  workspace: Option<PathBuf>,
  #[command(flatten)]
  common: CommonArgs,
  #[clap(subcommand)]
  command: Commands,
}

/// `lmprobe history <verb>` subcommand group (Phase 3A §3.1).
///
/// All verbs read from the shared repo-wide commit cache; the cache
/// is incremental and only walks new commits since the last cached
/// `HEAD` advance.
#[derive(Subcommand, Debug, Clone)]
pub enum HistoryCmd {
  /// Show logic-changing commits for a symbol or source line.
  #[command(
    after_help = "Example:\n  lmprobe history blame src/lib.rs:42 --around 3 .\n\nUse --follow for file renames; --semantic adds signature-change categories."
  )]
  Blame {
    /// Symbol name OR `<path>:<line>` form for line blame.
    target: String,
    #[clap(default_value = ".")]
    path: PathBuf,
    /// Include N source lines before and after PATH:LINE targets.
    #[clap(long)]
    around: Option<usize>,
    /// Track the symbol across file renames.
    #[clap(long)]
    follow: bool,
    /// Classify body, parameter, and return-type changes.
    #[clap(long)]
    semantic: bool,
  },
  /// Find the first AST appearance of a symbol.
  #[command(
    after_help = "Example:\n  lmprobe history introduced Parser --follow .\n\nMatches an exact name; --follow tracks file renames."
  )]
  Introduced {
    name: String,
    #[clap(default_value = ".")]
    path: PathBuf,
    /// Accepted for compatibility; currently does not filter introduction history.
    #[clap(long, short = 'l')]
    lang: Option<String>,
    /// Track the symbol across file renames.
    #[clap(long)]
    follow: bool,
  },
  /// Find commits by message or code pattern.
  #[command(
    after_help = "Example:\n  lmprobe history commit -p 'fix' --since 30d .\n\nWithout -p, lists commits in the time window. --since accepts 30d, 12h, or 2w."
  )]
  Commit {
    /// Match commit messages or code changes.
    #[clap(long, short = 'p')]
    pattern: Option<String>,
    /// Time window, such as 30d, 12h, or 2w.
    #[clap(long)]
    since: Option<String>,
    /// Filter commits by author text.
    #[clap(long)]
    author: Option<String>,
    #[clap(default_value = ".")]
    path: PathBuf,
  },
  /// Find historical code changes matching an AST pattern.
  #[command(
    after_help = "Example:\n  lmprobe history diff -p 'foo($X)' --since 30d .\n\nReports before/after pattern context for matching commits."
  )]
  Diff {
    /// AST pattern to find in historical file changes.
    #[clap(long, short = 'p')]
    pattern: String,
    /// Time window, such as 30d, 12h, or 2w.
    #[clap(long)]
    since: Option<String>,
    #[clap(default_value = ".")]
    path: PathBuf,
  },
  /// Rank files by commit count in a time window.
  #[command(after_help = "Example:\n  lmprobe history churn --top 10 --since 30d .")]
  Churn {
    /// Maximum number of ranked files.
    #[clap(long, default_value = "20")]
    top: usize,
    /// Time window, such as 30d, 12h, or 2w.
    #[clap(long)]
    since: Option<String>,
    #[clap(default_value = ".")]
    path: PathBuf,
  },
  /// Rank files that often change with a given file.
  #[command(
    after_help = "Example:\n  lmprobe history co-change src/lib.rs --top 10 --since 30d .\n\nScans at most 1000 commits; truncated coverage is reported."
  )]
  CoChange {
    file: String,
    /// Time window, such as 30d, 12h, or 2w.
    #[clap(long)]
    since: Option<String>,
    /// Keep only the first N co-change rows after score sorting.
    #[clap(long)]
    top: Option<usize>,
    /// Keep only rows whose score is at least this value.
    #[clap(long)]
    min_score: Option<f32>,
    #[clap(default_value = ".")]
    path: PathBuf,
  },
  /// Find TODO/FIXME/XXX/HACK markers added in history.
  #[command(after_help = "Example:\n  lmprobe history todos --since 30d .")]
  Todos {
    /// Time window, such as 30d, 12h, or 2w.
    #[clap(long)]
    since: Option<String>,
    #[clap(default_value = ".")]
    path: PathBuf,
  },
  /// Show a symbol's changes through Git history.
  #[command(
    after_help = "Example:\n  lmprobe history evolution Parser .\n\nAlias of lmprobe evolution."
  )]
  Evolution {
    name: String,
    #[clap(default_value = ".")]
    path: PathBuf,
    /// Refresh symbol milestones instead of using cached milestones.
    #[clap(long)]
    force: bool,
    /// Use substring matching instead of an exact identifier.
    #[clap(long)]
    regex: bool,
  },
}

fn parse_file_predicate(
  value: &str,
) -> std::result::Result<crate::schema::FilePredicateInput, String> {
  let predicate: crate::schema::FilePredicateInput =
    serde_json::from_str(value).map_err(|error| error.to_string())?;
  lmprobe_engine::file_predicate::FilePredicate::compile(&predicate)?;
  Ok(predicate)
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum FileAction {
  Display,
  Delete,
}

#[derive(Subcommand)]
enum Commands {
  /// Read-only AWS queries with JSON parameters and structured results.
  #[command(
    after_help = "Example:\n  lmprobe aws query sts get-caller-identity --json\n\nUse aws catalog to list supported operations. Complex inputs use aws query --request-file request.json. Requires an installed, configured AWS CLI for queries."
  )]
  Aws(crate::aws_command::AwsArgs),
  /// Inspect common Git operations as parsed records.
  #[command(
    after_help = "Example:\n  lmprobe git --repo . status --json\n\nRead-only status, diff, log, show, branch, remote, rev-parse, ls-files, and blame. Paths are relative to the repository root. Use --json or --outfile for versioned results."
  )]
  Git(crate::git_command::GitArgs),
  /// Preview text replacements and line edits; use --apply to write.
  #[command(
    after_help = "Examples:\n  lmprobe edit notes.txt -p oldword -r newword --word --json\n  lmprobe edit notes.txt --insert-after 3 --text 'new line'\n  lmprobe edit notes.txt --script edits.json --apply --force\n\nDefault: read-only preview. JSON scripts sequence replace, insert_before, insert_after, delete_lines, replace_lines, prepend, append. --apply requires clean Git unless --force. Explicit files only."
  )]
  Edit(crate::edit_command::EditArgs),
  /// Find project roots and nested projects.
  #[command(
    after_help = "Example:\n  lmprobe discover --tree .\n\nUse --top-level-only for a compact monorepo map."
  )]
  Discover {
    #[clap(default_value = ".")]
    path: PathBuf,
    /// Render the parent/child project hierarchy as an
    /// indented tree (markdown only; JSON/SARIF carry the
    /// `children` field on each Project).
    #[clap(long)]
    tree: bool,
    /// Cap walker recursion depth.
    #[clap(long)]
    depth: Option<usize>,
    /// Emit only projects with no discovered project parent.
    #[clap(long)]
    top_level_only: bool,
  },
  /// Find declarations by name, kind, or AST pattern.
  #[command(
    after_help = "Example:\n  lmprobe search --name Parser .\n\nAST patterns: -p 'foo($X)' -l python src/. A single file can infer its language."
  )]
  Search {
    /// Deprecated positional query; prefer --name or -p.
    query: Option<String>,
    #[clap(default_value = ".")]
    path: PathBuf,
    /// Explicit search root; avoids positional query/path ambiguity.
    #[clap(long = "path", value_name = "PATH")]
    path_flag: Option<PathBuf>,
    /// Render the body-elided declaration outline.
    #[clap(long)]
    outline: bool,
    /// Return full declaration bodies.
    #[clap(long, conflicts_with = "outline")]
    body: bool,
    /// AST pattern; directories need -l, a single file can infer its language.
    #[clap(long, short = 'p', conflicts_with = "name")]
    pattern: Option<String>,
    /// Match strictness for the pattern engine.
    #[clap(long, value_enum)]
    strictness: Option<Strictness>,
    /// Case-insensitive substring filter on declaration names.
    #[clap(long, conflicts_with = "pattern")]
    name: Option<String>,
    /// Portable kind (function, class, struct, etc.) or parser-specific kind.
    #[clap(long)]
    kind: Option<String>,
    /// Tree-sitter literal kind — bypasses the taxonomy.
    #[clap(long)]
    raw_kind: Option<String>,
    /// Require a matching descendant (inline YAML rule).
    #[clap(long)]
    has: Option<String>,
    /// Require a matching ancestor (inline YAML rule).
    #[clap(long)]
    inside: Option<String>,
    /// Require a following match (inline YAML rule).
    #[clap(long)]
    precedes: Option<String>,
    /// Require a preceding match (inline YAML rule).
    #[clap(long)]
    follows: Option<String>,
    /// Exclude nodes matching this inline YAML rule.
    #[clap(long)]
    not: Option<String>,
    /// Require every rule (repeatable inline YAML).
    #[clap(long, num_args = 1)]
    all: Vec<String>,
    /// Require at least one rule (repeatable inline YAML).
    #[clap(long, num_args = 1)]
    any: Vec<String>,
    /// YAML rule file applied as a descendant filter on candidate declarations.
    #[clap(long, short = 'r')]
    rule: Option<PathBuf>,
    /// (deprecated, replaced by --has) substring on decoration text.
    #[clap(long, hide = true)]
    has_decoration: Option<String>,
    /// Filter by meaningful direct-child text (declarations or data structure children).
    #[clap(long)]
    has_child: Option<String>,
    #[clap(long, hide = true)]
    has_attr: Option<String>,
  },
  /// Find a symbol definition by exact name.
  #[command(
    after_help = "Example:\n  lmprobe def --body handle_request src/\n\nUse --regex for substring matching or --class to select a containing type."
  )]
  Def {
    name: String,
    #[clap(default_value = ".")]
    path: PathBuf,
    /// Use substring matching instead of an exact identifier.
    #[clap(long)]
    regex: bool,
    /// Restrict method/function definitions to a containing class/type name.
    #[clap(long = "class", value_name = "CLASS")]
    class_name: Option<String>,
    /// Require PATH to be a file and prefer concrete implementation
    /// hits over interface/trait signatures in that file.
    #[clap(long)]
    this_file: bool,
    /// Return full declaration bodies instead of skeletonized content.
    #[clap(long)]
    body: bool,
    /// Include N source lines before and after each definition hit.
    #[clap(long)]
    around: Option<usize>,
    /// Choose programming languages (code) or all supported languages (all).
    #[clap(long, value_enum, default_value = "code")]
    scope: Scope,
  },
  /// Find identifier references to a symbol.
  #[command(
    after_help = "Example:\n  lmprobe ref handle_request src/\n\nComments and doc strings are excluded unless requested."
  )]
  Ref {
    name: String,
    #[clap(default_value = ".")]
    path: PathBuf,
    /// Choose programming languages (code) or all supported languages (all).
    #[clap(long, value_enum, default_value = "code")]
    scope: Scope,
    /// Include identifier matches inside comment nodes.
    #[clap(long)]
    include_comments: bool,
    /// Include identifier matches inside doc-string nodes.
    #[clap(long)]
    include_docs: bool,
    /// Attach a mechanical local role classification to JSON/NDJSON hits.
    #[clap(long)]
    role: bool,
  },
  /// List portable and parser-specific kinds for a language.
  #[command(after_help = "Example:\n  lmprobe list-kinds rust")]
  ListKinds {
    /// Parser language (rust, python, etc.).
    #[clap(value_name = "LANG")]
    lang: Option<String>,
  },
  /// Find calls to a function or method.
  #[command(after_help = "Example:\n  lmprobe callers handle_request src/")]
  Callers {
    name: String,
    /// Keep only caller hits from likely test paths.
    #[clap(long)]
    tests_only: bool,
    #[clap(default_value = ".")]
    path: PathBuf,
  },
  /// Find imports of a module.
  #[command(after_help = "Example:\n  lmprobe imports requests src/")]
  Imports {
    name: String,
    #[clap(default_value = ".")]
    path: PathBuf,
  },
  /// Find uses of an annotation, decorator, or attribute.
  #[command(after_help = "Example:\n  lmprobe decorations derive src/")]
  Decorations {
    name: String,
    #[clap(default_value = ".")]
    path: PathBuf,
  },
  /// Show a symbol's changes through Git history.
  #[command(
    after_help = "Example:\n  lmprobe evolution Parser .\n\nExact name by default; --regex enables substring matching."
  )]
  Evolution {
    name: String,
    #[clap(default_value = ".")]
    path: PathBuf,
    /// Refresh symbol milestones instead of using cached milestones.
    #[clap(long)]
    force: bool,
    /// Use substring matching instead of an exact identifier.
    #[clap(long)]
    regex: bool,
  },
  /// Find functions or methods called by a symbol.
  #[command(after_help = "Example:\n  lmprobe callees handle_request src/")]
  Callees {
    name: String,
    #[clap(default_value = ".")]
    path: PathBuf,
  },
  /// Find a type's parents or implemented interfaces.
  #[command(after_help = "Example:\n  lmprobe supertypes MyClass src/")]
  Supertypes {
    name: String,
    #[clap(default_value = ".")]
    path: PathBuf,
  },
  /// Find subtypes or implementers of a type.
  #[command(after_help = "Example:\n  lmprobe subtypes MyTrait src/")]
  Subtypes {
    name: String,
    #[clap(default_value = ".")]
    path: PathBuf,
  },
  /// Find declarations marked as overriding a method.
  #[command(after_help = "Example:\n  lmprobe overrides render src/")]
  Overrides {
    name: String,
    #[clap(default_value = ".")]
    path: PathBuf,
  },
  /// Find exports of a symbol.
  #[command(after_help = "Example:\n  lmprobe exports handler src/")]
  Exports {
    name: String,
    #[clap(default_value = ".")]
    path: PathBuf,
  },
  /// Walk callers or callees to estimate change impact.
  #[command(
    after_help = "Example:\n  lmprobe impact handle_request --depth 2 src/\n\nDefault direction: upstream (callers). Use --direction downstream for callees."
  )]
  Impact {
    symbol: String,
    #[clap(default_value = ".")]
    path: PathBuf,
    /// Walk callers (upstream) or callees (downstream).
    #[clap(long, value_enum, default_value = "upstream")]
    direction: ImpactDirection,
    /// Maximum graph traversal depth.
    #[clap(long, default_value = "3")]
    depth: u32,
    /// Keep only impact nodes from likely test paths.
    #[clap(long, conflicts_with = "exclude_tests")]
    tests_only: bool,
    /// Drop impact nodes from likely test paths.
    #[clap(long, conflicts_with = "tests_only")]
    exclude_tests: bool,
  },
  /// Show GitHub pull request checks, reviews, and comments.
  #[command(
    after_help = "Example:\n  lmprobe pr-triage https://github.com/owner/repo/pull/123\n\nRequires the gh CLI with access to the repository."
  )]
  PrTriage {
    /// Include review/comment evidence since this cutoff.
    #[clap(long, value_enum, default_value = "last-author-comment")]
    since: PrTriageSinceArg,
    /// Shorten evidence excerpts for terminal use.
    #[clap(long)]
    compact: bool,
    /// GitHub pull request URL: https://github.com/<owner>/<repo>/pull/<number>.
    url: String,
  },
  /// Gather a symbol's definition, calls, imports, and history.
  #[command(
    after_help = "Example:\n  lmprobe context handle_request --around 3 src/\n\nUse --usage to anchor on references. Definition bundles include Git history; run in a Git repo."
  )]
  Context {
    /// Anchor on a definition. This is the default behavior.
    #[clap(long, conflicts_with = "usage")]
    def: bool,
    /// Anchor on usages/references instead of definitions.
    #[clap(long, conflicts_with = "def")]
    usage: bool,
    symbol: String,
    #[clap(default_value = ".")]
    path: PathBuf,
    /// Requested graph depth (currently returns one-hop evidence).
    #[clap(long, default_value = "1")]
    depth: u32,
    /// Follow one explicit relative import/re-export hop when the
    /// symbol is not declared under PATH.
    #[clap(long)]
    import_aware: bool,
    /// Include N source lines before and after the resolved context anchor.
    #[clap(long)]
    around: Option<usize>,
    /// Attach enclosing structural declaration chain metadata to usage anchors.
    #[clap(long)]
    parents: bool,
    /// Attach innermost enclosing structural body metadata/content to usage anchors.
    #[clap(long)]
    body: bool,
    /// Maximum recent-history entries (default: 3; must be at least 1).
    #[clap(long, value_parser = parse_recent_history_limit)]
    recent_history_limit: Option<usize>,
  },
  /// Explore Git blame, commits, churn, and symbol history.
  #[command(
    after_help = "Example:\n  lmprobe history churn --top 10 --since 30d .\n\nRun lmprobe history <COMMAND> --help for a specific history query."
  )]
  History {
    #[clap(subcommand)]
    cmd: HistoryCmd,
  },
  /// Inject logs, run a command, then restore source files.
  #[command(
    after_help = "Example:\n  lmprobe trace 'print($X)' -l python . --exec 'python3 app.py' --worktree\n\nRequires a clean Git tree unless --force is set. --worktree isolates changes at HEAD."
  )]
  Trace {
    query: String,
    #[clap(default_value = ".")]
    path: PathBuf,
    /// Shell command to run after instrumentation.
    #[clap(long)]
    exec: String,
    /// Override the language-specific log injection template.
    #[clap(long)]
    template: Option<String>,
    /// Python/Bash/R watch variables. Requires `--json-trace`.
    #[clap(long, value_delimiter = ',')]
    watch: Vec<String>,
    /// Emit and parse bounded `[TRACE_JSON]` runtime evidence for `--watch`.
    #[clap(long)]
    json_trace: bool,
    /// Include `shape` metadata for watched values when available.
    #[clap(long)]
    shape: bool,
    /// Include `len(...)` metadata for watched values when available.
    #[clap(long)]
    len: bool,
    /// Include object identity metadata for watched values.
    #[clap(long)]
    id: bool,
    /// Bound watched value repr payloads.
    #[clap(long, default_value_t = 120)]
    repr_limit: usize,
    /// Bypass the dirty-tree or non-Git safety check.
    #[clap(long)]
    force: bool,
    /// Run in a detached worktree at HEAD (optional location).
    #[clap(long, num_args = 0..=1, default_missing_value = "")]
    worktree: Option<String>,
    /// Save the instrumentation patch before rollback (optional destination).
    #[clap(long, num_args = 0..=1, default_missing_value = "")]
    keep_patch: Option<String>,
  },
  /// Compare declarations in a file between Git revisions.
  #[command(after_help = "Example:\n  lmprobe ast-diff src/lib.rs --from HEAD~1")]
  AstDiff {
    file: PathBuf,
    /// Base Git revision.
    #[clap(long)]
    from: String,
    /// Target Git revision.
    #[clap(long, default_value = "HEAD")]
    to: String,
  },
  /// Show changed Git lines with source or AST context.
  #[command(
    after_help = "Example:\n  lmprobe diff-context --from HEAD~1 --around 3 --parents .\n\nReports changed-line evidence, not semantic conclusions."
  )]
  DiffContext {
    /// Base Git revision.
    #[clap(long)]
    from: String,
    /// Target Git revision.
    #[clap(long, default_value = "HEAD")]
    to: String,
    /// Include N source lines around each changed-line anchor.
    #[clap(long)]
    around: Option<usize>,
    /// Attach enclosing structural declaration chain.
    #[clap(long)]
    parents: bool,
    /// Attach innermost enclosing structural body.
    #[clap(long)]
    body: bool,
    #[clap(default_value = ".")]
    path: PathBuf,
  },
  /// Find acquire/release anchors and local context.
  #[command(
    after_help = "Example:\n  lmprobe resource-sites . --acquire 'os.Open' --release 'Close' --around 3\n\nGo/Rust/JS/TS evidence only; does not prove resource cleanup."
  )]
  ResourceSites {
    path: PathBuf,
    /// Regex for acquire-like anchors.
    #[clap(long)]
    acquire: String,
    /// Regex for release-like anchors.
    #[clap(long)]
    release: String,
    /// Include N source lines around acquire anchors.
    #[clap(long)]
    around: Option<usize>,
    /// Attach enclosing structural declaration chain.
    #[clap(long)]
    parents: bool,
    /// Attach innermost enclosing structural body.
    #[clap(long)]
    body: bool,
  },
  /// Find JS/TS teardown calls and await/return evidence.
  #[command(
    after_help = "Example:\n  lmprobe async-leaks src/ --call close --around 2\n\nRepeat --call for more names. Does not prove Promise types or cleanup correctness."
  )]
  AsyncLeaks {
    path: PathBuf,
    /// Call name to anchor, e.g. `stop`; repeat for multiple names.
    #[clap(long, required = true)]
    call: Vec<String>,
    /// Include N source lines around call anchors.
    #[clap(long)]
    around: Option<usize>,
    /// Attach enclosing structural declaration chain.
    #[clap(long)]
    parents: bool,
    /// Attach innermost enclosing structural body.
    #[clap(long)]
    body: bool,
  },
  /// Find Go crypto algorithm uses and nearby FIPS evidence.
  #[command(
    after_help = "Example:\n  lmprobe crypto-sites . --algorithm sha1 --fips-check\n\nRepeat --algorithm for more names. Evidence only; not a compliance verdict."
  )]
  CryptoSites {
    path: PathBuf,
    /// Algorithm name to anchor, e.g. `sha1`; repeat for multiple names.
    #[clap(long = "algorithm", required = true)]
    algorithms: Vec<String>,
    /// Attach nearby FIPS / boringcrypto / GODEBUG / GOFIPS evidence lines.
    #[clap(long)]
    fips_check: bool,
    /// Include N source lines around algorithm anchors.
    #[clap(long)]
    around: Option<usize>,
    /// Attach enclosing structural declaration chain.
    #[clap(long)]
    parents: bool,
    /// Attach innermost enclosing structural body.
    #[clap(long)]
    body: bool,
  },
  /// Extract config keys and optional code references.
  #[command(
    after_help = "Example:\n  lmprobe config-keys package.json pyproject.toml\n\nUse --grep-code for lexical code anchors; these are not data-flow conclusions."
  )]
  ConfigKeys {
    #[clap(required = true)]
    paths: Vec<PathBuf>,
    /// Keep only an exact config key path, for example tool.pytest.ini_options.addopts.
    #[clap(long)]
    key: Option<String>,
    /// Search code-tier files for escaped leaf-key text and attach matching anchors.
    #[clap(long)]
    grep_code: bool,
    /// Include N source lines around matching code anchors.
    #[clap(long)]
    around: Option<usize>,
    /// Attach enclosing structural declaration chain to code anchors.
    #[clap(long)]
    parents: bool,
    /// Attach mechanical local role classification to code anchors.
    #[clap(long)]
    role: bool,
  },
  /// Scan files or Git history for potential secrets.
  #[command(
    after_help = "Example:\n  lmprobe secrets --format json .\n\nFindings are redacted. Use --source git for history or --config for custom rules."
  )]
  Secrets {
    #[clap(default_value = ".")]
    path: PathBuf,
    /// Gitleaks-compatible TOML rules (default: bundled rules).
    #[clap(long)]
    config: Option<PathBuf>,
    /// Skip files larger than this many MB, matching gitleaks'
    /// target-size terminology.
    #[clap(long, default_value_t = 5)]
    max_target_megabytes: usize,
    /// Follow symlinks during the filesystem walk.
    #[clap(long)]
    follow_symlinks: bool,
    /// Ignore ignore-files and use gitleaks-compatible finding exit codes.
    #[clap(long)]
    gitleaks_compat: bool,
    /// AST context mode: annotate findings, suppress non-code findings, or off.
    #[clap(long, value_enum, default_value_t = AstFpCutArg::Annotate)]
    ast_fp_cut: AstFpCutArg,
    /// Include value-independent credential sink evidence (syntax only).
    #[clap(long, value_enum, default_value_t = SinkEvidenceArg::Off)]
    sink_evidence: SinkEvidenceArg,
    /// Scan live files (filesystem) or full Git history (git).
    #[clap(long, value_enum, default_value_t = SecretsSourceArg::Filesystem)]
    source: SecretsSourceArg,
    /// Suppress findings whose fingerprints appear in a gitleaks JSON baseline.
    #[clap(long)]
    baseline: Option<PathBuf>,
  },
  /// List or show credential sink query recipes.
  #[command(
    after_help = "Example:\n  lmprobe secrets-recipes list --provider aws\n\nRun generated recipes with lmprobe query --recipe secrets/<KEY>."
  )]
  SecretsRecipes {
    #[clap(subcommand)]
    cmd: SecretsRecipesCmd,
  },
  /// List or show credential source query recipes.
  #[command(
    after_help = "Example:\n  lmprobe secrets-sources list --source env-aws\n\nRun generated recipes with lmprobe query --recipe secrets-source/<KEY>."
  )]
  SecretsSources {
    #[clap(subcommand)]
    cmd: SecretsSourcesCmd,
  },
  /// Find sensitive actions and nearby guard anchors.
  #[command(
    after_help = "Example:\n  lmprobe guard-sites . --sensitive 'Delete|exec' --guard 'auth|allowed' --around 4\n\nEvidence only; does not prove authorization correctness."
  )]
  GuardSites {
    path: PathBuf,
    /// Regex for sensitive-action anchors.
    #[clap(long)]
    sensitive: String,
    /// Regex for nearby guard-looking anchors.
    #[clap(long)]
    guard: String,
    /// Include N source lines around sensitive anchors and search guards in that window.
    #[clap(long)]
    around: Option<usize>,
    /// Attach enclosing structural declaration chain and prefer same-declaration guards.
    #[clap(long)]
    parents: bool,
    /// Attach innermost enclosing structural body.
    #[clap(long)]
    body: bool,
  },
  /// Extract HTTP methods and paths from OpenAPI files.
  #[command(
    after_help = "Example:\n  lmprobe openapi-facts openapi.yaml\n\nReads JSON/YAML paths without full OpenAPI validation."
  )]
  OpenapiFacts { path: PathBuf },
  /// Find route declarations and literal HTTP paths.
  #[command(
    after_help = "Example:\n  lmprobe route-anchors src/ --framework fastapi\n\nSupports FastAPI decorators and generic literal route calls."
  )]
  RouteAnchors {
    path: PathBuf,
    /// Route syntax to scan (auto-detected when omitted).
    #[clap(long, value_enum)]
    framework: Option<RouteFramework>,
    /// Include N source lines around route anchors.
    #[clap(long)]
    around: Option<usize>,
    /// Attach enclosing structural declaration chain.
    #[clap(long)]
    parents: bool,
    /// Attach innermost enclosing structural body.
    #[clap(long)]
    body: bool,
  },
  /// Find HTTP client calls with literal URLs.
  #[command(
    after_help = "Example:\n  lmprobe http-client-calls src/ --around 2\n\nSupports Python requests/httpx and JS/TS fetch/axios."
  )]
  HttpClientCalls {
    path: PathBuf,
    /// Include N source lines around client call anchors.
    #[clap(long)]
    around: Option<usize>,
    /// Attach enclosing structural declaration chain.
    #[clap(long)]
    parents: bool,
    /// Attach innermost enclosing structural body.
    #[clap(long)]
    body: bool,
  },
  /// Preview AST replacements; use --apply to write changes.
  #[command(
    after_help = "Example:\n  lmprobe fix -p 'console.log($X)' -r 'logger.info($X)' -l javascript src/\n\nDefault: dry run. --apply requires a clean Git tree unless --force is set."
  )]
  Fix {
    /// Source pattern (ast-grep metavars allowed: `$X`, `$$$`).
    #[clap(long, short = 'p')]
    pattern: String,
    /// Replacement template referencing the pattern's metavars.
    #[clap(long, short = 'r')]
    replacement: String,
    #[clap(default_value = ".")]
    path: PathBuf,
    /// Parser language (rust, python, etc.).
    #[clap(long, short = 'l')]
    lang: Option<String>,
    /// Write replacements via an audited worktree patch (default: dry run).
    #[clap(long)]
    apply: bool,
    /// Bypass the dirty-tree or non-Git safety check.
    #[clap(long)]
    force: bool,
  },
  /// Search file contents with a regular expression.
  #[command(
    after_help = "Example:\n  lmprobe grep 'TODO|FIXME' --path src --path tests\n\nPATTERN is positional. Use | for alternation; --parents/--body add AST context."
  )]
  Grep {
    pattern: String,
    /// Single compatibility path. When omitted, `grep` defaults
    /// to `.` unless one or more `--path <PATH>` flags are set.
    path: Option<PathBuf>,
    /// Additional scan roots (repeatable); defaults to . when no paths are given.
    #[clap(long = "path", value_name = "PATH")]
    paths: Vec<PathBuf>,
    /// Attach enclosing structural declaration chain metadata to JSON/NDJSON hits.
    #[clap(long)]
    parents: bool,
    /// Attach innermost enclosing structural body metadata/content to JSON/NDJSON hits.
    #[clap(long)]
    body: bool,
    /// Filter hits to those inside one of the listed AST ancestor kinds
    /// (comma-separated `tree-sitter` kind names).
    #[clap(long)]
    inside_kind: Option<String>,
    /// Compatibility flag; line numbers are already included.
    #[clap(short = 'n', long = "line-number")]
    line_number: bool,
    /// Attach a mechanical local role classification to JSON/NDJSON hits.
    #[clap(long)]
    role: bool,
    /// Narrow scans to npm roots or directories with at least five Perl files.
    #[clap(long)]
    quick_projects: bool,
  },
  /// Find paths with modification time and size; optionally filter or act.
  #[command(
    after_help = "Example:\n  lmprobe find '\\.lmctl$' . --older-than 1w --action delete\n\nDefault: display only. Add --force to execute deletion, or pipe to lmprobe delete.\nUse --display name for paths, --paths-only --null for shell pipelines."
  )]
  Find {
    pattern: String,
    /// Single compatibility path. When omitted, `find` defaults
    /// to `.` unless one or more `--path <PATH>` flags are set.
    path: Option<PathBuf>,
    /// Additional scan roots (repeatable); defaults to . when no paths are given.
    #[clap(long = "path", value_name = "PATH")]
    paths: Vec<PathBuf>,
    /// Narrow scans to npm roots or directories with at least five Perl files.
    #[clap(long)]
    quick_projects: bool,
    /// Print raw paths for shell pipelines instead of an output envelope.
    #[clap(long)]
    paths_only: bool,
    /// Separate --paths-only output with NUL bytes for xargs -0.
    #[clap(long)]
    null: bool,
    /// Display columns (default: name,lastmod,size); comma-separated.
    #[clap(
      long,
      value_enum,
      value_delimiter = ',',
      default_value = "name,lastmod,size",
      conflicts_with = "paths_only"
    )]
    display: Vec<FileColumn>,
    /// Select entries last modified more than this age ago (1w, 7d, 24h).
    #[clap(long, value_parser = file_actions::parse_age)]
    older_than: Option<std::time::Duration>,
    /// Built-in action (default: display); delete previews until --force is set.
    #[clap(long, value_enum, conflicts_with = "paths_only")]
    action: Option<FileAction>,
    /// Execute --action delete; refuses directories, symlinks, or changed files.
    #[clap(long, requires = "action")]
    force: bool,
    /// Select results with a JSON filesystem predicate (kind, has_entry, git_remote, all/any/not).
    #[clap(long, value_parser = parse_file_predicate)]
    filter: Option<crate::schema::FilePredicateInput>,
    /// Stop inside matching directories; they remain eligible results (JSON predicate).
    #[clap(long, value_parser = parse_file_predicate, conflicts_with = "quick_projects")]
    prune: Option<crate::schema::FilePredicateInput>,
  },
  /// Preview deleting selected files from stdin; --force executes deletion.
  #[command(
    after_help = "Example:\n  lmprobe delete\n\nPipe a selection: lmprobe find '\\.log$' . --older-than 1w | lmprobe delete\nReview the preview, then add --force to delete regular files.\nAccepts find JSON/NDJSON or raw paths; --null accepts NUL-separated paths."
  )]
  Delete {
    /// Execute deletion after validating the complete selection (default: preview).
    #[clap(long)]
    force: bool,
    /// Read raw NUL-separated paths, for find --paths-only --null or fd -0.
    #[clap(long)]
    null: bool,
    /// Display columns (default: name,lastmod,size); comma-separated.
    #[clap(
      long,
      value_enum,
      value_delimiter = ',',
      default_value = "name,lastmod,size"
    )]
    display: Vec<FileColumn>,
  },
  /// Find Python slice/unsliced-variable forwarding suspects.
  #[command(
    after_help = "Example:\n  lmprobe py-slice-candidates src/ --trace-plan\n\nLow-confidence evidence only. --trace-plan suggests runtime watch checks."
  )]
  PySliceCandidates {
    /// Include deterministic candidate ids and trace watch plan hints.
    #[clap(long)]
    trace_plan: bool,
    /// Required Python files or directories; use . to scan the current directory.
    #[clap(required = true)]
    paths: Vec<PathBuf>,
  },
  /// Extract Python call arguments for a callee.
  #[command(
    after_help = "Example:\n  lmprobe callsites foo --args --around 1 src/\n\n--args is required. Reports syntax, not overload or dynamic dispatch resolution."
  )]
  Callsites {
    callee: String,
    /// Enable argument extraction. Required for this first command slice.
    #[clap(long, required = true)]
    args: bool,
    /// Include N source lines around each callsite.
    #[clap(long)]
    around: Option<usize>,
    #[clap(default_value = ".")]
    path: PathBuf,
  },
  /// Start the optional GraphQL HTTP server.
  #[command(
    after_help = "Example:\n  lmprobe serve --addr 127.0.0.1:8080\n\nBuild with --features serve. Non-loopback binds require --auth-token."
  )]
  Serve {
    /// HTTP listen address; POST GraphQL documents to /graphql.
    #[clap(long, default_value = "127.0.0.1:8080")]
    addr: String,
    /// Require this bearer token on every request, including loopback.
    #[clap(long)]
    auth_token: Option<String>,
  },
  /// Run a GraphQL document or recipe in-process.
  #[command(
    after_help = "Example:\n  lmprobe query '{ discover(path: \".\") { projects { name } } }'\n\nUse - for stdin, @FILE for a document, and --var KEY=VALUE for variables."
  )]
  Query {
    /// GraphQL query source. Use `-` for stdin or `@path` for a file.
    #[clap(required_unless_present_any = ["recipe", "infile"], conflicts_with_all = ["recipe", "infile"])]
    query: Option<String>,
    /// Run a generated recipe, e.g. `secrets/aws/python` or
    /// `secrets-source/env-aws/python`.
    #[clap(long, value_name = "KEY", conflicts_with_all = ["query", "infile"])]
    recipe: Option<String>,
    /// With --recipe, execute every shape under the selected key.
    #[clap(long, requires = "recipe")]
    all_shapes: bool,
    /// Variable KEY=VALUE (repeatable; JSON values or plain strings).
    #[clap(long = "var", value_name = "KEY=VALUE")]
    vars: Vec<String>,
    /// JSON object of variables; --var entries override matching keys.
    #[clap(long, value_name = "PATH")]
    vars_file: Option<PathBuf>,
    /// Operation name for documents containing multiple operations.
    #[clap(long, value_name = "NAME")]
    operation_name: Option<String>,
  },
}

// Read only real options: skip option values and everything after --.
fn invocation_options(
  args: &[OsString],
) -> (bool, Option<PathBuf>, Option<PathBuf>, Option<String>) {
  fn collect(command: &clap::Command, values: &mut std::collections::HashSet<String>) {
    for arg in command
      .get_arguments()
      .filter(|arg| arg.get_action().takes_values())
    {
      if let Some(long) = arg.get_long() {
        values.insert(format!("--{long}"));
      }
      if let Some(short) = arg.get_short() {
        values.insert(format!("-{short}"));
      }
      for alias in arg.get_all_aliases().unwrap_or_default() {
        values.insert(format!("--{alias}"));
      }
    }
    for child in command.get_subcommands() {
      collect(child, values);
    }
  }
  let command = Cli::command();
  let commands: std::collections::HashSet<_> = command
    .get_subcommands()
    .map(|child| child.get_name().to_string())
    .collect();
  let mut values = std::collections::HashSet::new();
  collect(&command, &mut values);
  let mut json = false;
  let mut outfile = None;
  let mut infile = None;
  let mut has_command = None;
  let mut index = 1;
  while index < args.len() {
    let arg = args[index].to_string_lossy();
    if arg == "--" {
      break;
    }
    if has_command.is_none() && commands.contains(arg.as_ref()) {
      has_command = Some(arg.to_string());
    }
    let (flag, inline) = arg
      .split_once('=')
      .map(|(flag, value)| (flag, Some(OsString::from(value))))
      .unwrap_or((&arg, None));
    if flag == "--json" {
      json = true;
    }
    if values.contains(flag) {
      let value = inline.or_else(|| {
        args
          .get(index + 1)
          .filter(|value| value.as_os_str() == "-" || !value.to_string_lossy().starts_with('-'))
          .cloned()
      });
      if matches!(flag, "--outfile" | "--infile" | "--intfile") {
        json = true;
      }
      if flag == "--outfile" {
        outfile = value.clone().map(PathBuf::from);
      }
      if matches!(flag, "--infile" | "--intfile") {
        infile = value.clone().map(PathBuf::from);
      }
      if !arg.contains('=') && value.is_some() {
        index += 1;
      }
    }
    index += 1;
  }
  (
    json || outfile.is_some() || infile.is_some(),
    outfile,
    infile,
    has_command,
  )
}

fn normalize_invocation(mut args: Vec<OsString>) -> Vec<OsString> {
  for arg in &mut args {
    if arg == "--" {
      break;
    }
    if let Some(value) = arg.to_str() {
      if matches!(value, "-infile" | "-intfile") {
        *arg = OsString::from("--infile");
      } else if let Some(path) = value
        .strip_prefix("-infile=")
        .or_else(|| value.strip_prefix("-intfile="))
      {
        *arg = OsString::from(format!("--infile={path}"));
      }
    }
  }
  let (_, _, infile, has_command) = invocation_options(&args);
  if infile.is_some() && has_command.is_none() {
    args.insert(1, OsString::from("query"));
  }
  args
}

// Parsing may stop before a typed command exists. Conservatively protect any
// explicitly mentioned existing input path, excluding the outfile value itself.
// In this failure/help-only path a collision falls back to stdout, never writes.
fn protect_parser_inputs(args: &[OsString]) -> Result<()> {
  fn short_values(command: &clap::Command, flags: &mut std::collections::HashSet<char>) {
    for argument in command
      .get_arguments()
      .filter(|argument| argument.get_action().takes_values())
    {
      if let Some(short) = argument.get_short() {
        flags.insert(short);
      }
    }
    for child in command.get_subcommands() {
      short_values(child, flags);
    }
  }
  let mut short_flags = std::collections::HashSet::new();
  short_values(&Cli::command(), &mut short_flags);
  let mut index = 1;
  let mut literal = false;
  while index < args.len() {
    let value = args[index].to_string_lossy();
    if !literal && value == "--" {
      literal = true;
      index += 1;
      continue;
    }
    if !literal && value == "--outfile" {
      index += 2;
      continue;
    }
    if !literal && value.starts_with("--outfile=") {
      index += 1;
      continue;
    }
    let candidate = if !literal && value.starts_with("--") {
      value.split_once('=').map(|(_, value)| value)
    } else if !literal && value.starts_with('-') {
      value[1..]
        .char_indices()
        .find(|(_, flag)| short_flags.contains(flag))
        .map(|(offset, flag)| &value[1 + offset + flag.len_utf8()..])
        .map(|value| value.strip_prefix('=').unwrap_or(value))
        .filter(|value| !value.is_empty())
    } else {
      Some(value.as_ref())
    };
    if let Some(candidate) = candidate {
      // @ is file indirection only for query sources; it is a literal filename
      // in rule/config arguments. Failed parses conservatively protect both.
      let paths = std::iter::once(PathBuf::from(candidate))
        .chain(candidate.strip_prefix('@').map(PathBuf::from));
      crate::program_output::protect(paths).map_err(|error| crate::error::anyhow!(error))?;
    }
    index += 1;
  }
  Ok(())
}

fn protect_git_output(root: &Path) -> Result<()> {
  let result = protect_git_output_inner(root);
  if result.is_err() {
    crate::program_output::cancel_file();
  }
  result
}

fn protect_git_output_inner(root: &Path) -> Result<()> {
  let Some(target) = crate::program_output::target() else {
    return Ok(());
  };
  let repo = match git2::Repository::discover(root) {
    Ok(repo) => repo,
    Err(error) => {
      crate::program_output::cancel_file();
      return Err(crate::error::anyhow!(
        "GIT_REPOSITORY_ERROR: cannot validate outfile safety for {}: {error}",
        root.display()
      ));
    }
  };
  let admin = repo.path().canonicalize()?;
  let common = match std::fs::read_to_string(admin.join("commondir")) {
    Ok(value) if !value.trim().is_empty() => admin.join(value.trim()).canonicalize()?,
    Ok(_) => {
      return Err(crate::error::anyhow!(
        "empty Git common-directory reference"
      ))
    }
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => admin.clone(),
    Err(error) => return Err(error.into()),
  };
  let workdir = repo.workdir().map(|path| path.canonicalize()).transpose()?;
  if target.starts_with(&admin)
    || target.starts_with(&common)
    || workdir
      .as_ref()
      .is_some_and(|path| target.starts_with(path) && target.exists())
  {
    crate::program_output::cancel_file();
    return Err(crate::error::anyhow!("OUTFILE_SOURCE_CONFLICT: Git outfile overlaps repository metadata or an existing working-tree file"));
  }
  let mut inputs = vec![
    admin.join("HEAD"),
    admin.join("index"),
    common.join("config"),
    common.join("index"),
    common.join("packed-refs"),
  ];
  if let Some(workdir) = repo.workdir() {
    inputs.push(workdir.join(".git"));
    {
      let index = repo.index()?;
      for entry in index.iter() {
        #[cfg(unix)]
        {
          use std::os::unix::ffi::OsStringExt;
          inputs.push(workdir.join(std::ffi::OsString::from_vec(entry.path)));
        }
        #[cfg(not(unix))]
        {
          if let Ok(path) = String::from_utf8(entry.path) {
            inputs.push(workdir.join(path));
          }
        }
      }
    }
  }
  crate::program_output::protect(inputs).map_err(|error| crate::error::anyhow!(error))
}

/// The clap command tree (the lmapi schema generator walks it).
pub(crate) fn command() -> clap::Command {
  Cli::command()
}

pub fn run() -> Result<()> {
  // lmapi mode (lmapi protocol 6.1): only lmapi-shaped argv; everything else is unchanged.
  let lmapi_args: Vec<OsString> = std::env::args_os().skip(1).collect();
  if let Some(code) = crate::lmapi::maybe_run(&lmapi_args) {
    process::exit(code);
  }
  let args = normalize_invocation(std::env::args_os().collect());
  let (requested, outfile, infile, command) = invocation_options(&args);
  crate::program_output::enable(requested);
  if args.len() == 1 {
    Cli::command().print_help()?;
    stdout_line("");
    return Ok(());
  }
  let cli = match Cli::try_parse_from(args.clone()) {
    Ok(cli) => cli,
    Err(err) if requested => {
      if let Err(error) = crate::program_output::prepare(outfile.as_deref(), infile.as_deref()) {
        crate::program_output::error("OUTFILE_PREPARE_FAILED", error.to_string());
        process::exit(2);
      }
      if let Err(error) = protect_parser_inputs(&args) {
        crate::program_output::error("OUTFILE_SOURCE_CONFLICT", error.to_string());
        process::exit(2);
      }
      if command.as_deref() == Some("aws") {
        if let Err(error) = crate::aws_command::protect_configuration() {
          crate::program_output::cancel_file();
          crate::program_output::error("OUTFILE_SOURCE_CONFLICT", error.to_string());
          process::exit(2);
        }
      }
      if command.as_deref() == Some("git") {
        let mut roots = vec![PathBuf::from(".")];
        for (index, arg) in args.iter().enumerate().take_while(|(_, arg)| *arg != "--") {
          if arg == "--repo" {
            if let Some(value) = args.get(index + 1) {
              roots.push(PathBuf::from(value));
            }
          }
          if let Some(value) = arg.to_str().and_then(|arg| arg.strip_prefix("--repo=")) {
            roots.push(PathBuf::from(value));
          }
        }
        for root in roots {
          if let Err(error) = protect_git_output(&root) {
            crate::program_output::error("OUTFILE_SOURCE_CONFLICT", error.to_string());
            process::exit(2);
          }
        }
      }
      let code = err.exit_code();
      match err.kind() {
        clap::error::ErrorKind::DisplayHelp => render_envelope_json(
          &serde_json::json!({"schema":"lmprobe.help.v1","text":err.to_string(),"warnings":[],"exit_status":"matched"}),
        ),
        clap::error::ErrorKind::DisplayVersion => render_envelope_json(
          &serde_json::json!({"schema":"lmprobe.version.v1","version":err.to_string().trim(),"warnings":[],"exit_status":"matched"}),
        ),
        _ => crate::program_output::error("ARGUMENT_ERROR", err.to_string()),
      }
      let code = if crate::program_output::failed() {
        2
      } else {
        code
      };
      if code != 0 {
        process::exit(code);
      }
      return Ok(());
    }
    Err(err) => render_parse_error(err, &args),
  };
  let program = cli.common.json || cli.common.outfile.is_some() || cli.common.infile.is_some();
  crate::program_output::enable(program);
  let result = (|| -> Result<()> {
    if program {
      if cli
        .common
        .format
        .is_some_and(|format| format != OutputFormat::Json && format != OutputFormat::Auto)
      {
        return Err(crate::error::anyhow!(
          "PROGRAM_FORMAT_CONFLICT: program JSON cannot combine with a different --format"
        ));
      }
      if matches!(&cli.command, Commands::Serve { .. }) {
        return Err(crate::error::anyhow!("STREAMING_COMMAND: serve uses HTTP JSON; program JSON/outfile require a one-shot command"));
      }
      if matches!(
        &cli.command,
        Commands::Find {
          paths_only: true,
          ..
        }
      ) {
        return Err(crate::error::anyhow!(
          "PROGRAM_FORMAT_CONFLICT: --paths-only cannot combine with program JSON"
        ));
      }
      if cli.common.infile.is_some() && !matches!(&cli.command, Commands::Query { .. }) {
        return Err(crate::error::anyhow!(
          "INFILE_COMMAND_CONFLICT: infile is a GraphQL expression document; use query"
        ));
      }
      crate::program_output::prepare(cli.common.outfile.as_deref(), cli.common.infile.as_deref())
        .map_err(|error| crate::error::anyhow!("OUTFILE_PREPARE_FAILED: {error}"))?;
      if let Commands::Git(args) = &cli.command {
        protect_git_output(&args.repo)?;
      }
      if matches!(&cli.command, Commands::Aws(_)) {
        if let Err(error) = crate::aws_command::protect_configuration() {
          crate::program_output::cancel_file();
          return Err(error);
        }
      }
      let mut inputs: Vec<PathBuf> = match &cli.command {
        Commands::Aws(args) => crate::aws_command::input_paths(args),
        Commands::Query {
          query, vars_file, ..
        } => vars_file
          .iter()
          .cloned()
          .chain(
            query
              .iter()
              .filter_map(|value| value.strip_prefix('@').map(PathBuf::from)),
          )
          .collect(),
        Commands::Secrets {
          config, baseline, ..
        } => config.iter().chain(baseline.iter()).cloned().collect(),
        Commands::Search { rule, .. } => rule.iter().cloned().collect(),
        Commands::Edit(args) => args
          .files
          .iter()
          .chain(args.script.iter())
          .cloned()
          .collect(),
        _ => Vec::new(),
      };
      inputs.extend(cli.workspace.iter().cloned());
      crate::program_output::protect(inputs).map_err(|error| crate::error::anyhow!(error))?;
    }
    run_invocation(cli)
  })();
  if let Err(error) = result {
    if program {
      let message = error.to_string();
      let code = message
        .split(':')
        .next()
        .filter(|code| !code.is_empty() && code.chars().all(|c| c.is_ascii_uppercase() || c == '_'))
        .unwrap_or("COMMAND_FAILED");
      crate::program_output::error(code, message.clone());
      process::exit(2);
    }
    return Err(error);
  }
  Ok(())
}

fn run_invocation(cli: Cli) -> Result<()> {
  let exit_zero = cli.exit_zero_on_no_match;
  let no_cache = cli.no_cache;
  let common = cli.common.clone();
  let format = if crate::program_output::enabled() {
    OutputFormat::Json
  } else {
    common.format.unwrap_or(OutputFormat::Auto)
  };
  let walk_opts = build_walk_opts(&common);
  // §4.7 — load the workspace config once at dispatch. None means
  // the legacy single-root path; Some routes through fan-out.
  let workspace_ctx = match &cli.workspace {
    Some(p) => Some(crate::workspace::WorkspaceCtx::load(p)?),
    None => None,
  };
  // §1.5 -A / -B / -C context lines for line-oriented sensors.
  // -C is shorthand for "before == after == N".
  let context_lines = {
    let c = common.context.unwrap_or(0);
    ContextLines {
      before: common.before.unwrap_or(c),
      after: common.after.unwrap_or(c),
    }
  };

  let (verb, status, warnings) = match cli.command {
    Commands::Discover {
      path,
      tree,
      depth,
      top_level_only,
    } => {
      let res = sensors::discover(&path, &walk_opts, depth, top_level_only)?;
      render_by_format(&res, format, "discover", |r| {
        if tree {
          render_discover_tree(r);
        } else {
          render_discover_markdown(r);
        }
      });
      ("discover", res.exit_status, res.warnings)
    }
    Commands::Search {
      mut query,
      mut path,
      path_flag,
      outline,
      body: _body,
      pattern,
      strictness,
      name,
      kind,
      raw_kind,
      has,
      inside,
      precedes,
      follows,
      not,
      all,
      any,
      rule,
      has_decoration,
      has_child,
      has_attr,
    } => {
      let mut warnings_pre: Vec<ResultDiagnostic> = Vec::new();

      if let Some(path_from_flag) = path_flag {
        path = path_from_flag;
      }

      if pattern.is_some() && query.is_some() && path.as_os_str() == "." {
        if let Some(q) = query
          .as_ref()
          .filter(|q| PathBuf::from(q).exists())
          .cloned()
        {
          path = PathBuf::from(&q);
          query = None;
        }
      }

      // Phase 6A #6: `search --name X PATH` precedence. clap puts
      // a single positional into `query` (the first positional
      // slot), leaving `path` defaulted to ".". Detect that
      // shape and swap so the user's likely intent (filter by
      // name within PATH) is honored. Warn so explicit form
      // (`--name X -- PATH`) becomes discoverable.
      if name.is_some() && query.is_some() && path.as_os_str() == "." {
        if let Some(q) = query.take() {
          path = PathBuf::from(&q);
          if path.as_os_str() != "." {
            let hint = search_name_positional_as_path_hint(&path);
            warnings_pre.push(
              ResultDiagnostic::new(
                "SEARCH_NAME_POSITIONAL_AS_PATH",
                format!(
                  "--name set; reinterpreted positional `{}` as PATH (would otherwise be ignored as query)",
                  q
                ),
              )
              .with_hint(hint),
            );
          }
        }
      }

      if outline
        && pattern.is_none()
        && name.is_none()
        && query.is_some()
        && path.as_os_str() == "."
      {
        if let Some(q) = query
          .as_ref()
          .filter(|q| PathBuf::from(q).exists())
          .cloned()
        {
          path = PathBuf::from(&q);
          query = None;
        }
      }

      // §1.1 — input mode resolution.
      //   1. -p <PAT> + -l <LANG> → pattern mode.
      //   2. --name <GLOB>        → name mode.
      //   3. positional + -l + has $ → pattern mode (deprecated).
      //   4. positional alone     → name mode (deprecated).
      let (pattern_mode, name_filter): (Option<PatternMode>, Option<String>) = match (
        pattern.as_ref(),
        common.lang.as_ref(),
        name.as_ref(),
        query.as_ref(),
      ) {
        (Some(pat), Some(lstr), _, _) => {
          let l = match parse_lang(lstr) {
            Some(l) => l,
            None => {
              return Err(crate::error::anyhow!(
                "unknown language `{}` for `-l`",
                lstr
              ));
            }
          };
          (
            Some(PatternMode {
              source: pat.clone(),
              language: l,
              strictness: strictness.map(|s| s.into()),
            }),
            None,
          )
        }
        (Some(pat), None, _, _) => match infer_pattern_lang_from_single_file(&path) {
          Some(l) => (
            Some(PatternMode {
              source: pat.clone(),
              language: l,
              strictness: strictness.map(|s| s.into()),
            }),
            None,
          ),
          None => {
            if crate::program_output::enabled() {
              return Err(crate::error::anyhow!("PATTERN_LANGUAGE_REQUIRED: pattern search requires --lang unless PATH is a recognized single file"));
            }
            // QA TC-082: clap convention — argument errors exit 2.
            eprintln!(
              "Error: `-p / --pattern` requires `-l / --lang` unless PATH is a single file with a known extension"
            );
            std::process::exit(2);
          }
        },
        (_, _, Some(g), _) => (None, Some(g.clone())),
        (_, lng, _, Some(q)) => {
          if q.contains('$') {
            match lng
              .and_then(|l| parse_lang(l))
              .or_else(|| infer_pattern_lang_from_single_file(&path))
            {
              Some(l) => {
                warnings_pre.push(
                  ResultDiagnostic::new(
                    "DEPRECATED_POSITIONAL_QUERY",
                    "positional pattern is deprecated; use `-p <PAT>` instead",
                  )
                  .with_hint("lmprobe search -p '<pattern>' -l <lang>"),
                );
                (
                  Some(PatternMode {
                    source: q.clone(),
                    language: l,
                    strictness: strictness.map(|s| s.into()),
                  }),
                  None,
                )
              }
              None => {
                if crate::program_output::enabled() {
                  return Err(crate::error::anyhow!(
                    "PATTERN_LANGUAGE_REQUIRED: positional AST pattern requires --lang"
                  ));
                }
                // QA TC-082 family: same exit-2 convention.
                eprintln!("Error: positional pattern with metavariable `$` requires `-l <lang>`");
                std::process::exit(2);
              }
            }
          } else {
            warnings_pre.push(
                            ResultDiagnostic::new(
                                "DEPRECATED_POSITIONAL_QUERY",
                                "positional name query is deprecated; use `--name <SUBSTR>` instead (case-insensitive substring match)",
                            )
                            .with_hint("lmprobe search --name '<substr>' [PATH]"),
                        );
            (None, Some(q.clone()))
          }
        }
        (None, _, None, None) => (None, None),
      };

      // §1.2 — combine the relational flags into a single
      // composite YAML rule (or load `-r FILE` directly).
      let composite_rule = compose_relational_yaml(
        has.as_deref(),
        inside.as_deref(),
        precedes.as_deref(),
        follows.as_deref(),
        not.as_deref(),
        &all,
        &any,
        rule.as_deref(),
      )?;

      let constraints = SearchConstraints {
        query: name_filter,
        has_decoration,
        has_child,
        has_attr,
        kind,
        raw_kind,
        has_rule: composite_rule,
        pattern: pattern_mode,
        ..Default::default()
      };
      let mut res = if let Some(ref ws) = workspace_ctx {
        let constraints_ref = constraints;
        let walk_opts_ref = walk_opts.clone();
        let (hits, ws_warnings) = fan_out_symbol_hits(ws, |root_path| {
          let mut r = sensors::search(root_path, outline, constraints_ref.clone(), &walk_opts_ref)?;
          Ok((std::mem::take(&mut r.hits), std::mem::take(&mut r.warnings)))
        })?;
        let mut env = crate::schema::SearchResult::new(hits);
        for w in ws_warnings {
          env.push_warning(w);
        }
        env
      } else {
        sensors::search(&path, outline, constraints, &walk_opts)?
      };
      for w in warnings_pre {
        res.push_warning(w);
      }
      // §1.5 --limit truncation.
      if let Some(lim) = common.limit {
        if res.hits.len() > lim {
          res.hits.truncate(lim);
        }
      }
      render_by_format(&res, format, "search", |r| {
        render_symbol_hits_markdown(&r.hits)
      });
      ("search", res.exit_status, res.warnings)
    }
    Commands::Def {
      name,
      path,
      regex,
      class_name,
      this_file,
      body,
      around,
      scope,
    } => {
      if this_file && !path.is_file() {
        return Err(crate::error::anyhow!(
          "--this-file requires PATH to be a file, got `{}`",
          path.display()
        ));
      }
      let mut res = sensors::def(
        name,
        &path,
        regex,
        class_name,
        scope_code_only(scope),
        this_file,
        body,
        &walk_opts,
      )?;
      attach_symbol_source_context(&mut res.hits, around);
      if let Some(lim) = common.limit {
        if res.hits.len() > lim {
          res.hits.truncate(lim);
        }
      }
      render_by_format(&res, format, "def", |r| {
        render_symbol_hits_markdown(&r.hits)
      });
      ("def", res.exit_status, res.warnings)
    }
    Commands::Ref {
      name,
      path,
      scope,
      include_comments,
      include_docs,
      role,
    } => {
      let mut res = sensors::refs(
        &name,
        &path,
        scope_code_only(scope),
        include_comments,
        include_docs,
        &walk_opts,
      )?;
      if let Some(lim) = common.limit {
        if res.hits.len() > lim {
          res.hits.truncate(lim);
        }
      }
      if role {
        sensors::attach_grep_roles_for_literal(&mut res.hits, &name, &walk_opts);
      }
      render_by_format(&res, format, "ref", |r| {
        render_ref_hits_markdown(r, context_lines)
      });
      ("ref", res.exit_status, res.warnings)
    }
    Commands::ListKinds { lang } => {
      let effective_lang = match lang.or(common.lang.clone()) {
        Some(l) => l,
        None => {
          return Err(crate::error::anyhow!(
            "`list-kinds` requires a language as positional arg or `-l/--lang`"
          ));
        }
      };
      let l = match parse_lang(&effective_lang) {
        Some(l) => l,
        None => {
          return Err(crate::error::anyhow!(
            "unknown language `{}`",
            effective_lang
          ));
        }
      };
      let tx = taxonomy_for(l);
      let rows: Vec<crate::schema::PortableKindRow> = PortableKind::ALL
        .iter()
        .map(|p| crate::schema::PortableKindRow {
          portable: p.name().to_string(),
          kinds: tx.kind_for(*p).iter().map(|s| s.to_string()).collect(),
        })
        .collect();
      let res = crate::schema::ListKindsResult::new(rows);
      render_by_format(&res, format, "list-kinds", |r| {
        stdout_line(&format!("# {} portable kind taxonomy", effective_lang));
        for row in &r.rows {
          if row.kinds.is_empty() {
            stdout_line(&format!("{:<14} —", row.portable));
          } else {
            stdout_line(&format!("{:<14} {}", row.portable, row.kinds.join(", ")));
          }
        }
      });
      ("list-kinds", res.exit_status, res.warnings)
    }
    Commands::Callers {
      name,
      tests_only,
      path,
    } => {
      let mut res = sensors::callers(&name, &path, &walk_opts)?;
      if tests_only {
        let had_hits = !res.hits.is_empty();
        res
          .hits
          .retain(|hit| crate::impact::is_test_path(&hit.file_path));
        if had_hits {
          res.exit_status = ExitStatus::from_count(res.hits.len());
        }
      }
      if let Some(lim) = common.limit {
        if res.hits.len() > lim {
          res.hits.truncate(lim);
        }
      }
      let origin = name.clone();
      render_by_format_with_graph(
        &res,
        format,
        "callers",
        |r| render_symbol_hits_markdown(&r.hits),
        |r| crate::output::render_symbol_graph_mermaid(&origin, &r.hits, "called by", "BT"),
        |r| crate::output::render_symbol_graph_dot(&origin, &r.hits, "called by"),
      );
      ("callers", res.exit_status, res.warnings)
    }
    Commands::Imports { name, path } => {
      let mut res = sensors::imports(&name, &path, &walk_opts)?;
      if let Some(lim) = common.limit {
        if res.hits.len() > lim {
          res.hits.truncate(lim);
        }
      }
      render_by_format(&res, format, "imports", |r| {
        render_symbol_hits_markdown(&r.hits)
      });
      ("imports", res.exit_status, res.warnings)
    }
    Commands::Decorations { name, path } => {
      let mut res = sensors::decorations(&name, &path, &walk_opts)?;
      if let Some(lim) = common.limit {
        if res.hits.len() > lim {
          res.hits.truncate(lim);
        }
      }
      render_by_format(&res, format, "decorations", |r| {
        render_symbol_hits_markdown(&r.hits)
      });
      ("decorations", res.exit_status, res.warnings)
    }
    Commands::Callees { name, path } => {
      let mut res = crate::graph_sensors::callees(&name, &path, &walk_opts)?;
      if let Some(lim) = common.limit {
        if res.hits.len() > lim {
          res.hits.truncate(lim);
        }
      }
      let origin = name.clone();
      render_by_format_with_graph(
        &res,
        format,
        "callees",
        |r| render_symbol_hits_markdown(&r.hits),
        |r| crate::output::render_symbol_graph_mermaid(&origin, &r.hits, "calls", "TD"),
        |r| crate::output::render_symbol_graph_dot(&origin, &r.hits, "calls"),
      );
      ("callees", res.exit_status, res.warnings)
    }
    Commands::Supertypes { name, path } => {
      let mut res = crate::graph_sensors::supertypes(&name, &path, &walk_opts)?;
      if let Some(lim) = common.limit {
        if res.hits.len() > lim {
          res.hits.truncate(lim);
        }
      }
      let origin = name.clone();
      render_by_format_with_graph(
        &res,
        format,
        "supertypes",
        |r| render_symbol_hits_markdown(&r.hits),
        |r| crate::output::render_symbol_graph_mermaid(&origin, &r.hits, "extends", "BT"),
        |r| crate::output::render_symbol_graph_dot(&origin, &r.hits, "extends"),
      );
      ("supertypes", res.exit_status, res.warnings)
    }
    Commands::Subtypes { name, path } => {
      let mut res = crate::graph_sensors::subtypes(&name, &path, &walk_opts)?;
      if let Some(lim) = common.limit {
        if res.hits.len() > lim {
          res.hits.truncate(lim);
        }
      }
      let origin = name.clone();
      render_by_format_with_graph(
        &res,
        format,
        "subtypes",
        |r| render_symbol_hits_markdown(&r.hits),
        |r| crate::output::render_symbol_graph_mermaid(&origin, &r.hits, "extended by", "TD"),
        |r| crate::output::render_symbol_graph_dot(&origin, &r.hits, "extended by"),
      );
      ("subtypes", res.exit_status, res.warnings)
    }
    Commands::Overrides { name, path } => {
      let mut res = crate::graph_sensors::overrides(&name, &path, &walk_opts)?;
      if let Some(lim) = common.limit {
        if res.hits.len() > lim {
          res.hits.truncate(lim);
        }
      }
      render_by_format(&res, format, "overrides", |r| {
        render_symbol_hits_markdown(&r.hits)
      });
      ("overrides", res.exit_status, res.warnings)
    }
    Commands::Exports { name, path } => {
      let mut res = crate::graph_sensors::exports(&name, &path, &walk_opts)?;
      if let Some(lim) = common.limit {
        if res.hits.len() > lim {
          res.hits.truncate(lim);
        }
      }
      render_by_format(&res, format, "exports", |r| {
        render_symbol_hits_markdown(&r.hits)
      });
      ("exports", res.exit_status, res.warnings)
    }
    Commands::Impact {
      symbol,
      path,
      direction,
      depth,
      tests_only,
      exclude_tests,
    } => {
      let res = crate::impact::impact(
        &symbol,
        &path,
        direction.into(),
        depth,
        impact_file_filter(tests_only, exclude_tests),
        &walk_opts,
      )?;
      render_by_format_with_graph(
        &res,
        format,
        "impact",
        render_impact_markdown,
        render_impact_mermaid,
        render_impact_dot,
      );
      ("impact", res.exit_status, res.warnings)
    }
    Commands::PrTriage {
      since,
      compact,
      url,
    } => {
      let res = crate::pr_triage::pr_triage(
        &url,
        crate::pr_triage::PrTriageOptions {
          since: since.into(),
          compact,
        },
      )?;
      render_by_format(&res, format, "pr_triage", render_pr_triage_markdown);
      ("pr_triage", res.exit_status, res.warnings)
    }
    Commands::Context {
      def: _,
      usage,
      symbol,
      path,
      depth,
      import_aware,
      around,
      parents,
      body,
      recent_history_limit,
    } => {
      let mode = if usage {
        crate::context_bundle::ContextAnchorMode::Usage
      } else {
        crate::context_bundle::ContextAnchorMode::Def
      };
      let res = crate::context_bundle::context_with_options(
        &symbol,
        &path,
        depth,
        &walk_opts,
        recent_history_limit,
        import_aware,
        around,
        mode,
        parents,
        body,
      )?;
      render_by_format(&res, format, "context", |r| {
        render_context_markdown(r);
      });
      ("context", res.exit_status, res.warnings)
    }
    Commands::Evolution {
      name,
      path,
      force,
      regex,
    } => {
      let res = sensors::evolution_with_regex(&name, &path, force, no_cache, regex, &walk_opts)?;
      render_by_format(&res, format, "evolution", render_evolution_markdown);
      ("evolution", res.exit_status, res.warnings)
    }
    Commands::Trace {
      query,
      path,
      exec,
      template,
      watch,
      json_trace,
      shape,
      len,
      id,
      repr_limit,
      force,
      worktree,
      keep_patch,
    } => {
      refuse_mutation_under_workspace(workspace_ctx.as_ref(), "trace")?;
      // §4.2 — translate the clap surfaces. None = legacy
      // in-place; Some("") = auto temp-dir; Some(p) = explicit.
      let worktree_opt = worktree.map(|s| {
        if s.is_empty() {
          None
        } else {
          Some(PathBuf::from(s))
        }
      });
      let keep_patch_opt = keep_patch.map(|s| {
        if s.is_empty() {
          None
        } else {
          Some(PathBuf::from(s))
        }
      });
      let safety = sensors::TraceSafety {
        worktree: worktree_opt,
        keep_patch: keep_patch_opt,
      };
      let trace_watch = sensors::TraceWatchOptions {
        watch,
        json_trace,
        include_shape: shape,
        include_len: len,
        include_id: id,
        repr_limit,
      };
      crate::program_output::protect_trace_root(&path)
        .map_err(|error| crate::error::anyhow!(error))?;
      let res = sensors::trace_with_safety_and_watch(
        &query,
        &path,
        &exec,
        template,
        force,
        &safety,
        &walk_opts,
        &trace_watch,
      )?;
      render_by_format(&res, format, "trace", render_trace_markdown);
      ("trace", res.exit_status, res.warnings)
    }
    Commands::AstDiff { file, from, to } => {
      let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
      let res = crate::ast_diff_sensor::ast_diff(&file, &from, &to, &root)?;
      render_by_format(&res, format, "ast_diff", |r| {
        stdout_line(&format!(
          "# ast-diff {} ({} → {}): {} symbols",
          file.display(),
          from,
          to,
          r.nodes.len()
        ));
        for n in &r.nodes {
          if n.change_type == "UNCHANGED" {
            continue;
          }
          stdout_line(&format!(
            "- [{}] {}  (L{} → L{})",
            n.change_type, n.name, n.from_line, n.to_line
          ));
          if n.change_type == "MODIFIED" {
            stdout_line(&format!(
              "    -- {}",
              n.from_signature.lines().next().unwrap_or("")
            ));
            stdout_line(&format!(
              "    ++ {}",
              n.to_signature.lines().next().unwrap_or("")
            ));
          }
        }
      });
      ("ast_diff", res.exit_status, res.warnings)
    }
    Commands::DiffContext {
      from,
      to,
      around,
      parents,
      body,
      path,
    } => {
      let res =
        crate::diff_context::diff_context(&from, &to, &path, around, parents, body, &walk_opts)?;
      render_by_format(&res, format, "diff_context", |r| {
        stdout_line(&format!(
          "# diff-context {}..{}: {} changed-line anchors",
          from,
          to,
          r.anchors.len()
        ));
        for anchor in &r.anchors {
          stdout_line(&format!(
            "{}:{}: {}",
            anchor.file_path, anchor.line_number, anchor.line_content
          ));
          if !anchor.source_context.is_empty() {
            for line in &anchor.source_context {
              let sep = if line.line_number == anchor.line_number {
                ":"
              } else {
                "-"
              };
              stdout_line(&format!(
                "{}{}{}: {}",
                anchor.file_path, sep, line.line_number, line.line_content
              ));
            }
          }
          if !anchor.parents.is_empty() {
            let chain = anchor
              .parents
              .iter()
              .map(|parent| {
                parent
                  .name
                  .as_deref()
                  .map(|name| format!("{} {}", parent.kind, name))
                  .unwrap_or_else(|| parent.kind.clone())
              })
              .collect::<Vec<_>>()
              .join(" > ");
            stdout_line(&format!("  parents: {}", chain));
          }
          if let Some(body) = &anchor.body {
            let name = body.name.as_deref().unwrap_or("<anonymous>");
            stdout_line(&format!(
              "  body: {} {} L{}-L{}",
              body.kind, name, body.line_start, body.line_end
            ));
          }
        }
      });
      ("diff_context", res.exit_status, res.warnings)
    }
    Commands::ResourceSites {
      path,
      acquire,
      release,
      around,
      parents,
      body,
    } => {
      let res = crate::resource_sites::resource_sites(
        &path, &acquire, &release, around, parents, body, &walk_opts,
      )?;
      render_by_format(&res, format, "resource_sites", |r| {
        stdout_line(&format!(
          "# resource-sites: {} evidence groups",
          r.sites.len()
        ));
        for site in &r.sites {
          let enclosing = site
            .enclosing
            .as_ref()
            .and_then(|parent| parent.name.as_deref())
            .unwrap_or("<module>");
          stdout_line(&format!(
            "{}: {} acquire anchors, {} release anchors ({})",
            site.file_path,
            site.acquire_calls.len(),
            site.release_calls.len(),
            enclosing
          ));
          if site.acquire_timing.is_some()
            || site.release_lifecycle.is_some()
            || site.optional_acquire.is_some()
          {
            let mut classifications = Vec::new();
            if let Some(timing) = site.acquire_timing.as_deref() {
              classifications.push(format!("acquire_timing={}", timing));
            }
            if let Some(lifecycle) = site.release_lifecycle.as_deref() {
              classifications.push(format!("release_lifecycle={}", lifecycle));
            }
            if let Some(optional) = site.optional_acquire {
              classifications.push(format!("optional_acquire={}", optional));
            }
            stdout_line(&format!("  classification: {}", classifications.join(", ")));
          }
          for call in &site.acquire_calls {
            stdout_line(&format!(
              "  acquire {}:{}: {}",
              call.line_number, call.callee, call.line_content
            ));
          }
          for call in &site.release_calls {
            stdout_line(&format!(
              "  release {}:{}: {}",
              call.line_number, call.callee, call.line_content
            ));
          }
          for call in &site.defer_calls {
            stdout_line(&format!(
              "  defer {}:{}: {}",
              call.line_number, call.callee, call.line_content
            ));
          }
          for call in &site.error_branches {
            stdout_line(&format!(
              "  error-branch {}:{}: {}",
              call.line_number, call.callee, call.line_content
            ));
          }
        }
      });
      ("resource_sites", res.exit_status, res.warnings)
    }
    Commands::AsyncLeaks {
      path,
      call,
      around,
      parents,
      body,
    } => {
      let res = crate::async_leaks::async_leaks(&path, &call, around, parents, body, &walk_opts)?;
      render_by_format(&res, format, "async_leaks", |r| {
        stdout_line(&format!("# async-leaks: {} call anchors", r.sites.len()));
        for site in &r.sites {
          stdout_line(&format!(
            "{}:{}: {} awaited={} returned={}",
            site.file_path, site.line_number, site.callee, site.awaited, site.returned
          ));
          if !site.parents.is_empty() {
            let chain = site
              .parents
              .iter()
              .map(|parent| {
                parent
                  .name
                  .as_deref()
                  .map(|name| format!("{} {}", parent.kind, name))
                  .unwrap_or_else(|| parent.kind.clone())
              })
              .collect::<Vec<_>>()
              .join(" > ");
            stdout_line(&format!("  parents: {}", chain));
          }
          if let Some(body) = &site.body {
            let name = body.name.as_deref().unwrap_or("<anonymous>");
            stdout_line(&format!(
              "  body: {} {} L{}-L{}",
              body.kind, name, body.line_start, body.line_end
            ));
          }
          if !site.source_context.is_empty() {
            for line in &site.source_context {
              stdout_line(&format!("  {:>6}: {}", line.line_number, line.line_content));
            }
          }
        }
      });
      ("async_leaks", res.exit_status, res.warnings)
    }
    Commands::CryptoSites {
      path,
      algorithms,
      fips_check,
      around,
      parents,
      body,
    } => {
      let res = crate::crypto_sites::crypto_sites(
        &path,
        &algorithms,
        fips_check,
        around,
        parents,
        body,
        &walk_opts,
      )?;
      render_by_format(&res, format, "crypto_sites", |r| {
        stdout_line(&format!(
          "# crypto-sites: {} algorithm anchors",
          r.sites.len()
        ));
        for site in &r.sites {
          stdout_line(&format!(
            "{}:{}: {} {}",
            site.file_path, site.line_number, site.algorithm, site.match_text
          ));
          if !site.import_paths.is_empty() {
            stdout_line(&format!("  imports: {}", site.import_paths.join(", ")));
          }
          for guard in &site.nearby_guards {
            stdout_line(&format!(
              "  guard {}:{}: {}",
              guard.line_number, guard.match_text, guard.line_content
            ));
          }
          if !site.parents.is_empty() {
            let chain = site
              .parents
              .iter()
              .map(|parent| {
                parent
                  .name
                  .as_deref()
                  .map(|name| format!("{} {}", parent.kind, name))
                  .unwrap_or_else(|| parent.kind.clone())
              })
              .collect::<Vec<_>>()
              .join(" > ");
            stdout_line(&format!("  parents: {}", chain));
          }
          if let Some(body) = &site.body {
            let name = body.name.as_deref().unwrap_or("<anonymous>");
            stdout_line(&format!(
              "  body: {} {} L{}-L{}",
              body.kind, name, body.line_start, body.line_end
            ));
          }
          if !site.source_context.is_empty() {
            for line in &site.source_context {
              stdout_line(&format!("  {:>6}: {}", line.line_number, line.line_content));
            }
          }
        }
      });
      ("crypto_sites", res.exit_status, res.warnings)
    }
    Commands::ConfigKeys {
      paths,
      key,
      grep_code,
      around,
      parents,
      role,
    } => {
      let res = crate::config_keys::config_keys(
        &paths,
        key.as_deref(),
        grep_code,
        around,
        parents,
        role,
        &walk_opts,
      )?;
      render_by_format(&res, format, "config_keys", |r| {
        stdout_line(&format!("# config-keys: {} keys", r.keys.len()));
        for key in &r.keys {
          let summary = key
            .value_summary
            .as_deref()
            .map(|value| format!(" = {}", value))
            .unwrap_or_default();
          stdout_line(&format!(
            "{}:{}: {} ({}){}",
            key.file_path, key.line_number, key.key_path, key.value_kind, summary
          ));
          for anchor in &key.code_anchors {
            stdout_line(&format!(
              "  code {}:{}: {}",
              anchor.file_path, anchor.line_number, anchor.line_content
            ));
            if let Some(role) = &anchor.role {
              stdout_line(&format!("    role: {}", role));
            }
            if !anchor.parents.is_empty() {
              let chain = anchor
                .parents
                .iter()
                .map(|parent| {
                  parent
                    .name
                    .as_deref()
                    .map(|name| format!("{} {}", parent.kind, name))
                    .unwrap_or_else(|| parent.kind.clone())
                })
                .collect::<Vec<_>>()
                .join(" > ");
              stdout_line(&format!("    parents: {}", chain));
            }
            if !anchor.source_context.is_empty() {
              for line in &anchor.source_context {
                stdout_line(&format!(
                  "    {:>6}: {}",
                  line.line_number, line.line_content
                ));
              }
            }
          }
        }
      });
      ("config_keys", res.exit_status, res.warnings)
    }
    Commands::Secrets {
      path,
      config,
      max_target_megabytes,
      follow_symlinks,
      gitleaks_compat,
      ast_fp_cut,
      sink_evidence,
      source,
      baseline,
    } => {
      let scan_opts = crate::secrets::SecretsScanOptions {
        config_path: config,
        max_target_megabytes,
        follow_symlinks,
        gitleaks_compat,
        ast_fp_cut: ast_fp_cut.into(),
        sink_evidence: sink_evidence.into(),
        source_mode: source.into(),
        baseline_path: baseline,
      };
      let res = crate::secrets::scan_filesystem(&path, &walk_opts, &scan_opts)?;
      let status = match res.exit_status {
        lmprobe_secrets::SecretsExitStatus::Matched => ExitStatus::Matched,
        lmprobe_secrets::SecretsExitStatus::NoMatch => ExitStatus::NoMatch,
        lmprobe_secrets::SecretsExitStatus::Error => ExitStatus::Error,
      };
      let warnings: Vec<_> = res
        .warnings
        .iter()
        .map(|warning| {
          let diagnostic = ResultDiagnostic::new(warning.code.clone(), warning.message.clone());
          if let Some(hint) = &warning.hint {
            diagnostic.with_hint(hint.clone())
          } else {
            diagnostic
          }
        })
        .collect();
      render_by_format(&res, format, "secrets", |r| {
        stdout_line(&format!(
          "# secrets: {} findings, {} suppressed, {} semantic, {} sink-evidence",
          r.findings.len(),
          r.suppressed.len(),
          r.semantic_findings.len(),
          r.sink_evidence.len()
        ));
        for finding in &r.findings {
          stdout_line(&format!(
            "{}:{}:{}: {} {}",
            finding.file_path,
            finding.start_line,
            finding.start_column,
            finding.rule_id,
            finding.redacted_secret
          ));
        }
      });
      if gitleaks_compat {
        let partial = has_primary_path_partial_coverage(&warnings);
        let code = match status {
          ExitStatus::Matched => 1,
          ExitStatus::NoMatch => 0,
          ExitStatus::Error => 2,
        };
        let code = if crate::program_output::failed() || (partial && code == 0) {
          2
        } else {
          code
        };
        process::exit(code);
      }
      ("secrets", status, warnings)
    }
    Commands::SecretsRecipes { cmd } => dispatch_secrets_recipes(cmd, format, &walk_opts)?,
    Commands::SecretsSources { cmd } => dispatch_secrets_sources(cmd, format, &walk_opts)?,
    Commands::GuardSites {
      path,
      sensitive,
      guard,
      around,
      parents,
      body,
    } => {
      let res = crate::guard_sites::guard_sites(
        &path, &sensitive, &guard, around, parents, body, &walk_opts,
      )?;
      render_by_format(&res, format, "guard_sites", |r| {
        stdout_line(&format!(
          "# guard-sites: {} evidence candidates",
          r.candidates.len()
        ));
        for candidate in &r.candidates {
          stdout_line(&format!(
            "{}:{}: sensitive {}",
            candidate.file_path, candidate.sensitive.line_number, candidate.sensitive.match_text
          ));
          for guard in &candidate.nearby_guards {
            stdout_line(&format!(
              "  guard {}:{}: {}",
              guard.line_number, guard.match_text, guard.line_content
            ));
          }
          for decoration in &candidate.decorations {
            stdout_line(&format!(
              "  decoration {}:{}: {}",
              decoration.line_number, decoration.match_text, decoration.line_content
            ));
          }
          if !candidate.parents.is_empty() {
            let chain = candidate
              .parents
              .iter()
              .map(|parent| {
                parent
                  .name
                  .as_deref()
                  .map(|name| format!("{} {}", parent.kind, name))
                  .unwrap_or_else(|| parent.kind.clone())
              })
              .collect::<Vec<_>>()
              .join(" > ");
            stdout_line(&format!("  parents: {}", chain));
          }
          if let Some(body) = &candidate.body {
            let name = body.name.as_deref().unwrap_or("<anonymous>");
            stdout_line(&format!(
              "  body: {} {} L{}-L{}",
              body.kind, name, body.line_start, body.line_end
            ));
          }
          if !candidate.source_context.is_empty() {
            for line in &candidate.source_context {
              stdout_line(&format!("  {:>6}: {}", line.line_number, line.line_content));
            }
          }
        }
      });
      ("guard_sites", res.exit_status, res.warnings)
    }
    Commands::OpenapiFacts { path } => {
      let res = crate::api_facts::openapi_facts(&path, &walk_opts)?;
      render_by_format(&res, format, "openapi_facts", |r| {
        stdout_line(&format!(
          "# openapi-facts: {} operations",
          r.operations.len()
        ));
        for op in &r.operations {
          let operation_id = op
            .operation_id
            .as_deref()
            .map(|id| format!(" operationId={}", id))
            .unwrap_or_default();
          stdout_line(&format!(
            "{} {}{} ({})",
            op.method, op.path, operation_id, op.file_path
          ));
          if !op.required_params.is_empty() {
            stdout_line(&format!(
              "  required params: {}",
              op.required_params.join(", ")
            ));
          }
          if !op.response_schemas.is_empty() {
            stdout_line(&format!(
              "  response schemas: {}",
              op.response_schemas.join(", ")
            ));
          }
        }
      });
      ("openapi_facts", res.exit_status, res.warnings)
    }
    Commands::RouteAnchors {
      path,
      framework,
      around,
      parents,
      body,
    } => {
      let framework = framework.map(RouteFramework::as_str);
      let res =
        crate::api_facts::route_anchors(&path, framework, around, parents, body, &walk_opts)?;
      render_by_format(&res, format, "route_anchors", |r| {
        stdout_line(&format!("# route-anchors: {} routes", r.routes.len()));
        for route in &r.routes {
          stdout_line(&format!(
            "{}:{}: {} {}",
            route.file_path, route.line_number, route.method, route.path
          ));
          if let Some(handler) = &route.handler {
            stdout_line(&format!("  handler: {}", handler));
          }
          if !route.parents.is_empty() {
            let chain = route
              .parents
              .iter()
              .map(|parent| {
                parent
                  .name
                  .as_deref()
                  .map(|name| format!("{} {}", parent.kind, name))
                  .unwrap_or_else(|| parent.kind.clone())
              })
              .collect::<Vec<_>>()
              .join(" > ");
            stdout_line(&format!("  parents: {}", chain));
          }
          if let Some(body) = &route.body {
            let name = body.name.as_deref().unwrap_or("<anonymous>");
            stdout_line(&format!(
              "  body: {} {} L{}-L{}",
              body.kind, name, body.line_start, body.line_end
            ));
          }
          if !route.source_context.is_empty() {
            for line in &route.source_context {
              stdout_line(&format!("  {:>6}: {}", line.line_number, line.line_content));
            }
          }
        }
      });
      ("route_anchors", res.exit_status, res.warnings)
    }
    Commands::HttpClientCalls {
      path,
      around,
      parents,
      body,
    } => {
      let res = crate::api_facts::http_client_calls(&path, around, parents, body, &walk_opts)?;
      render_by_format(&res, format, "http_client_calls", |r| {
        stdout_line(&format!("# http-client-calls: {} calls", r.calls.len()));
        for call in &r.calls {
          let method = call.method.as_deref().unwrap_or("<method>");
          stdout_line(&format!(
            "{}:{}: {} {}",
            call.file_path, call.line_number, method, call.url
          ));
          if let Some(client) = &call.client {
            stdout_line(&format!("  client: {}", client));
          }
          if !call.parents.is_empty() {
            let chain = call
              .parents
              .iter()
              .map(|parent| {
                parent
                  .name
                  .as_deref()
                  .map(|name| format!("{} {}", parent.kind, name))
                  .unwrap_or_else(|| parent.kind.clone())
              })
              .collect::<Vec<_>>()
              .join(" > ");
            stdout_line(&format!("  parents: {}", chain));
          }
          if let Some(body) = &call.body {
            let name = body.name.as_deref().unwrap_or("<anonymous>");
            stdout_line(&format!(
              "  body: {} {} L{}-L{}",
              body.kind, name, body.line_start, body.line_end
            ));
          }
          if !call.source_context.is_empty() {
            for line in &call.source_context {
              stdout_line(&format!("  {:>6}: {}", line.line_number, line.line_content));
            }
          }
        }
      });
      ("http_client_calls", res.exit_status, res.warnings)
    }
    Commands::Aws(args) => {
      if workspace_ctx.is_some() {
        return Err(crate::error::anyhow!(
          "AWS_WORKSPACE_UNSUPPORTED: AWS queries do not use filesystem workspaces"
        ));
      }
      let res = crate::aws_command::run(&args)?;
      let status: ExitStatus = serde_json::from_value(res["exit_status"].clone())?;
      let warnings: Vec<crate::schema::Diagnostic> =
        serde_json::from_value(res["warnings"].clone())?;
      render_by_format(&res, format, "aws", |r| {
        stdout_line(&serde_json::to_string_pretty(r).unwrap())
      });
      ("aws", status, warnings)
    }
    Commands::Git(args) => {
      if workspace_ctx.is_some() {
        return Err(crate::error::anyhow!(
          "GIT_WORKSPACE_UNSUPPORTED: use --repo for a single repository"
        ));
      }
      let res = crate::git_command::run(&args)?;
      let status: ExitStatus = serde_json::from_value(res["exit_status"].clone())?;
      let warnings: Vec<crate::schema::Diagnostic> =
        serde_json::from_value(res["warnings"].clone())?;
      render_by_format(&res, format, "git", |r| {
        stdout_line(&serde_json::to_string_pretty(r).unwrap());
      });
      ("git", status, warnings)
    }
    Commands::Edit(args) => {
      refuse_mutation_under_workspace(workspace_ctx.as_ref(), "edit")?;
      let res = crate::edit_command::run(&args, common.ignore_case, common.max_filesize)?;
      render_by_format(&res, format, "edit", |r| {
        stdout_line(&format!(
          "# edit: {} changed files ({})",
          r.files.len(),
          r.mode
        ));
        for file in &r.files {
          stdout_line(&format!(
            "--- {}\n{}\n+++ {}\n{}",
            file.path.display(),
            file.before,
            file.path.display(),
            file.after
          ));
        }
        if let Some(path) = &r.audit_path {
          stdout_line(&format!("# recovery: {}", path.display()));
        }
      });
      ("edit", res.exit_status, res.warnings)
    }
    Commands::Fix {
      pattern,
      replacement,
      path,
      lang,
      apply,
      force,
    } => {
      if apply {
        refuse_mutation_under_workspace(workspace_ctx.as_ref(), "fix")?;
      }
      let lang_hint = lang.as_deref().and_then(parse_lang);
      if crate::program_output::target().is_some() {
        let preview =
          crate::fix_sensor::fix_dry_run(&pattern, &replacement, &path, lang_hint, &walk_opts)?;
        crate::program_output::protect(
          preview
            .diffs
            .iter()
            .map(|diff| PathBuf::from(&diff.file_path)),
        )
        .map_err(|error| crate::error::anyhow!(error))?;
      }
      let res = if apply {
        crate::fix_sensor::fix_apply(&pattern, &replacement, &path, lang_hint, force, &walk_opts)?
      } else {
        crate::fix_sensor::fix_dry_run(&pattern, &replacement, &path, lang_hint, &walk_opts)?
      };
      render_by_format(&res, format, "fix", |r| {
        stdout_line(&format!(
          "# {} matches across {} files (mode: {})",
          r.matches_count, r.files_count, r.mode
        ));
        if r.mode == "applied" {
          if let Some(p) = &r.patch_path {
            stdout_line(&format!("# patch: {}", p));
          }
          if let Some(p) = &r.audit_log_path {
            stdout_line(&format!("# audit: {}", p));
          }
          stdout_line(&format!("# applied {} files", r.applied_files_count));
        } else {
          for d in &r.diffs {
            stdout_line(&crate::fix_sensor::render_unified_diff(d));
          }
        }
      });
      ("fix", res.exit_status, res.warnings)
    }
    Commands::Grep {
      pattern,
      path,
      paths,
      line_number: _,
      parents,
      body,
      inside_kind,
      role,
      quick_projects,
    } => {
      // §1.5 -i/--ignore-case: prefix the regex with `(?i)`.
      let pat = if common.ignore_case {
        format!("(?i){}", pattern)
      } else {
        pattern
      };
      let missing_path_diags = explicit_missing_read_path_diagnostics(path.as_ref(), &paths);
      if !missing_path_diags.is_empty() {
        let mut res = crate::schema::GrepResultEnvelope::new(Vec::new());
        res.exit_status = ExitStatus::Error;
        for d in missing_path_diags {
          res.push_warning(d);
        }
        render_by_format(&res, format, "grep", |r| {
          render_grep_hits_markdown(r, context_lines)
        });
        ("grep", res.exit_status, res.warnings)
      } else {
        let roots = collect_read_paths(path, paths);
        let inside_kind_filters = match inside_kind {
          Some(inner) => Some(parse_inside_kind_filter(&inner)?),
          None => None,
        };
        let has_inside_kind_filter = inside_kind_filters.is_some();
        let mut local_walk_opts = walk_opts.clone();
        local_walk_opts.quick_project_roots = quick_projects;
        if inside_kind_filters.is_some() {
          local_walk_opts.result_limit = None;
        }
        let mut res = sensors::grep_many(&pat, &roots, &local_walk_opts)?;
        if let Some(inside_kind_filters) = inside_kind_filters {
          let filter_kinds = inside_kind_filters
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
          sensors::attach_grep_structural_expansions(&mut res.hits, &local_walk_opts, true, true);
          use std::collections::BTreeMap;
          let mut by_file: BTreeMap<String, Vec<usize>> = BTreeMap::new();
          for (idx, hit) in res.hits.iter().enumerate() {
            by_file.entry(hit.file_path.clone()).or_default().push(idx);
          }

          let mut keep = vec![false; res.hits.len()];
          for (file_path, hit_indices) in by_file {
            let line_numbers = hit_indices
              .iter()
              .map(|idx| res.hits[*idx].line_number)
              .collect::<Vec<_>>();
            let file_inside = sensors::grep_hits_inside_kinds(
              Path::new(&file_path),
              &local_walk_opts,
              &line_numbers,
              &filter_kinds,
            );
            for (local_idx, hit_idx) in hit_indices.iter().enumerate() {
              if file_inside.get(local_idx).copied().unwrap_or(false) {
                keep[*hit_idx] = true;
              }
            }
          }
          let mut filtered_hits = Vec::with_capacity(res.hits.len());
          for (idx, hit) in res.hits.into_iter().enumerate() {
            if keep.get(idx).copied().unwrap_or(false) {
              filtered_hits.push(hit);
            }
          }
          res.hits = filtered_hits;
        } else if parents || body {
          sensors::attach_grep_structural_expansions(
            &mut res.hits,
            &local_walk_opts,
            parents,
            body,
          );
        }
        if let Some(lim) = common.limit {
          if res.hits.len() > lim {
            res.hits.truncate(lim);
          }
        }
        if role {
          sensors::attach_grep_roles(&mut res.hits, &pat, &local_walk_opts);
        }
        attach_grep_context(&mut res.hits, context_lines);
        if has_inside_kind_filter
          && res.hits.is_empty()
          && matches!(res.exit_status, ExitStatus::Matched)
        {
          res.exit_status = ExitStatus::NoMatch;
        }
        render_by_format(&res, format, "grep", |r| {
          render_grep_hits_markdown(r, context_lines)
        });
        ("grep", res.exit_status, res.warnings)
      }
    }
    Commands::Find {
      pattern,
      path,
      paths,
      quick_projects,
      paths_only,
      null,
      display,
      older_than,
      action,
      force,
      filter,
      prune,
    } => {
      if force && action != Some(FileAction::Delete) {
        if crate::program_output::enabled() {
          return Err(crate::error::anyhow!(
            "INVALID_ACTION: --force requires --action delete"
          ));
        }
        clap::Error::raw(
          clap::error::ErrorKind::ArgumentConflict,
          "--force requires --action delete",
        )
        .exit();
      }
      if force {
        refuse_mutation_under_workspace(workspace_ctx.as_ref(), "delete")?;
      }
      let pat = if common.ignore_case {
        format!("(?i){}", pattern)
      } else {
        pattern
      };
      let missing_path_diags = explicit_missing_read_path_diagnostics(path.as_ref(), &paths);
      let mut res = if !missing_path_diags.is_empty() {
        let mut res = crate::schema::FindResult::new(Vec::new());
        res.exit_status = ExitStatus::Error;
        res.warnings = missing_path_diags;
        res
      } else {
        let roots = collect_read_paths(path, paths);
        let mut local_walk_opts = walk_opts.clone();
        local_walk_opts.quick_project_roots = quick_projects;
        sensors::find_many_with_predicates(
          &pat,
          &roots,
          &local_walk_opts,
          older_than,
          filter.as_ref(),
          prune.as_ref(),
        )?
      };
      if action == Some(FileAction::Delete) && res.exit_status != ExitStatus::Error {
        if force && res.partial {
          res.exit_status = ExitStatus::Error;
          res.push_warning(crate::schema::Diagnostic::new(
            "DELETE_REFUSED",
            "selection is incomplete; no files deleted",
          ));
        } else {
          if force {
            crate::program_output::protect(
              res
                .files
                .iter()
                .map(|file| PathBuf::from(&file.absolute_path)),
            )
            .map_err(|error| crate::error::anyhow!(error))?;
          }
          let result = file_actions::delete(&res.files, force);
          res.exit_status = result.exit_status;
          res.partial |= result.partial;
          res.warnings.extend(result.warnings.clone());
          res.action = Some(result);
        }
      }
      if paths_only {
        render_find_paths_only(&res, null);
      } else {
        render_by_format(&res, format, "find", |res| {
          render_find_columns(res, &display);
          if let Some(action) = &res.action {
            stdout_line(&format!(
              "{}: {} selected, {} deleted",
              action.mode,
              action.files.len(),
              action.deleted_count
            ));
          }
        });
      }
      ("find", res.exit_status, res.warnings)
    }
    Commands::Delete {
      force,
      null,
      display,
    } => {
      // stdin must be fully decoded before a delete can happen.
      if force {
        refuse_mutation_under_workspace(workspace_ctx.as_ref(), "delete")?;
      }
      use std::io::Read;
      let mut input = String::new();
      let selection = std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| e.to_string())
        .and_then(|_| file_actions::read_selection(&input, null));
      let res = match selection {
        Ok(files) => {
          if force {
            crate::program_output::protect(
              files.iter().map(|file| PathBuf::from(&file.absolute_path)),
            )
            .map_err(|error| crate::error::anyhow!(error))?;
          }
          file_actions::delete(&files, force)
        }
        Err(message) => {
          let mut result = file_actions::delete(&[], false);
          result.exit_status = ExitStatus::Error;
          result.warnings.push(crate::schema::Diagnostic::new(
            "FILE_SELECTION_FAILED",
            message,
          ));
          result
        }
      };
      render_by_format(&res, format, "delete", |res| {
        render_delete_markdown(res, &display)
      });
      ("delete", res.exit_status, res.warnings)
    }
    Commands::PySliceCandidates { trace_plan, paths } => {
      let missing_path_diags = explicit_missing_read_path_diagnostics(None, &paths);
      let res = if missing_path_diags.is_empty() {
        sensors::py_slice_candidates(&paths, &walk_opts, trace_plan)?
      } else {
        let mut res = crate::schema::PySliceCandidatesResult::new(Vec::new());
        res.exit_status = ExitStatus::Error;
        for d in missing_path_diags {
          res.push_warning(d);
        }
        res
      };
      render_by_format(&res, format, "py_slice_candidates", |r| {
        for candidate in &r.candidates {
          stdout_line(&format!(
            "{}:{}: candidate {} = {}; forwarded with {} at line {} (confidence: {})",
            candidate.file_path,
            candidate.slice_line_number,
            candidate.derived_sliced_variable,
            candidate.slice_expression,
            candidate.related_unsliced_variable,
            candidate.call_line_number,
            candidate.confidence
          ));
        }
      });
      ("py_slice_candidates", res.exit_status, res.warnings)
    }
    Commands::Callsites {
      callee,
      args: _args,
      around,
      path,
    } => {
      let missing_path_diags = explicit_missing_read_path_diagnostics(Some(&path), &[]);
      let res = if missing_path_diags.is_empty() {
        sensors::callsites_args(&callee, &path, around, &walk_opts)?
      } else {
        let mut res = crate::schema::CallsitesResult::new(Vec::new());
        res.exit_status = ExitStatus::Error;
        for d in missing_path_diags {
          res.push_warning(d);
        }
        res
      };
      render_by_format(&res, format, "callsites", |r| {
        for callsite in &r.callsites {
          let mut args = callsite.positional_args.clone();
          args.extend(
            callsite
              .keyword_args
              .iter()
              .map(|arg| format!("{}={}", arg.name, arg.value)),
          );
          stdout_line(&format!(
            "{}:{}: {}({})",
            callsite.file_path,
            callsite.line_number,
            callsite.callee_text,
            args.join(", ")
          ));
          for context in &callsite.source_context {
            let sep = if context.line_number == callsite.line_number {
              ":"
            } else {
              "-"
            };
            stdout_line(&format!(
              "{}{}{}: {}",
              callsite.file_path, sep, context.line_number, context.line_content
            ));
          }
        }
      });
      ("callsites", res.exit_status, res.warnings)
    }
    Commands::Serve {
      addr: _addr,
      auth_token: _auth_token,
    } => {
      #[cfg(feature = "serve")]
      {
        serve_graphql(_addr, _auth_token)?;
        return Ok(());
      }
      #[cfg(not(feature = "serve"))]
      {
        return Err(crate::error::anyhow!(
                    "the `serve` subcommand requires `--features serve`; rebuild with `cargo build --features serve`"
                ));
      }
    }
    Commands::Query {
      query,
      recipe,
      all_shapes,
      vars,
      vars_file,
      operation_name,
    } => dispatch_query(
      query,
      recipe,
      all_shapes,
      vars,
      vars_file,
      operation_name,
      common.infile.clone(),
      format,
      workspace_ctx.clone(),
    )?,
    Commands::History { cmd } => dispatch_history(cmd, format, no_cache, &common)?,
  };

  // §0.7 — mirror diagnostics to stderr and map exit_status to
  // process exit code. `-q/--quiet` suppresses the stderr mirror.
  // In explicit JSON mode, non-error warnings stay envelope-only
  // unless `-v/--verbose` is set; error-status diagnostics remain
  // visible on stderr.
  let partial = has_primary_path_partial_coverage(&warnings) || crate::program_output::partial();
  if should_render_diagnostics_stderr(format, status, partial, common.quiet, common.verbose) {
    render_diagnostics_stderr(verb, &warnings, common.verbose);
  }
  let code = if crate::program_output::failed() {
    2
  } else {
    exit_code_for(status, exit_zero, partial)
  };
  if code != 0 {
    process::exit(code);
  }
  Ok(())
}

/// Dispatch one of the 8 `lmprobe history <verb>` subcommands.
/// Returns the same `(verb, status, warnings)` tuple shape the main
/// run() match arms use so post-render diagnostics + exit-code
/// mapping are uniform.
fn dispatch_history(
  cmd: HistoryCmd,
  format: OutputFormat,
  no_cache: bool,
  common: &CommonArgs,
) -> Result<(&'static str, ExitStatus, Vec<crate::schema::Diagnostic>)> {
  use crate::history_sensors as hs;

  match cmd {
    HistoryCmd::Blame {
      target,
      path,
      around,
      follow,
      semantic,
    } => {
      let root = hs::resolve_root(&path);
      let res =
        hs::blame_with_follow_semantic_around(&target, &root, follow, semantic, no_cache, around)?;
      render_by_format(&res, format, "history_blame", |r| {
        println!("# lmprobe.history_blame.v1");
        if !r.source_context.is_empty() {
          println!("## source context");
          for line in &r.source_context {
            println!("{:>6}: {}", line.line_number, line.line_content);
          }
        }
        for e in &r.entries {
          println!(
            "- {} [{}] {} — {} {}",
            &e.commit_hash[..e.commit_hash.len().min(12)],
            e.change_type,
            e.author,
            e.file_path,
            e.timestamp
          );
          if !e.excerpt.is_empty() {
            println!("    {}", e.excerpt.lines().next().unwrap_or(""));
          }
        }
      });
      Ok(("history_blame", res.exit_status, res.warnings))
    }
    HistoryCmd::Introduced {
      name,
      path,
      lang: _,
      follow,
    } => {
      let root = hs::resolve_root(&path);
      let res = hs::introduced_with_follow(&name, &root, None, follow, no_cache)?;
      render_by_format(&res, format, "history_introduced", |r| {
        println!("# lmprobe.history_introduced.v1");
        for e in &r.entries {
          println!(
            "- {} {} — {} {}",
            &e.commit_hash[..e.commit_hash.len().min(12)],
            e.author,
            e.file_path,
            e.timestamp
          );
          if !e.excerpt.is_empty() {
            println!("    {}", e.excerpt.lines().next().unwrap_or(""));
          }
        }
      });
      Ok(("history_introduced", res.exit_status, res.warnings))
    }
    HistoryCmd::Commit {
      pattern,
      since,
      author,
      path,
    } => {
      let root = hs::resolve_root(&path);
      let res = hs::history_commit(
        pattern.as_deref(),
        since.as_deref(),
        author.as_deref(),
        &root,
      )?;
      render_by_format(&res, format, "history_commit", |r| {
        println!("# lmprobe.history_commit.v1");
        for h in &r.hits {
          println!(
            "- {} {} — {}",
            &h.commit_hash[..h.commit_hash.len().min(12)],
            h.author,
            h.message.lines().next().unwrap_or("")
          );
          if !h.matched_files.is_empty() {
            println!("    files: {}", h.matched_files.join(", "));
          }
        }
      });
      Ok(("history_commit", res.exit_status, res.warnings))
    }
    HistoryCmd::Diff {
      pattern,
      since,
      path,
    } => {
      let root = hs::resolve_root(&path);
      let res = hs::history_diff(&pattern, since.as_deref(), &root)?;
      render_by_format(&res, format, "history_diff", |r| {
        println!("# lmprobe.history_diff.v1");
        for h in &r.hits {
          println!(
            "- {} {}:{}",
            &h.commit_hash[..h.commit_hash.len().min(12)],
            h.file_path,
            h.timestamp
          );
          if !h.before.is_empty() {
            println!("    - {}", h.before.lines().next().unwrap_or(""));
          }
          if !h.after.is_empty() {
            println!("    + {}", h.after.lines().next().unwrap_or(""));
          }
        }
      });
      Ok(("history_diff", res.exit_status, res.warnings))
    }
    HistoryCmd::Churn { top, since, path } => {
      let root = hs::resolve_root(&path);
      let res = hs::churn(top, since.as_deref(), &root)?;
      render_by_format(&res, format, "history_churn", |r| {
        println!("# lmprobe.history_churn.v1");
        for e in &r.rows {
          println!(
            "- {:5} {}  (last: {})",
            e.commit_count, e.path, e.last_touched
          );
        }
      });
      Ok(("history_churn", res.exit_status, res.warnings))
    }
    HistoryCmd::CoChange {
      file,
      since,
      top,
      min_score,
      path,
    } => {
      let root = hs::resolve_root(&path);
      let res = hs::co_change(&file, since.as_deref(), &root)?;
      let res = hs::filter_co_change_result(
        res,
        top,
        min_score,
        &common.exclude_globs,
        common.r#type.as_deref(),
      );
      render_by_format(&res, format, "history_co_change", |r| {
        println!("# lmprobe.history_co_change.v1");
        for e in &r.rows {
          println!("- {:5} ({:.2}) {}", e.co_occurrences, e.score, e.path);
        }
      });
      Ok(("history_co_change", res.exit_status, res.warnings))
    }
    HistoryCmd::Todos { since, path } => {
      let root = hs::resolve_root(&path);
      let res = hs::todos(since.as_deref(), &root)?;
      render_by_format(&res, format, "history_todos", |r| {
        println!("# lmprobe.history_todos.v1");
        for h in &r.hits {
          println!(
            "- {} [{}] {}:{}  {}",
            &h.commit_hash[..h.commit_hash.len().min(12)],
            h.marker,
            h.file_path,
            h.line_number,
            h.text
          );
        }
      });
      Ok(("history_todos", res.exit_status, res.warnings))
    }
    HistoryCmd::Evolution {
      name,
      path,
      force,
      regex,
    } => {
      // Alias: identical to top-level Commands::Evolution. The
      // global `--no-cache` flag threads through here so the
      // alias and the original verb produce identical output.
      let walk_opts = sensors::WalkOpts::default();
      let res = sensors::evolution_with_regex(&name, &path, force, no_cache, regex, &walk_opts)?;
      render_by_format(&res, format, "evolution", |r| {
        render_evolution_markdown(r);
      });
      Ok(("evolution", res.exit_status, res.warnings))
    }
  }
}

fn exit_code_for(status: ExitStatus, exit_zero_on_no_match: bool, partial: bool) -> i32 {
  if partial && status != ExitStatus::Error {
    return 3;
  }
  match status {
    ExitStatus::Matched => 0,
    ExitStatus::NoMatch => {
      if exit_zero_on_no_match {
        0
      } else {
        1
      }
    }
    ExitStatus::Error => 2,
  }
}

fn has_primary_path_partial_coverage(warnings: &[crate::schema::Diagnostic]) -> bool {
  warnings
    .iter()
    .any(|warning| is_coverage_partial_diagnostic_code(&warning.code))
}

fn is_coverage_partial_diagnostic_code(code: &str) -> bool {
  matches!(
    code,
    "FILE_METADATA_FAILED"
      | "FILE_TOO_LARGE"
      | "FILE_READ_FAILED"
      | "WALK_ERROR"
      | "WALK_ENTRY_ERROR"
      | "DELETE_FAILED"
      | "GIT_GRAPH_INCOMPLETE"
      | "GIT_REMOTE_NAME_ENCODING"
      | "AWS_PAGE_INCOMPLETE"
      | "AWS_PARTIAL_FAILURE"
  )
}

#[derive(serde::Serialize)]
struct RecipeListEnvelope {
  schema: &'static str,
  recipes: Vec<RecipeRow>,
  warnings: Vec<ResultDiagnostic>,
  exit_status: ExitStatus,
}

#[derive(serde::Serialize)]
struct RecipeShowEnvelope {
  schema: &'static str,
  provider_id: String,
  language: String,
  sdk: String,
  shape_index: usize,
  graphql: String,
  warnings: Vec<ResultDiagnostic>,
  exit_status: ExitStatus,
}

#[derive(Clone, serde::Serialize)]
struct RecipeRow {
  provider_id: String,
  language: String,
  sdk: String,
  shape_index: usize,
  kind: String,
  callee: String,
  credential_kind: String,
  rule_ids: Vec<String>,
}

struct RecipeShapeRef<'a> {
  provider: &'a lmprobe_secrets::Provider,
  language: &'a lmprobe_secrets::ProviderLanguage,
  shape_index: usize,
  shape: &'a lmprobe_secrets::CallShape,
}

#[derive(serde::Serialize)]
struct SourceRecipeListEnvelope {
  schema: &'static str,
  recipes: Vec<SourceRecipeRow>,
  warnings: Vec<ResultDiagnostic>,
  exit_status: ExitStatus,
}

#[derive(serde::Serialize)]
struct SourceRecipeShowEnvelope {
  schema: &'static str,
  source_id: String,
  source_category: String,
  provider_id: Option<String>,
  language: String,
  sdk: String,
  shape_index: usize,
  graphql: String,
  warnings: Vec<ResultDiagnostic>,
  exit_status: ExitStatus,
}

#[derive(Clone, serde::Serialize)]
struct SourceRecipeRow {
  source_id: String,
  source_category: String,
  provider_id: Option<String>,
  language: String,
  sdk: String,
  shape_index: usize,
  kind: String,
  selector: String,
  credential_kind: String,
}

struct SourceRecipeShapeRef<'a> {
  source: &'a lmprobe_secrets::Source,
  language: &'a lmprobe_secrets::SourceLanguage,
  shape_index: usize,
  shape: &'a lmprobe_secrets::ReadShape,
}

fn dispatch_secrets_recipes(
  cmd: SecretsRecipesCmd,
  format: OutputFormat,
  _walk_opts: &WalkOpts,
) -> Result<(&'static str, ExitStatus, Vec<ResultDiagnostic>)> {
  let db = lmprobe_secrets::load_default_provider_db()
    .map_err(|err| crate::error::anyhow!("load provider DB: {}", err))?;
  match cmd {
    SecretsRecipesCmd::List { provider, language } => {
      let rows = recipe_rows(&db, provider.as_deref(), language.as_deref());
      let status = ExitStatus::from_count(rows.len());
      let env = RecipeListEnvelope {
        schema: "lmprobe.secrets_recipes.v1",
        recipes: rows,
        warnings: Vec::new(),
        exit_status: status,
      };
      match format {
        OutputFormat::Json => render_envelope_json(&env),
        OutputFormat::Auto | OutputFormat::Markdown => render_recipe_rows_table(&env.recipes),
        _ => render_envelope_json(&env),
      }
      Ok(("secrets_recipes", status, Vec::new()))
    }
    SecretsRecipesCmd::Show { key } => {
      let selected = select_recipe_shapes(&db, &key, false)?;
      let selected = selected
        .first()
        .ok_or_else(|| crate::error::anyhow!("no recipe matched `{}`", key))?;
      let graphql = lmprobe_secrets::generate_graphql_recipe(
        selected.provider,
        selected.language,
        selected.shape,
      );
      let env = RecipeShowEnvelope {
        schema: "lmprobe.secrets_recipes.show.v1",
        provider_id: selected.provider.id.clone(),
        language: selected.language.language.to_string(),
        sdk: selected.language.sdk.clone(),
        shape_index: selected.shape_index,
        graphql,
        warnings: Vec::new(),
        exit_status: ExitStatus::Matched,
      };
      match format {
        OutputFormat::Json => render_envelope_json(&env),
        OutputFormat::Auto | OutputFormat::Markdown => stdout_line(&env.graphql),
        _ => render_envelope_json(&env),
      }
      Ok(("secrets_recipes", ExitStatus::Matched, Vec::new()))
    }
  }
}

fn dispatch_secrets_sources(
  cmd: SecretsSourcesCmd,
  format: OutputFormat,
  _walk_opts: &WalkOpts,
) -> Result<(&'static str, ExitStatus, Vec<ResultDiagnostic>)> {
  let db = lmprobe_secrets::load_default_source_db()
    .map_err(|err| crate::error::anyhow!("load source DB: {}", err))?;
  match cmd {
    SecretsSourcesCmd::List {
      source,
      language,
      category,
      provider,
    } => {
      let rows = source_recipe_rows(
        &db,
        source.as_deref(),
        language.as_deref(),
        category.as_deref(),
        provider.as_deref(),
      );
      let status = ExitStatus::from_count(rows.len());
      let env = SourceRecipeListEnvelope {
        schema: "lmprobe.secrets_sources.v1",
        recipes: rows,
        warnings: Vec::new(),
        exit_status: status,
      };
      match format {
        OutputFormat::Json => render_envelope_json(&env),
        OutputFormat::Auto | OutputFormat::Markdown => {
          render_source_recipe_rows_table(&env.recipes)
        }
        _ => render_envelope_json(&env),
      }
      Ok(("secrets_sources", status, Vec::new()))
    }
    SecretsSourcesCmd::Show { key } => {
      let selected = select_source_recipe_shapes(&db, &key, false)?;
      let selected = selected
        .first()
        .ok_or_else(|| crate::error::anyhow!("no source recipe matched `{}`", key))?;
      let graphql = lmprobe_secrets::generate_source_graphql_recipe(
        selected.source,
        selected.language.language,
        selected.shape,
      );
      let env = SourceRecipeShowEnvelope {
        schema: "lmprobe.secrets_sources.show.v1",
        source_id: selected.source.id.clone(),
        source_category: source_category_name(selected.source.category).to_string(),
        provider_id: selected.source.provider_id.clone(),
        language: selected.language.language.to_string(),
        sdk: selected.language.sdk.clone(),
        shape_index: selected.shape_index,
        graphql,
        warnings: Vec::new(),
        exit_status: ExitStatus::Matched,
      };
      match format {
        OutputFormat::Json => render_envelope_json(&env),
        OutputFormat::Auto | OutputFormat::Markdown => stdout_line(&env.graphql),
        _ => render_envelope_json(&env),
      }
      Ok(("secrets_sources", ExitStatus::Matched, Vec::new()))
    }
  }
}

fn recipe_rows(
  db: &lmprobe_secrets::ProviderDb,
  provider_filter: Option<&str>,
  language_filter: Option<&str>,
) -> Vec<RecipeRow> {
  db.providers
    .iter()
    .filter(|provider| provider_filter.is_none_or(|id| provider.id == id))
    .flat_map(|provider| {
      provider
        .languages
        .iter()
        .filter(move |language| {
          language_filter.is_none_or(|id| language.language.to_string() == id)
        })
        .flat_map(move |language| {
          language
            .call_shapes
            .iter()
            .enumerate()
            .map(move |(shape_index, shape)| RecipeRow {
              provider_id: provider.id.clone(),
              language: language.language.to_string(),
              sdk: language.sdk.clone(),
              shape_index,
              kind: format!("{:?}", shape.kind).to_lowercase(),
              callee: display_callee(shape),
              credential_kind: provider.credential_kind.clone(),
              rule_ids: provider.rule_ids.clone(),
            })
        })
    })
    .collect()
}

fn render_recipe_rows_table(rows: &[RecipeRow]) {
  stdout_line("provider\tlanguage\tsdk\tshape\tkind\tcallee");
  for row in rows {
    stdout_line(&format!(
      "{}\t{}\t{}\t{}\t{}\t{}",
      row.provider_id, row.language, row.sdk, row.shape_index, row.kind, row.callee
    ));
  }
}

fn source_recipe_rows(
  db: &lmprobe_secrets::SourceDb,
  source_filter: Option<&str>,
  language_filter: Option<&str>,
  category_filter: Option<&str>,
  provider_filter: Option<&str>,
) -> Vec<SourceRecipeRow> {
  db.sources
    .iter()
    .filter(|source| source_filter.is_none_or(|id| source.id == id))
    .filter(|source| {
      category_filter.is_none_or(|category| source_category_name(source.category) == category)
    })
    .filter(|source| {
      provider_filter.is_none_or(|provider| source.provider_id.as_deref() == Some(provider))
    })
    .flat_map(|source| {
      source
        .languages
        .iter()
        .filter(move |language| {
          language_filter.is_none_or(|id| language.language.to_string() == id)
        })
        .flat_map(move |language| {
          language
            .read_shapes
            .iter()
            .enumerate()
            .map(move |(shape_index, shape)| SourceRecipeRow {
              source_id: source.id.clone(),
              source_category: source_category_name(source.category).to_string(),
              provider_id: source.provider_id.clone(),
              language: language.language.to_string(),
              sdk: language.sdk.clone(),
              shape_index,
              kind: read_shape_kind_name(shape.kind).to_string(),
              selector: display_source_selector(shape),
              credential_kind: source.credential_kind.clone(),
            })
        })
    })
    .collect()
}

fn render_source_recipe_rows_table(rows: &[SourceRecipeRow]) {
  stdout_line("source\tcategory\tprovider\tlanguage\tsdk\tshape\tkind\tselector");
  for row in rows {
    stdout_line(&format!(
      "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
      row.source_id,
      row.source_category,
      row.provider_id.as_deref().unwrap_or(""),
      row.language,
      row.sdk,
      row.shape_index,
      row.kind,
      row.selector
    ));
  }
}

fn execute_recipe_shape(
  provider: &lmprobe_secrets::Provider,
  language: &lmprobe_secrets::ProviderLanguage,
  shape: &lmprobe_secrets::CallShape,
  path: &Path,
  workspace_ctx: Option<crate::workspace::WorkspaceCtx>,
) -> Result<crate::schema::CallsitesResult> {
  let recipe = lmprobe_secrets::generate_graphql_recipe(provider, language, shape);
  let mut variables = serde_json::Map::new();
  variables.insert(
    "path".to_string(),
    serde_json::Value::String(path.display().to_string()),
  );
  let query = execute_graphql_query_result(recipe, variables, None, workspace_ctx)?;
  if !query.errors.is_empty() {
    let messages = query
      .errors
      .iter()
      .map(|err| err.message.as_str())
      .collect::<Vec<_>>()
      .join("; ");
    return Err(crate::error::anyhow!("recipe GraphQL failed: {}", messages));
  }
  let data = query
    .data
    .ok_or_else(|| crate::error::anyhow!("recipe GraphQL returned no data"))?;
  let callsites = data
    .get("callsites")
    .ok_or_else(|| crate::error::anyhow!("recipe GraphQL returned no callsites envelope"))?;
  serde_json::from_value(decamelize_callsites_value(callsites.clone()))
    .map_err(|err| crate::error::anyhow!("decode callsites recipe result: {}", err))
}

fn execute_source_recipe_shape(
  source: &lmprobe_secrets::Source,
  language: &lmprobe_secrets::SourceLanguage,
  shape: &lmprobe_secrets::ReadShape,
  path: &Path,
  workspace_ctx: Option<crate::workspace::WorkspaceCtx>,
) -> Result<crate::schema::SourceReadsResult> {
  let recipe = lmprobe_secrets::generate_source_graphql_recipe(source, language.language, shape);
  let mut variables = serde_json::Map::new();
  variables.insert(
    "path".to_string(),
    serde_json::Value::String(path.display().to_string()),
  );
  let query = execute_graphql_query_result(recipe, variables, None, workspace_ctx)?;
  if !query.errors.is_empty() {
    let messages = query
      .errors
      .iter()
      .map(|err| err.message.as_str())
      .collect::<Vec<_>>()
      .join("; ");
    return Err(crate::error::anyhow!(
      "source recipe GraphQL failed: {}",
      messages
    ));
  }
  let data = query
    .data
    .ok_or_else(|| crate::error::anyhow!("source recipe GraphQL returned no data"))?;
  let source_reads = data.get("sourceReads").ok_or_else(|| {
    crate::error::anyhow!("source recipe GraphQL returned no sourceReads envelope")
  })?;
  serde_json::from_value(decamelize_source_reads_value(source_reads.clone()))
    .map_err(|err| crate::error::anyhow!("decode sourceReads recipe result: {}", err))
}

fn decamelize_callsites_value(value: serde_json::Value) -> serde_json::Value {
  match value {
    serde_json::Value::Object(map) => serde_json::Value::Object(
      map
        .into_iter()
        .map(|(key, value)| {
          let value = if key == "exitStatus" {
            match value {
              serde_json::Value::String(status) => {
                serde_json::Value::String(status.to_ascii_lowercase())
              }
              other => other,
            }
          } else {
            decamelize_callsites_value(value)
          };
          (callsites_json_key(&key).to_string(), value)
        })
        .collect(),
    ),
    serde_json::Value::Array(items) => {
      serde_json::Value::Array(items.into_iter().map(decamelize_callsites_value).collect())
    }
    other => other,
  }
}

fn decamelize_source_reads_value(value: serde_json::Value) -> serde_json::Value {
  match value {
    serde_json::Value::Object(map) => serde_json::Value::Object(
      map
        .into_iter()
        .map(|(key, value)| {
          let value = if key == "exitStatus" {
            match value {
              serde_json::Value::String(status) => {
                serde_json::Value::String(status.to_ascii_lowercase())
              }
              other => other,
            }
          } else {
            decamelize_source_reads_value(value)
          };
          (source_reads_json_key(&key).to_string(), value)
        })
        .collect(),
    ),
    serde_json::Value::Array(items) => serde_json::Value::Array(
      items
        .into_iter()
        .map(decamelize_source_reads_value)
        .collect(),
    ),
    other => other,
  }
}

fn callsites_json_key(key: &str) -> &str {
  match key {
    "argValuesRedacted" => "arg_values_redacted",
    "exitStatus" => "exit_status",
    "filePath" => "file_path",
    "lineNumber" => "line_number",
    "calleeText" => "callee_text",
    "positionalArgs" => "positional_args",
    "keywordArgs" => "keyword_args",
    "sinkKind" => "sink_kind",
    "importEvidence" => "import_evidence",
    "structuredArgs" => "structured_args",
    "sourceContext" => "source_context",
    "lineContent" => "line_content",
    _ => key,
  }
}

fn source_reads_json_key(key: &str) -> &str {
  match key {
    "argValuesRedacted" => "arg_values_redacted",
    "exitStatus" => "exit_status",
    "sourceReads" => "source_reads",
    "filePath" => "file_path",
    "lineNumber" => "line_number",
    "sourceId" => "source_id",
    "sourceCategory" => "source_category",
    "providerId" => "provider_id",
    "readText" => "read_text",
    "keyName" => "key_name",
    "envVarName" => "env_var_name",
    "accessPath" => "access_path",
    "importEvidence" => "import_evidence",
    _ => key,
  }
}

enum RecipeExecutionResult {
  Callsites(crate::schema::CallsitesResult),
  SourceReads(crate::schema::SourceReadsResult),
}

fn execute_recipe_key(
  key: &str,
  path: &Path,
  all_shapes: bool,
  workspace_ctx: Option<crate::workspace::WorkspaceCtx>,
) -> Result<RecipeExecutionResult> {
  if key.starts_with("secrets-source/") {
    return execute_source_recipe_key(key, path, all_shapes, workspace_ctx)
      .map(RecipeExecutionResult::SourceReads);
  }
  let db = lmprobe_secrets::load_default_provider_db()
    .map_err(|err| crate::error::anyhow!("load provider DB: {}", err))?;
  let selected = select_recipe_shapes(&db, key, all_shapes)?;
  if selected.is_empty() {
    return Err(crate::error::anyhow!("no recipe matched `{}`", key));
  }
  let mut merged = crate::schema::CallsitesResult::new(Vec::new());
  for item in selected {
    let mut res = execute_recipe_shape(
      item.provider,
      item.language,
      item.shape,
      path,
      workspace_ctx.clone(),
    )?;
    merged.callsites.append(&mut res.callsites);
    merged.warnings.append(&mut res.warnings);
    merged.arg_values_redacted |= res.arg_values_redacted;
  }
  merged.exit_status = ExitStatus::from_count(merged.callsites.len());
  Ok(RecipeExecutionResult::Callsites(merged))
}

fn execute_source_recipe_key(
  key: &str,
  path: &Path,
  all_shapes: bool,
  workspace_ctx: Option<crate::workspace::WorkspaceCtx>,
) -> Result<crate::schema::SourceReadsResult> {
  let db = lmprobe_secrets::load_default_source_db()
    .map_err(|err| crate::error::anyhow!("load source DB: {}", err))?;
  let selected = select_source_recipe_shapes(&db, key, all_shapes)?;
  if selected.is_empty() {
    return Err(crate::error::anyhow!("no source recipe matched `{}`", key));
  }
  let mut merged = crate::schema::SourceReadsResult::new(Vec::new());
  for item in selected {
    let mut res = execute_source_recipe_shape(
      item.source,
      item.language,
      item.shape,
      path,
      workspace_ctx.clone(),
    )?;
    merged.source_reads.append(&mut res.source_reads);
    merged.warnings.append(&mut res.warnings);
    merged.arg_values_redacted |= res.arg_values_redacted;
  }
  merged.exit_status = ExitStatus::from_count(merged.source_reads.len());
  Ok(merged)
}

fn select_recipe_shapes<'a>(
  db: &'a lmprobe_secrets::ProviderDb,
  key: &str,
  all_shapes: bool,
) -> Result<Vec<RecipeShapeRef<'a>>> {
  let parsed = parse_recipe_key(key)?;
  let provider = db
    .providers
    .iter()
    .find(|provider| provider.id == parsed.provider)
    .ok_or_else(|| crate::error::anyhow!("unknown provider `{}`", parsed.provider))?;
  let language_entries = provider
    .languages
    .iter()
    .filter(|language| language.language.to_string() == parsed.language)
    .filter(|language| parsed.sdk.as_ref().is_none_or(|sdk| language.sdk == *sdk))
    .collect::<Vec<_>>();
  if language_entries.is_empty() {
    return Err(crate::error::anyhow!(
      "no recipe for `{}/{}`{}",
      parsed.provider,
      parsed.language,
      parsed
        .sdk
        .as_ref()
        .map(|sdk| format!("/{sdk}"))
        .unwrap_or_default()
    ));
  }
  if parsed.sdk.is_none() && language_entries.len() > 1 {
    let sdks = language_entries
      .iter()
      .map(|language| language.sdk.as_str())
      .collect::<Vec<_>>()
      .join(", ");
    return Err(crate::error::anyhow!(
      "multiple SDKs match `{}/{}`; include one of: {}",
      parsed.provider,
      parsed.language,
      sdks
    ));
  }
  let language = language_entries[0];
  let shapes = if all_shapes {
    language.call_shapes.iter().enumerate().collect::<Vec<_>>()
  } else {
    language.call_shapes.iter().take(1).enumerate().collect()
  };
  Ok(
    shapes
      .into_iter()
      .map(|(shape_index, shape)| RecipeShapeRef {
        provider,
        language,
        shape_index,
        shape,
      })
      .collect(),
  )
}

fn select_source_recipe_shapes<'a>(
  db: &'a lmprobe_secrets::SourceDb,
  key: &str,
  all_shapes: bool,
) -> Result<Vec<SourceRecipeShapeRef<'a>>> {
  let parsed = parse_source_recipe_key(key)?;
  let source = db
    .sources
    .iter()
    .find(|source| source.id == parsed.source)
    .ok_or_else(|| crate::error::anyhow!("unknown source `{}`", parsed.source))?;
  let language_entries = source
    .languages
    .iter()
    .filter(|language| language.language.to_string() == parsed.language)
    .filter(|language| parsed.sdk.as_ref().is_none_or(|sdk| language.sdk == *sdk))
    .collect::<Vec<_>>();
  if language_entries.is_empty() {
    return Err(crate::error::anyhow!(
      "no source recipe for `{}/{}`{}",
      parsed.source,
      parsed.language,
      parsed
        .sdk
        .as_ref()
        .map(|sdk| format!("/{sdk}"))
        .unwrap_or_default()
    ));
  }
  if parsed.sdk.is_none() && language_entries.len() > 1 {
    let sdks = language_entries
      .iter()
      .map(|language| language.sdk.as_str())
      .collect::<Vec<_>>()
      .join(", ");
    return Err(crate::error::anyhow!(
      "multiple SDKs match `{}/{}`; include one of: {}",
      parsed.source,
      parsed.language,
      sdks
    ));
  }
  let language = language_entries[0];
  let shapes = if all_shapes {
    language.read_shapes.iter().enumerate().collect::<Vec<_>>()
  } else {
    language.read_shapes.iter().take(1).enumerate().collect()
  };
  Ok(
    shapes
      .into_iter()
      .map(|(shape_index, shape)| SourceRecipeShapeRef {
        source,
        language,
        shape_index,
        shape,
      })
      .collect(),
  )
}

struct ParsedRecipeKey {
  provider: String,
  language: String,
  sdk: Option<String>,
}

fn parse_recipe_key(key: &str) -> Result<ParsedRecipeKey> {
  let key = key.strip_prefix("secrets/").unwrap_or(key);
  let parts = key.split('/').collect::<Vec<_>>();
  match parts.as_slice() {
    [provider, language] if !provider.is_empty() && !language.is_empty() => Ok(ParsedRecipeKey {
      provider: (*provider).to_string(),
      language: (*language).to_string(),
      sdk: None,
    }),
    [provider, language, sdk]
      if !provider.is_empty() && !language.is_empty() && !sdk.is_empty() =>
    {
      Ok(ParsedRecipeKey {
        provider: (*provider).to_string(),
        language: (*language).to_string(),
        sdk: Some((*sdk).to_string()),
      })
    }
    _ => Err(crate::error::anyhow!(
      "recipe key must be <provider>/<language>[/<sdk>] or secrets/<provider>/<language>[/<sdk>]"
    )),
  }
}

struct ParsedSourceRecipeKey {
  source: String,
  language: String,
  sdk: Option<String>,
}

fn parse_source_recipe_key(key: &str) -> Result<ParsedSourceRecipeKey> {
  let key = key.strip_prefix("secrets-source/").unwrap_or(key);
  let parts = key.split('/').collect::<Vec<_>>();
  match parts.as_slice() {
    [source, language] if !source.is_empty() && !language.is_empty() => {
      Ok(ParsedSourceRecipeKey {
        source: (*source).to_string(),
        language: (*language).to_string(),
        sdk: None,
      })
    }
    [source, language, sdk]
      if !source.is_empty() && !language.is_empty() && !sdk.is_empty() =>
    {
      Ok(ParsedSourceRecipeKey {
        source: (*source).to_string(),
        language: (*language).to_string(),
        sdk: Some((*sdk).to_string()),
      })
    }
    _ => Err(crate::error::anyhow!(
      "source recipe key must be <source>/<language>[/<sdk>] or secrets-source/<source>/<language>[/<sdk>]"
    )),
  }
}

fn display_callee(shape: &lmprobe_secrets::CallShape) -> String {
  shape
    .callee
    .as_deref()
    .or(shape.struct_type.as_deref())
    .or(shape.assignment_target.as_deref())
    .unwrap_or("")
    .to_string()
}

fn display_source_selector(shape: &lmprobe_secrets::ReadShape) -> String {
  shape
    .access_paths
    .first()
    .or_else(|| shape.callees.first())
    .or_else(|| shape.object_paths.first())
    .or_else(|| shape.key_names.first())
    .or_else(|| shape.keys.first())
    .cloned()
    .unwrap_or_default()
}

fn source_category_name(category: lmprobe_secrets::SourceCategory) -> &'static str {
  match category {
    lmprobe_secrets::SourceCategory::Env => "env",
    lmprobe_secrets::SourceCategory::Vault => "vault",
    lmprobe_secrets::SourceCategory::SecretManager => "secret-manager",
    lmprobe_secrets::SourceCategory::Dotenv => "dotenv",
    lmprobe_secrets::SourceCategory::ConfigFile => "config-file",
  }
}

fn read_shape_kind_name(kind: lmprobe_secrets::ReadShapeKind) -> &'static str {
  match kind {
    lmprobe_secrets::ReadShapeKind::EnvRead => "env_read",
    lmprobe_secrets::ReadShapeKind::KvGet => "kv_get",
    lmprobe_secrets::ReadShapeKind::SdkCall => "sdk_call",
    lmprobe_secrets::ReadShapeKind::DotenvLoad => "dotenv_load",
    lmprobe_secrets::ReadShapeKind::ConfigLookup => "config_lookup",
  }
}

// SymbolHit child-resolver bridge. Phase 5A workspace split moved
// the `#[ComplexObject] impl SymbolHit` block to `lmprobe-types`
// (orphan rule). The actual sensor work (callers / callees /
// recent_history) stays here in the binary where the sensor
// functions are reachable; an `Arc<dyn SymbolHitChildResolvers>`
// instance is registered as schema Context data so the types-side
// resolvers can fetch + delegate. Lives here in 5A.1; moves to
// lmprobe-engine in a later 5A step alongside the sensors.
struct SymbolHitChildResolversImpl;

#[async_trait::async_trait]
impl crate::schema::SymbolHitChildResolvers for SymbolHitChildResolversImpl {
  async fn callers(
    &self,
    ctx: &async_graphql::Context<'_>,
    hit: &crate::schema::SymbolHit,
    limit: Option<i32>,
    path: Option<String>,
  ) -> std::result::Result<crate::schema::CallersResult, async_graphql::Error> {
    let root = path.as_deref().unwrap_or(".");
    let mut res = sensors::callers(&hit.name, std::path::Path::new(root), &gql_walk_opts())
      .map_err(|e| async_graphql::Error::new(e.to_string()))?;
    let cap = limit.map(|n| n.max(0) as usize).unwrap_or(10);
    if res.hits.len() > cap {
      res.hits.truncate(cap);
    }
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn callees(
    &self,
    ctx: &async_graphql::Context<'_>,
    hit: &crate::schema::SymbolHit,
    limit: Option<i32>,
    path: Option<String>,
  ) -> std::result::Result<crate::schema::CalleesResult, async_graphql::Error> {
    let root = path.as_deref().unwrap_or(".");
    let mut res =
      crate::graph_sensors::callees(&hit.name, std::path::Path::new(root), &gql_walk_opts())
        .map_err(|e| async_graphql::Error::new(e.to_string()))?;
    let cap = limit.map(|n| n.max(0) as usize).unwrap_or(10);
    if res.hits.len() > cap {
      res.hits.truncate(cap);
    }
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn recent_history(
    &self,
    ctx: &async_graphql::Context<'_>,
    hit: &crate::schema::SymbolHit,
    limit: Option<i32>,
    path: Option<String>,
  ) -> std::result::Result<crate::schema::BlameResult, async_graphql::Error> {
    let root = path.as_deref().unwrap_or(".");
    let resolved = crate::history_sensors::resolve_root(std::path::Path::new(root));
    let mut res =
      crate::history_sensors::blame_with_follow_semantic(&hit.name, &resolved, false, false, false)
        .map_err(|e| async_graphql::Error::new(e.to_string()))?;
    let cap = limit.map(|n| n.max(0) as usize).unwrap_or(3);
    if res.entries.len() > cap {
      res.entries.truncate(cap);
    }
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
}

// Build the GraphQL schema. Shared between the in-process `query`
// subcommand and the HTTP daemon — schema-first invariant.
//
// Optional `workspace_ctx` is stashed in the schema data so
// mutation resolvers can refuse with `WORKSPACE_READ_ONLY` when
// the user invoked lmprobe under `--workspace`. None means
// single-root mode (mutations allowed pending D7 checks).
//
// Registers the SymbolHit child-resolver impl on every schema
// instance; types-side ComplexObject methods fetch via
// `ctx.data::<Arc<dyn SymbolHitChildResolvers>>()`.
pub(crate) fn build_schema(
  workspace_ctx: Option<crate::workspace::WorkspaceCtx>,
) -> Schema<Query, Mutation, EmptySubscription> {
  let resolvers: std::sync::Arc<dyn crate::schema::SymbolHitChildResolvers> =
    std::sync::Arc::new(SymbolHitChildResolversImpl);
  let mut builder = Schema::build(Query, Mutation, EmptySubscription).data(resolvers);
  if let Some(ws) = workspace_ctx {
    builder = builder.data(ws);
  }
  builder.finish()
}

/// Resolve the `query` source argument:
///   - `-`         → read entire stdin
///   - `@<path>`   → read the file at `<path>`
///   - otherwise   → treat the arg itself as the GraphQL document
fn read_query_source(arg: &str) -> Result<String> {
  if arg == "-" {
    let mut buf = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
      .map_err(|e| crate::error::anyhow!("read stdin: {}", e))?;
    Ok(buf)
  } else if let Some(path) = arg.strip_prefix('@') {
    std::fs::read_to_string(path)
      .map_err(|e| crate::error::anyhow!("read query file `{}`: {}", path, e))
  } else {
    Ok(arg.to_string())
  }
}

/// Parse `--var KEY=VALUE` pairs and a `--vars-file <path>` JSON
/// object into a single `serde_json::Map`. Repeated `--var` keys
/// override earlier ones; `--var` overrides `--vars-file`.
fn build_query_variables(
  vars: Vec<String>,
  vars_file: Option<PathBuf>,
) -> Result<serde_json::Map<String, serde_json::Value>> {
  let mut map = serde_json::Map::new();
  if let Some(path) = vars_file {
    let s = std::fs::read_to_string(&path)
      .map_err(|e| crate::error::anyhow!("read vars-file `{}`: {}", path.display(), e))?;
    let obj: serde_json::Value = serde_json::from_str(&s)
      .map_err(|e| crate::error::anyhow!("parse vars-file `{}`: {}", path.display(), e))?;
    match obj {
      serde_json::Value::Object(o) => {
        for (k, v) in o {
          map.insert(k, v);
        }
      }
      _ => {
        return Err(crate::error::anyhow!(
          "vars-file `{}` must be a JSON object",
          path.display()
        ));
      }
    }
  }
  for raw in vars {
    let (k, v) = raw
      .split_once('=')
      .ok_or_else(|| crate::error::anyhow!("--var expects KEY=VALUE; got `{}`", raw))?;
    // Try JSON first (so `--var limit=10` is a number); fall back
    // to a JSON string for free-form inputs.
    let val = serde_json::from_str::<serde_json::Value>(v)
      .unwrap_or_else(|_| serde_json::Value::String(v.to_string()));
    map.insert(k.to_string(), val);
  }
  Ok(map)
}

fn execute_graphql_query_result(
  document: String,
  variables: serde_json::Map<String, serde_json::Value>,
  operation_name: Option<String>,
  workspace_ctx: Option<crate::workspace::WorkspaceCtx>,
) -> Result<crate::schema::QueryResult> {
  use crate::schema::{QueryError, QueryErrorLocation, QueryResult};

  let schema = build_schema(workspace_ctx);
  let exit_states: QueryExitStates = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
  let progress: QueryProgressState =
    std::sync::Arc::new(std::sync::Mutex::new(QueryProgress::default()));
  let mut req = async_graphql::Request::new(document)
    .data(progress.clone())
    .data(exit_states.clone())
    .variables(async_graphql::Variables::from_json(
      serde_json::Value::Object(variables),
    ));
  if let Some(name) = operation_name {
    req = req.operation_name(name);
  }

  let runtime = tokio::runtime::Builder::new_current_thread()
    .build()
    .map_err(|e| crate::error::anyhow!("tokio runtime: {}", e))?;
  let response = runtime.block_on(schema.execute(req));

  let data = if response.is_ok() || matches!(&response.data, async_graphql::Value::Object(_)) {
    Some(serde_json::to_value(&response.data).unwrap_or(serde_json::Value::Null))
  } else {
    None
  };

  let mut errors = Vec::with_capacity(response.errors.len());
  for e in &response.errors {
    let code = e
      .extensions
      .as_ref()
      .and_then(|ext| ext.get("code"))
      .and_then(|v| match v {
        async_graphql::Value::String(s) => Some(s.clone()),
        _ => None,
      });
    errors.push(QueryError {
      message: e.message.clone(),
      path: e
        .path
        .iter()
        .map(|seg| match seg {
          async_graphql::PathSegment::Field(s) => s.clone(),
          async_graphql::PathSegment::Index(i) => i.to_string(),
        })
        .collect(),
      locations: e
        .locations
        .iter()
        .map(|l| QueryErrorLocation {
          line: l.line as u32,
          column: l.column as u32,
        })
        .collect(),
      code,
    });
  }

  let mut warnings: Vec<crate::schema::Diagnostic> = errors
    .iter()
    .map(|e| {
      let code = e
        .code
        .clone()
        .unwrap_or_else(|| "GRAPHQL_ERROR".to_string());
      let mut msg = e.message.clone();
      if !e.path.is_empty() {
        msg.push_str(&format!(" (path: {})", e.path.join(".")));
      }
      crate::schema::Diagnostic::new(code, msg)
    })
    .collect();

  let progress = progress.lock().unwrap();
  let partial = progress.partial;
  warnings.extend(progress.diagnostics.clone());
  let resolver_statuses = exit_states.lock().unwrap().clone();
  let exit_status = if !errors.is_empty() {
    ExitStatus::Error
  } else if data.is_none() {
    ExitStatus::Error
  } else if !resolver_statuses.is_empty() {
    classify_from_resolver_states(&resolver_statuses)
  } else {
    infer_query_exit_status(data.as_ref().unwrap())
  };

  Ok(QueryResult {
    schema: QueryResult::SCHEMA.to_string(),
    data,
    errors,
    warnings,
    exit_status,
    partial,
  })
}

/// `lmprobe query '<gql>'` — execute a one-shot GraphQL query
/// against the in-process schema. Returns the standard
/// `(verb, status, warnings)` tuple so post-render diagnostics +
/// exit-code mapping in `run()` are uniform with every other verb.
fn dispatch_query(
  query: Option<String>,
  recipe: Option<String>,
  all_shapes: bool,
  vars: Vec<String>,
  vars_file: Option<PathBuf>,
  operation_name: Option<String>,
  infile: Option<PathBuf>,
  format: OutputFormat,
  workspace_ctx: Option<crate::workspace::WorkspaceCtx>,
) -> Result<(&'static str, ExitStatus, Vec<crate::schema::Diagnostic>)> {
  let variables = build_query_variables(vars, vars_file)?;
  if let Some(key) = recipe {
    let path = variables
      .get("path")
      .and_then(|value| value.as_str())
      .ok_or_else(|| crate::error::anyhow!("--recipe requires `--var path=<PATH>`"))?;
    return match execute_recipe_key(&key, Path::new(path), all_shapes, workspace_ctx)? {
      RecipeExecutionResult::Callsites(res) => {
        let warnings = res.warnings.clone();
        render_by_format(&res, format, "callsites", |r| {
          for callsite in &r.callsites {
            stdout_line(&format!(
              "{}:{}: {}",
              callsite.file_path, callsite.line_number, callsite.callee_text
            ));
          }
        });
        Ok(("callsites", res.exit_status, warnings))
      }
      RecipeExecutionResult::SourceReads(res) => {
        let warnings = res.warnings.clone();
        render_by_format(&res, format, "source_reads", |r| {
          for hit in &r.source_reads {
            stdout_line(&format!(
              "{}:{}: {} {}",
              hit.file_path, hit.line_number, hit.source_id, hit.read_text
            ));
          }
        });
        Ok(("source_reads", res.exit_status, warnings))
      }
    };
  }

  let document = if let Some(path) = infile {
    if path == PathBuf::from("-") {
      read_query_source("-")?
    } else {
      std::fs::read_to_string(&path)
        .map_err(|error| crate::error::anyhow!("read infile {}: {error}", path.display()))?
    }
  } else {
    let query = query.ok_or_else(|| crate::error::anyhow!("missing GraphQL query source"))?;
    read_query_source(&query)?
  };
  let envelope = execute_graphql_query_result(document, variables, operation_name, workspace_ctx)?;
  let exit_status = envelope.exit_status;
  let warnings = envelope.warnings.clone();

  match format {
    OutputFormat::Ndjson => {
      crate::output::render_envelope_ndjson(&envelope);
    }
    OutputFormat::Json => {
      crate::output::render_envelope_json(&envelope);
    }
    OutputFormat::Auto => {
      // Pipe → NDJSON; TTY → JSON pretty (not markdown).
      match format.resolve() {
        OutputFormat::Ndjson => crate::output::render_envelope_ndjson(&envelope),
        _ => crate::output::render_envelope_json(&envelope),
      }
    }
    OutputFormat::Markdown
    | OutputFormat::Sarif
    | OutputFormat::Github
    | OutputFormat::Mermaid
    | OutputFormat::Dot => {
      eprintln!(
        "lmprobe: query: --format {:?} not applicable to GraphQL responses; rendering as JSON",
        format
      );
      crate::output::render_envelope_json(&envelope);
    }
  }

  Ok(("query", exit_status, warnings))
}

/// Walk a GraphQL response's `data` and infer the verb-level exit
/// status. Per arbitration I, callers map this to a process exit
/// code. Logic:
///   - If any per-verb response carries a selected `exitStatus`
///     field, use it (trust the verb's own envelope).
///   - Otherwise, infer from arrays: a verb whose only arrays are
///     all empty is treated as `NoMatch`; any non-empty array
///     means at least one hit and the verb is `Matched`.
///   - If multiple verbs were aliased in one query, take the
///     "best": any `Matched` → `Matched`; else `NoMatch`. Errors
///     are handled at the top level (errors[] != []).
/// QA TC-G17 — selection-set-independent exit-status classification.
///
/// Each Query resolver writes its envelope's `exit_status` into a
/// shared Vec via async-graphql request-scoped data. After execute()
/// returns, dispatch_query reads this list and classifies based on
/// the resolver state, NOT the projected response tree.
///
/// This fixes the bug where a client query that selects only `hits`
/// (no `exitStatus`/`warnings`) gets misclassified as NoMatch when
/// the resolver's NOT_IMPLEMENTED-promotion produced Matched. The
/// projection filter strips `exitStatus`/`warnings` from the
/// response.data tree, so the prior tree-walk classifier (kept as
/// `infer_query_exit_status` for the no-resolver-ran fallback) saw
/// only an empty `hits` array and returned NoMatch.
pub(crate) type QueryExitStates = std::sync::Arc<std::sync::Mutex<Vec<ExitStatus>>>;
#[derive(Default)]
struct QueryProgress {
  partial: bool,
  diagnostics: Vec<crate::schema::Diagnostic>,
}
type QueryProgressState = std::sync::Arc<std::sync::Mutex<QueryProgress>>;
fn record_query_diagnostics(
  ctx: &async_graphql::Context<'_>,
  warnings: &[crate::schema::Diagnostic],
  partial: bool,
) {
  if let Some(state) = ctx.data_opt::<QueryProgressState>() {
    let mut state = state.lock().unwrap();
    state.partial |= partial || has_primary_path_partial_coverage(warnings);
    state.diagnostics.extend_from_slice(warnings);
  }
}

fn record_query_status(ctx: &async_graphql::Context<'_>, status: ExitStatus) {
  if let Some(states) = ctx.data_opt::<QueryExitStates>() {
    states.lock().unwrap().push(status);
  }
}

fn classify_from_resolver_states(states: &[ExitStatus]) -> ExitStatus {
  if states.iter().any(|s| matches!(s, ExitStatus::Error)) {
    ExitStatus::Error
  } else if states.iter().any(|s| matches!(s, ExitStatus::Matched)) {
    ExitStatus::Matched
  } else {
    ExitStatus::NoMatch
  }
}

fn infer_query_exit_status(data: &serde_json::Value) -> ExitStatus {
  let obj = match data.as_object() {
    Some(o) => o,
    None => return ExitStatus::Matched,
  };
  if obj.is_empty() {
    return ExitStatus::Matched;
  }
  let mut any_matched = false;
  let mut any_no_match = false;
  for (_, v) in obj {
    match infer_verb_exit_status(v) {
      ExitStatus::Matched => any_matched = true,
      ExitStatus::NoMatch => any_no_match = true,
      ExitStatus::Error => return ExitStatus::Error,
    }
  }
  if any_matched {
    ExitStatus::Matched
  } else if any_no_match {
    ExitStatus::NoMatch
  } else {
    ExitStatus::Matched
  }
}

fn infer_verb_exit_status(v: &serde_json::Value) -> ExitStatus {
  let obj = match v.as_object() {
    Some(o) => o,
    None => return ExitStatus::Matched,
  };
  // Trust an explicitly-selected exitStatus if present (camelCase
  // is what async-graphql emits; snake_case is what the CLI's
  // serde envelopes use — accept either).
  if let Some(s) = obj
    .get("exitStatus")
    .or_else(|| obj.get("exit_status"))
    .and_then(|s| s.as_str())
  {
    return match s.to_ascii_lowercase().as_str() {
      "matched" => ExitStatus::Matched,
      "no_match" => ExitStatus::NoMatch,
      "error" => ExitStatus::Error,
      _ => ExitStatus::Matched,
    };
  }
  // Derive from arrays: if every array under this verb is empty,
  // treat as NoMatch.
  let mut found_array = false;
  let mut any_non_empty = false;
  for (_, fv) in obj {
    if let serde_json::Value::Array(a) = fv {
      found_array = true;
      if !a.is_empty() {
        any_non_empty = true;
        break;
      }
    }
  }
  if !found_array || any_non_empty {
    ExitStatus::Matched
  } else {
    ExitStatus::NoMatch
  }
}

// ─────────────────────────── GraphQL ──────────────────────────────
//
// Phase 4E: GraphQL is the core query surface. The Query type and its
// resolvers are unconditional; the `query` subcommand executes them
// in-process with a current-thread tokio runtime. The HTTP daemon
// (axum + multi-thread tokio) stays opt-in behind `--features serve`.
//
// Every resolver returns the **envelope** (with `warnings` and
// `exit_status`) — schema-first, single source of truth.
//
// `trace` is intentionally excluded (mutation surface lands in Phase 4E
// follow-up chunks per the design correction doc).

#[cfg(feature = "serve")]
fn serve_graphql(addr: String, auth_token: Option<String>) -> Result<()> {
  if !is_loopback_addr(&addr) && auth_token.is_none() {
    return Err(crate::error::anyhow!(
      "remote bind `{}` requires `--auth-token <TOKEN>` (loopback binds skip auth)",
      addr
    ));
  }
  // The HTTP daemon doesn't take a `--workspace` flag yet; the
  // CLI surface is single-root for the long-running server. If
  // workspace support lands on `serve` later, thread it through
  // here the same way `dispatch_query` does.
  let schema = build_schema(None);
  let auth_state = AuthState { token: auth_token };
  let app = Router::new()
    .route("/graphql", post(graphql_handler))
    .layer(axum::middleware::from_fn_with_state(
      auth_state,
      require_bearer_token,
    ))
    .with_state(schema);
  stdout_line(&format!(
    "lmprobe GraphQL server running at http://{}/graphql",
    addr
  ));
  let rt = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()
    .map_err(|e| crate::error::anyhow!("tokio runtime: {}", e))?;
  rt.block_on(async {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok::<(), anyhow::Error>(())
  })?;
  Ok(())
}

// `Query` is the GraphQL root. Wired into the in-process `query`
// subcommand (default build) and the HTTP daemon (`--features serve`).
pub(crate) struct Query;

fn gql_walk_opts() -> WalkOpts {
  WalkOpts::default()
}

fn gql_walk_opts_with(no_default_excludes: Option<bool>, quick_projects: Option<bool>) -> WalkOpts {
  WalkOpts {
    no_default_excludes: no_default_excludes.unwrap_or(false),
    quick_project_roots: quick_projects.unwrap_or(false),
    ..WalkOpts::default()
  }
}

fn gql_read_roots(path: String, paths: Option<Vec<String>>) -> Vec<PathBuf> {
  let mut roots = Vec::new();
  roots.push(PathBuf::from(path));
  roots.extend(paths.unwrap_or_default().into_iter().map(PathBuf::from));
  roots
}

fn gql_missing_path_diagnostics(roots: &[PathBuf]) -> Vec<ResultDiagnostic> {
  explicit_missing_read_path_diagnostics(None, roots)
}

fn marker_parent_roots(marker_paths: &[String]) -> Vec<PathBuf> {
  let mut roots = marker_paths
    .iter()
    .filter_map(|path| {
      Path::new(path).parent().map(|parent| {
        if parent.as_os_str().is_empty() {
          PathBuf::from(".")
        } else {
          parent.to_path_buf()
        }
      })
    })
    .collect::<Vec<_>>();
  roots.sort_by(|a, b| {
    a.components()
      .count()
      .cmp(&b.components().count())
      .then_with(|| a.cmp(b))
  });
  let mut pruned = Vec::new();
  for root in roots {
    if !pruned
      .iter()
      .any(|parent: &PathBuf| root.starts_with(parent))
    {
      pruned.push(root);
    }
  }
  pruned
}

fn attach_gql_grep_expansions(
  res: &mut crate::schema::GrepResultEnvelope,
  pattern: &str,
  around: Option<usize>,
  parents: Option<bool>,
  body: Option<bool>,
  role: Option<bool>,
) {
  let walk_opts = gql_walk_opts();
  let parents = parents.unwrap_or(false);
  let body = body.unwrap_or(false);
  if parents || body {
    sensors::attach_grep_structural_expansions(&mut res.hits, &walk_opts, parents, body);
  }
  if role.unwrap_or(false) {
    sensors::attach_grep_roles(&mut res.hits, pattern, &walk_opts);
  }
  if let Some(around) = around {
    attach_grep_context(
      &mut res.hits,
      ContextLines {
        before: around,
        after: around,
      },
    );
  }
}

struct ArgRedactionPolicy {
  credential_names: Vec<String>,
  secret_keywords: Vec<String>,
}

fn default_arg_redaction_policy() -> ArgRedactionPolicy {
  let mut credential_names = vec![
    "secret".to_string(),
    "token".to_string(),
    "password".to_string(),
    "key".to_string(),
    "auth".to_string(),
    "credential".to_string(),
    "apikey".to_string(),
  ];
  if let Ok(db) = lmprobe_secrets::load_default_provider_db() {
    for shape in db
      .providers
      .iter()
      .flat_map(|provider| &provider.languages)
      .flat_map(|language| &language.call_shapes)
    {
      credential_names.extend(shape.kwargs.iter().cloned());
      credential_names.extend(shape.object_keys.iter().cloned());
      credential_names.extend(shape.object_paths.iter().cloned());
      credential_names.extend(shape.struct_fields.iter().cloned());
      if let Some(target) = &shape.assignment_target {
        credential_names.push(target.clone());
      }
    }
  }
  credential_names.sort();
  credential_names.dedup();

  let secret_keywords =
    lmprobe_secrets::load_gitleaks_config_str(lmprobe_secrets::DEFAULT_GITLEAKS_CONFIG)
      .map(|config| config.keywords.into_iter().collect::<Vec<_>>())
      .unwrap_or_default();

  ArgRedactionPolicy {
    credential_names,
    secret_keywords,
  }
}

fn redact_callsite_arg_values(
  mut res: crate::schema::CallsitesResult,
) -> crate::schema::CallsitesResult {
  let policy = default_arg_redaction_policy();
  let mut any_redacted = false;
  for hit in &mut res.callsites {
    for value in &mut hit.positional_args {
      if should_redact_arg(None, value, &policy) {
        *value = "REDACTED".to_string();
        any_redacted = true;
      }
    }
    for arg in &mut hit.keyword_args {
      if should_redact_arg(Some(&arg.name), &arg.value, &policy) {
        arg.value = "REDACTED".to_string();
        any_redacted = true;
      }
    }
    for arg in &mut hit.structured_args {
      if should_redact_arg(arg.name.as_deref(), &arg.text, &policy) {
        arg.text = "REDACTED".to_string();
        any_redacted = true;
      }
    }
  }
  res.arg_values_redacted = any_redacted;
  res
}

fn should_redact_arg(name: Option<&str>, value: &str, policy: &ArgRedactionPolicy) -> bool {
  if let Some(name) = name {
    let lower_name = name.to_ascii_lowercase();
    if policy
      .credential_names
      .iter()
      .any(|candidate| lower_name.contains(&candidate.to_ascii_lowercase()))
    {
      return true;
    }
  }

  let unquoted = value
    .trim()
    .trim_matches('"')
    .trim_matches('\'')
    .trim_matches('`');
  if unquoted.len() >= 10 && lmprobe_secrets::shannon_entropy(unquoted) >= 3.5 {
    return true;
  }

  let lower_value = unquoted.to_ascii_lowercase();
  let generic_terms = [
    "secret",
    "token",
    "password",
    "key",
    "auth",
    "credential",
    "apikey",
  ];
  policy
    .secret_keywords
    .iter()
    .filter(|keyword| keyword.len() >= 3)
    .filter(|keyword| !generic_terms.contains(&keyword.as_str()))
    .any(|keyword| lower_value.contains(keyword))
}

// Phase 4E §5.1 composition: child resolvers on `SymbolHit` live
// in `lmprobe-types` (orphan rule: ComplexObject impl must be in
// the same crate as the SimpleObject derive). The actual sensor
// work delegates to a `SymbolHitChildResolvers` impl registered
// as schema `Context` data — see `child_resolvers_impl` below
// + `build_schema()`.

#[Object]
impl Query {
  async fn discover(
    &self,
    ctx: &async_graphql::Context<'_>,
    path: String,
    top_level_only: Option<bool>,
    no_default_excludes: Option<bool>,
  ) -> std::result::Result<crate::schema::DiscoverResult, GqlError> {
    let res = sensors::discover(
      std::path::Path::new(&path),
      &gql_walk_opts_with(no_default_excludes, None),
      None,
      top_level_only.unwrap_or(false),
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn find(
    &self,
    ctx: &async_graphql::Context<'_>,
    pattern: String,
    path: String,
    paths: Option<Vec<String>>,
    no_default_excludes: Option<bool>,
    quick_projects: Option<bool>,
    older_than: Option<String>,
    filter: Option<crate::schema::FilePredicateInput>,
    prune: Option<crate::schema::FilePredicateInput>,
  ) -> std::result::Result<crate::schema::FindResult, GqlError> {
    let roots = gql_read_roots(path, paths);
    let missing_path_diags = gql_missing_path_diagnostics(&roots);
    if !missing_path_diags.is_empty() {
      let mut res = crate::schema::FindResult::new(Vec::new());
      res.exit_status = ExitStatus::Error;
      for d in missing_path_diags {
        res.push_warning(d);
      }
      record_query_status(ctx, res.exit_status);
      record_query_diagnostics(ctx, &res.warnings, res.partial);
      return Ok(res);
    }
    let age = older_than
      .as_deref()
      .map(file_actions::parse_age)
      .transpose()
      .map_err(GqlError::new)?;
    let res = sensors::find_many_with_predicates(
      &pattern,
      &roots,
      &gql_walk_opts_with(no_default_excludes, quick_projects),
      age,
      filter.as_ref(),
      prune.as_ref(),
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, res.partial);
    Ok(res)
  }
  /// Grep all directories containing a marker matched by `markerPattern`.
  ///
  /// This is the GraphQL-side internal pipe for:
  /// find marker files under `path` -> take their parent dirs -> grep content.
  async fn grep_dirs_with(
    &self,
    ctx: &async_graphql::Context<'_>,
    marker_pattern: String,
    pattern: String,
    path: String,
    paths: Option<Vec<String>>,
    around: Option<usize>,
    parents: Option<bool>,
    body: Option<bool>,
    role: Option<bool>,
    no_default_excludes: Option<bool>,
  ) -> std::result::Result<crate::schema::GrepResultEnvelope, GqlError> {
    let walk_opts = gql_walk_opts_with(no_default_excludes, None);
    let marker_roots = gql_read_roots(path, paths);
    let missing_path_diags = gql_missing_path_diagnostics(&marker_roots);
    if !missing_path_diags.is_empty() {
      let mut res = crate::schema::GrepResultEnvelope::new(Vec::new());
      res.exit_status = ExitStatus::Error;
      for d in missing_path_diags {
        res.push_warning(d);
      }
      record_query_status(ctx, res.exit_status);
      record_query_diagnostics(ctx, &res.warnings, false);
      return Ok(res);
    }
    let marker_matches = sensors::find_many(&marker_pattern, &marker_roots, &walk_opts)
      .map_err(|e| GqlError::new(e.to_string()))?;
    if marker_matches.exit_status == ExitStatus::Error {
      let mut res = crate::schema::GrepResultEnvelope::new(Vec::new());
      res.exit_status = ExitStatus::Error;
      for warning in marker_matches.warnings {
        res.push_warning(warning);
      }
      record_query_status(ctx, res.exit_status);
      record_query_diagnostics(ctx, &res.warnings, false);
      return Ok(res);
    }

    let roots = marker_parent_roots(&marker_matches.paths);
    let mut res = if roots.is_empty() {
      crate::schema::GrepResultEnvelope::new(Vec::new())
    } else {
      sensors::grep_many(&pattern, &roots, &walk_opts).map_err(|e| GqlError::new(e.to_string()))?
    };
    for warning in marker_matches.warnings {
      res.push_warning(warning);
    }
    attach_gql_grep_expansions(&mut res, &pattern, around, parents, body, role);
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn grep(
    &self,
    ctx: &async_graphql::Context<'_>,
    pattern: String,
    path: String,
    paths: Option<Vec<String>>,
    around: Option<usize>,
    parents: Option<bool>,
    body: Option<bool>,
    role: Option<bool>,
    no_default_excludes: Option<bool>,
    quick_projects: Option<bool>,
  ) -> std::result::Result<crate::schema::GrepResultEnvelope, GqlError> {
    let roots = gql_read_roots(path, paths);
    let missing_path_diags = gql_missing_path_diagnostics(&roots);
    if !missing_path_diags.is_empty() {
      let mut res = crate::schema::GrepResultEnvelope::new(Vec::new());
      res.exit_status = ExitStatus::Error;
      for d in missing_path_diags {
        res.push_warning(d);
      }
      record_query_status(ctx, res.exit_status);
      record_query_diagnostics(ctx, &res.warnings, res.partial);
      return Ok(res);
    }
    let mut res = sensors::grep_many(
      &pattern,
      &roots,
      &gql_walk_opts_with(no_default_excludes, quick_projects),
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    attach_gql_grep_expansions(&mut res, &pattern, around, parents, body, role);
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, res.partial);
    Ok(res)
  }
  async fn callsites(
    &self,
    ctx: &async_graphql::Context<'_>,
    path: String,
    callee: Option<String>,
    language: crate::schema::CallsiteLanguage,
    filters: Option<Vec<crate::schema::CallsiteFilterInput>>,
    redact_arg_values: Option<bool>,
    around: Option<usize>,
  ) -> std::result::Result<crate::schema::CallsitesResult, GqlError> {
    let filters = filters.unwrap_or_default();
    let mut res = sensors::callsites_filtered(
      callee.as_deref(),
      std::path::Path::new(&path),
      Some(language),
      &filters,
      around,
      &gql_walk_opts(),
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    if redact_arg_values.unwrap_or(true) {
      res = redact_callsite_arg_values(res);
    }
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn source_reads(
    &self,
    ctx: &async_graphql::Context<'_>,
    path: String,
    language: crate::schema::SourceReadLanguage,
    filters: Option<Vec<crate::schema::SourceReadFilterInput>>,
    redact_arg_values: Option<bool>,
  ) -> std::result::Result<crate::schema::SourceReadsResult, GqlError> {
    let filters = filters.unwrap_or_default();
    let res = sensors::source_reads(sensors::SourceReadsInput {
      root: std::path::Path::new(&path),
      language,
      filters: &filters,
      redact_arg_values: redact_arg_values.unwrap_or(true),
      opts: &gql_walk_opts(),
    })
    .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn secrets(
    &self,
    ctx: &async_graphql::Context<'_>,
    path: String,
    source: Option<crate::schema::SecretsSourceInput>,
    gitleaks_compat: Option<bool>,
    config: Option<String>,
    ast_fp_cut: Option<crate::schema::SecretsAstFpCutMode>,
    sink_evidence: Option<crate::schema::SecretsSinkEvidenceMode>,
    baseline: Option<String>,
    max_target_megabytes: Option<i32>,
    follow_symlinks: Option<bool>,
  ) -> std::result::Result<crate::schema::SecretsResult, GqlError> {
    let scan_opts = crate::secrets::SecretsScanOptions {
      config_path: config.map(std::path::PathBuf::from),
      max_target_megabytes: max_target_megabytes
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(5),
      follow_symlinks: follow_symlinks.unwrap_or(false),
      gitleaks_compat: gitleaks_compat.unwrap_or(false),
      ast_fp_cut: match ast_fp_cut.unwrap_or_default() {
        crate::schema::SecretsAstFpCutMode::Off => lmprobe_secrets::AstFpCutMode::Off,
        crate::schema::SecretsAstFpCutMode::Annotate => lmprobe_secrets::AstFpCutMode::Annotate,
        crate::schema::SecretsAstFpCutMode::Suppress => lmprobe_secrets::AstFpCutMode::Suppress,
      },
      sink_evidence: match sink_evidence.unwrap_or_default() {
        crate::schema::SecretsSinkEvidenceMode::Off => lmprobe_secrets::SinkEvidenceMode::Off,
        crate::schema::SecretsSinkEvidenceMode::On => lmprobe_secrets::SinkEvidenceMode::On,
      },
      source_mode: match source.unwrap_or_default() {
        crate::schema::SecretsSourceInput::Filesystem => {
          crate::secrets::SecretsSourceMode::Filesystem
        }
        crate::schema::SecretsSourceInput::Git => crate::secrets::SecretsSourceMode::Git,
      },
      baseline_path: baseline.map(std::path::PathBuf::from),
    };
    let res =
      crate::secrets::scan_filesystem(std::path::Path::new(&path), &gql_walk_opts(), &scan_opts)
        .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, res.partial);
    Ok(res)
  }
  async fn search(
    &self,
    ctx: &async_graphql::Context<'_>,
    query: Option<String>,
    path: String,
    outline: Option<bool>,
    has_decoration: Option<String>,
    has_child: Option<String>,
    has_attr: Option<String>,
    kind: Option<String>,
    has: Option<String>,
    inside: Option<String>,
    precedes: Option<String>,
    follows: Option<String>,
    not: Option<String>,
  ) -> std::result::Result<crate::schema::SearchResult, GqlError> {
    let composite = compose_relational_yaml(
      has.as_deref(),
      inside.as_deref(),
      precedes.as_deref(),
      follows.as_deref(),
      not.as_deref(),
      &[],
      &[],
      None,
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    let constraints = SearchConstraints {
      query,
      has_decoration,
      has_child,
      has_attr,
      kind,
      has_rule: composite,
      ..Default::default()
    };
    let res = sensors::search(
      std::path::Path::new(&path),
      outline.unwrap_or(false),
      constraints,
      &gql_walk_opts(),
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn def(
    &self,
    ctx: &async_graphql::Context<'_>,
    name: String,
    path: String,
    regex: Option<bool>,
    class_name: Option<String>,
    scope_code: Option<bool>,
    body: Option<bool>,
    around: Option<usize>,
  ) -> std::result::Result<crate::schema::DefResult, GqlError> {
    let mut res = sensors::def(
      name,
      std::path::Path::new(&path),
      regex.unwrap_or(false),
      class_name,
      scope_code.unwrap_or(true),
      false,
      body.unwrap_or(false),
      &gql_walk_opts(),
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    attach_symbol_source_context(&mut res.hits, around);
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn refs(
    &self,
    ctx: &async_graphql::Context<'_>,
    name: String,
    path: String,
    scope_code: Option<bool>,
    include_comments: Option<bool>,
    include_docs: Option<bool>,
    role: Option<bool>,
  ) -> std::result::Result<crate::schema::RefResult, GqlError> {
    let mut res = sensors::refs(
      &name,
      std::path::Path::new(&path),
      scope_code.unwrap_or(true),
      include_comments.unwrap_or(false),
      include_docs.unwrap_or(false),
      &gql_walk_opts(),
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    if role.unwrap_or(false) {
      sensors::attach_grep_roles_for_literal(&mut res.hits, &name, &gql_walk_opts());
    }
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn callers(
    &self,
    ctx: &async_graphql::Context<'_>,
    name: String,
    path: String,
  ) -> std::result::Result<crate::schema::CallersResult, GqlError> {
    let res = sensors::callers(&name, std::path::Path::new(&path), &gql_walk_opts())
      .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn imports(
    &self,
    ctx: &async_graphql::Context<'_>,
    name: String,
    path: String,
  ) -> std::result::Result<crate::schema::ImportsResult, GqlError> {
    let res = sensors::imports(&name, std::path::Path::new(&path), &gql_walk_opts())
      .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn decorations(
    &self,
    ctx: &async_graphql::Context<'_>,
    name: String,
    path: String,
  ) -> std::result::Result<crate::schema::DecorationsResult, GqlError> {
    let res = sensors::decorations(&name, std::path::Path::new(&path), &gql_walk_opts())
      .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn evolution(
    &self,
    ctx: &async_graphql::Context<'_>,
    name: String,
    path: String,
    force: Option<bool>,
  ) -> std::result::Result<crate::schema::EvolutionResult, GqlError> {
    let res = sensors::evolution_with_regex(
      &name,
      std::path::Path::new(&path),
      force.unwrap_or(false),
      false,
      false,
      &gql_walk_opts(),
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  /// `list-kinds` — enumerate the portable-kind taxonomy for a
  /// given language.
  async fn list_kinds(
    &self,
    ctx: &async_graphql::Context<'_>,
    lang: crate::gql_scalars::Language,
  ) -> std::result::Result<crate::schema::ListKindsResult, GqlError> {
    let sg: SgLang = lang.into();
    let tx = taxonomy_for(sg);
    let rows: Vec<crate::schema::PortableKindRow> = PortableKind::ALL
      .iter()
      .map(|p| crate::schema::PortableKindRow {
        portable: p.name().to_string(),
        kinds: tx.kind_for(*p).iter().map(|s| s.to_string()).collect(),
      })
      .collect();
    let res = crate::schema::ListKindsResult::new(rows);
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  /// `callees` — symbols invoked from inside `name`'s body.
  async fn callees(
    &self,
    ctx: &async_graphql::Context<'_>,
    name: String,
    path: String,
  ) -> std::result::Result<crate::schema::CalleesResult, GqlError> {
    let res = crate::graph_sensors::callees(&name, std::path::Path::new(&path), &gql_walk_opts())
      .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  /// `supertypes` — superclasses / parent traits of `name`.
  async fn supertypes(
    &self,
    ctx: &async_graphql::Context<'_>,
    name: String,
    path: String,
  ) -> std::result::Result<crate::schema::SupertypesResult, GqlError> {
    let res =
      crate::graph_sensors::supertypes(&name, std::path::Path::new(&path), &gql_walk_opts())
        .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  /// `subtypes` — subclasses / impls / extends of `name`.
  async fn subtypes(
    &self,
    ctx: &async_graphql::Context<'_>,
    name: String,
    path: String,
  ) -> std::result::Result<crate::schema::SubtypesResult, GqlError> {
    let res = crate::graph_sensors::subtypes(&name, std::path::Path::new(&path), &gql_walk_opts())
      .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  /// `overrides` — methods that override `name` in subclasses.
  async fn overrides(
    &self,
    ctx: &async_graphql::Context<'_>,
    name: String,
    path: String,
  ) -> std::result::Result<crate::schema::OverridesResult, GqlError> {
    let res = crate::graph_sensors::overrides(&name, std::path::Path::new(&path), &gql_walk_opts())
      .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  /// `exports` — files / modules that export `name`.
  async fn exports(
    &self,
    ctx: &async_graphql::Context<'_>,
    name: String,
    path: String,
  ) -> std::result::Result<crate::schema::ExportsResult, GqlError> {
    let res = crate::graph_sensors::exports(&name, std::path::Path::new(&path), &gql_walk_opts())
      .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  /// `impact` — BFS expansion in `direction` (upstream / downstream)
  /// up to `depth` (default 2). The `IMPACT_DEPTH_CAPPED` warning
  /// fires when `depth` exceeds the hard cap of 10 (clamped).
  async fn impact(
    &self,
    ctx: &async_graphql::Context<'_>,
    symbol: String,
    path: String,
    direction: Option<lmprobe_engine::impact::ImpactDirection>,
    depth: Option<u32>,
    tests_only: Option<bool>,
    exclude_tests: Option<bool>,
  ) -> std::result::Result<crate::schema::ImpactResult, GqlError> {
    let tests_only = tests_only.unwrap_or(false);
    let exclude_tests = exclude_tests.unwrap_or(false);
    if tests_only && exclude_tests {
      return Err(GqlError::new(
        "impact testsOnly and excludeTests are mutually exclusive",
      ));
    }
    let res = crate::impact::impact(
      &symbol,
      std::path::Path::new(&path),
      direction.unwrap_or(lmprobe_engine::impact::ImpactDirection::Upstream),
      depth.unwrap_or(2),
      impact_file_filter(tests_only, exclude_tests),
      &gql_walk_opts(),
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  /// `context` — composite bundle: definition + callers (upstream
  /// to `depth`) + recent history. Emits `SYMBOL_NOT_FOUND` warning
  /// when `def` returns nothing; the bundle is then partial.
  async fn context(
    &self,
    ctx: &async_graphql::Context<'_>,
    symbol: String,
    path: String,
    depth: Option<u32>,
    import_aware: Option<bool>,
    usage: Option<bool>,
    around: Option<usize>,
    parents: Option<bool>,
    body: Option<bool>,
  ) -> std::result::Result<crate::schema::ContextResult, GqlError> {
    let mode = if usage.unwrap_or(false) {
      crate::context_bundle::ContextAnchorMode::Usage
    } else {
      crate::context_bundle::ContextAnchorMode::Def
    };
    let res = crate::context_bundle::context_with_options(
      &symbol,
      std::path::Path::new(&path),
      depth.unwrap_or(1),
      &gql_walk_opts(),
      // GraphQL clients use the existing
      // `SymbolHit::recentHistory(limit:N)` resolver arg for
      // per-sub-list control; the bundle keeps default-3.
      None,
      import_aware.unwrap_or(false),
      around,
      mode,
      parents.unwrap_or(false),
      body.unwrap_or(false),
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  /// `ast-diff` — AST-elided diff of `file` between two git revs.
  /// Per arbitration A: the typed scalars stay as `String` here
  /// (revs may be tags / branches / partial SHAs); CommitSha is
  /// reserved for resolvers that need a strict 40-hex contract.
  async fn ast_diff(
    &self,
    ctx: &async_graphql::Context<'_>,
    file: String,
    from: String,
    to: String,
    path: Option<String>,
  ) -> std::result::Result<crate::schema::AstDiffResult, GqlError> {
    let root = path.as_deref().unwrap_or(".");
    let res = crate::ast_diff_sensor::ast_diff(
      std::path::Path::new(&file),
      &from,
      &to,
      std::path::Path::new(root),
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn resource_sites(
    &self,
    ctx: &async_graphql::Context<'_>,
    path: String,
    acquire: String,
    release: String,
    around: Option<usize>,
    parents: Option<bool>,
    body: Option<bool>,
  ) -> std::result::Result<crate::schema::ResourceSitesResult, GqlError> {
    let res = crate::resource_sites::resource_sites(
      std::path::Path::new(&path),
      &acquire,
      &release,
      around,
      parents.unwrap_or(false),
      body.unwrap_or(false),
      &gql_walk_opts(),
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn async_leaks(
    &self,
    ctx: &async_graphql::Context<'_>,
    path: String,
    call: Vec<String>,
    around: Option<usize>,
    parents: Option<bool>,
    body: Option<bool>,
  ) -> std::result::Result<crate::schema::AsyncLeaksResult, GqlError> {
    let res = crate::async_leaks::async_leaks(
      std::path::Path::new(&path),
      &call,
      around,
      parents.unwrap_or(false),
      body.unwrap_or(false),
      &gql_walk_opts(),
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn crypto_sites(
    &self,
    ctx: &async_graphql::Context<'_>,
    path: String,
    algorithms: Vec<String>,
    fips_check: Option<bool>,
    around: Option<usize>,
    parents: Option<bool>,
    body: Option<bool>,
  ) -> std::result::Result<crate::schema::CryptoSitesResult, GqlError> {
    let res = crate::crypto_sites::crypto_sites(
      std::path::Path::new(&path),
      &algorithms,
      fips_check.unwrap_or(false),
      around,
      parents.unwrap_or(false),
      body.unwrap_or(false),
      &gql_walk_opts(),
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn config_keys(
    &self,
    ctx: &async_graphql::Context<'_>,
    path: String,
    key: Option<String>,
    grep_code: Option<bool>,
    around: Option<usize>,
    parents: Option<bool>,
    role: Option<bool>,
  ) -> std::result::Result<crate::schema::ConfigKeysResult, GqlError> {
    let paths = vec![std::path::PathBuf::from(path)];
    let res = crate::config_keys::config_keys(
      &paths,
      key.as_deref(),
      grep_code.unwrap_or(false),
      around,
      parents.unwrap_or(false),
      role.unwrap_or(false),
      &gql_walk_opts(),
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn guard_sites(
    &self,
    ctx: &async_graphql::Context<'_>,
    path: String,
    sensitive: String,
    guard: String,
    around: Option<usize>,
    parents: Option<bool>,
    body: Option<bool>,
  ) -> std::result::Result<crate::schema::GuardSitesResult, GqlError> {
    let res = crate::guard_sites::guard_sites(
      std::path::Path::new(&path),
      &sensitive,
      &guard,
      around,
      parents.unwrap_or(false),
      body.unwrap_or(false),
      &gql_walk_opts(),
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn openapi_facts(
    &self,
    ctx: &async_graphql::Context<'_>,
    path: String,
  ) -> std::result::Result<crate::schema::OpenApiFactsResult, GqlError> {
    let res = crate::api_facts::openapi_facts(std::path::Path::new(&path), &gql_walk_opts())
      .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn route_anchors(
    &self,
    ctx: &async_graphql::Context<'_>,
    path: String,
    framework: Option<String>,
    around: Option<usize>,
    parents: Option<bool>,
    body: Option<bool>,
  ) -> std::result::Result<crate::schema::RouteAnchorsResult, GqlError> {
    let res = crate::api_facts::route_anchors(
      std::path::Path::new(&path),
      framework.as_deref(),
      around,
      parents.unwrap_or(false),
      body.unwrap_or(false),
      &gql_walk_opts(),
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  async fn http_client_calls(
    &self,
    ctx: &async_graphql::Context<'_>,
    path: String,
    around: Option<usize>,
    parents: Option<bool>,
    body: Option<bool>,
  ) -> std::result::Result<crate::schema::HttpClientCallsResult, GqlError> {
    let res = crate::api_facts::http_client_calls(
      std::path::Path::new(&path),
      around,
      parents.unwrap_or(false),
      body.unwrap_or(false),
      &gql_walk_opts(),
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  /// `history` — nested resolver returning the 7 history sub-verbs.
  /// Mirrors the CLI's `lmprobe history <subcommand>` surface so
  /// `query` can navigate to `history { blame { … } }` in one
  /// round-trip.
  async fn history(&self) -> HistoryQueries {
    HistoryQueries
  }
  // `trace` and `fix` are intentionally not exposed via GraphQL
  // until the Phase 4E mutation surface lands.
}

/// Nested type for the 7 `lmprobe history <verb>` sub-resolvers.
/// Held as a unit struct since the resolvers themselves take a
/// `path` argument and don't share resolver-instance state.
pub(crate) struct HistoryQueries;

#[Object]
impl HistoryQueries {
  /// `history blame <target>` — per-symbol or per-file commit blame.
  /// `follow` traces cross-file renames; `semantic` annotates each
  /// entry with `INTRODUCED | LOGIC_CHANGE | FORMATTING | RENAMED`.
  async fn blame(
    &self,
    ctx: &async_graphql::Context<'_>,
    target: String,
    path: Option<String>,
    follow: Option<bool>,
    semantic: Option<bool>,
  ) -> std::result::Result<crate::schema::BlameResult, GqlError> {
    let root = path.as_deref().unwrap_or(".");
    let resolved = crate::history_sensors::resolve_root(std::path::Path::new(root));
    let res = crate::history_sensors::blame_with_follow_semantic(
      &target,
      &resolved,
      follow.unwrap_or(false),
      semantic.unwrap_or(false),
      false,
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  /// `history introduced <name>` — earliest-introducing commit(s)
  /// per the same blame backbone, filtered to `INTRODUCED` rows.
  async fn introduced(
    &self,
    ctx: &async_graphql::Context<'_>,
    name: String,
    path: Option<String>,
    follow: Option<bool>,
  ) -> std::result::Result<crate::schema::IntroducedResult, GqlError> {
    let root = path.as_deref().unwrap_or(".");
    let resolved = crate::history_sensors::resolve_root(std::path::Path::new(root));
    let res = crate::history_sensors::introduced_with_follow(
      &name,
      &resolved,
      None,
      follow.unwrap_or(false),
      false,
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  /// `history commit [-p <pat>] [--since X] [--author A]`.
  async fn commit(
    &self,
    ctx: &async_graphql::Context<'_>,
    pattern: Option<String>,
    since: Option<String>,
    author: Option<String>,
    path: Option<String>,
  ) -> std::result::Result<crate::schema::HistoryCommitResult, GqlError> {
    let root = path.as_deref().unwrap_or(".");
    let resolved = crate::history_sensors::resolve_root(std::path::Path::new(root));
    let res = crate::history_sensors::history_commit(
      pattern.as_deref(),
      since.as_deref(),
      author.as_deref(),
      &resolved,
    )
    .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  /// `history diff -p <pattern> [--since X]`.
  async fn diff(
    &self,
    ctx: &async_graphql::Context<'_>,
    pattern: String,
    since: Option<String>,
    path: Option<String>,
  ) -> std::result::Result<crate::schema::HistoryDiffResult, GqlError> {
    let root = path.as_deref().unwrap_or(".");
    let resolved = crate::history_sensors::resolve_root(std::path::Path::new(root));
    let res = crate::history_sensors::history_diff(&pattern, since.as_deref(), &resolved)
      .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  /// `history churn [--top N] [--since X]`.
  async fn churn(
    &self,
    ctx: &async_graphql::Context<'_>,
    top: Option<u32>,
    since: Option<String>,
    path: Option<String>,
  ) -> std::result::Result<crate::schema::HistoryChurnResult, GqlError> {
    let root = path.as_deref().unwrap_or(".");
    let resolved = crate::history_sensors::resolve_root(std::path::Path::new(root));
    let res =
      crate::history_sensors::churn(top.unwrap_or(20) as usize, since.as_deref(), &resolved)
        .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  /// `history co-change <file> [--since X] [--top N] [--min-score X]`.
  /// Output-only filters mirror CLI `--exclude-glob` and `--type` as
  /// `excludeGlobs` and `fileType`; `HISTORY_TRUNCATED` fires in
  /// `warnings[]` when the 1000-commit cap is hit.
  async fn co_change(
    &self,
    ctx: &async_graphql::Context<'_>,
    file: String,
    since: Option<String>,
    top: Option<u32>,
    min_score: Option<f32>,
    exclude_globs: Option<Vec<String>>,
    file_type: Option<String>,
    path: Option<String>,
  ) -> std::result::Result<crate::schema::HistoryCoChangeResult, GqlError> {
    let root = path.as_deref().unwrap_or(".");
    let resolved = crate::history_sensors::resolve_root(std::path::Path::new(root));
    let res = crate::history_sensors::co_change(&file, since.as_deref(), &resolved)
      .map_err(|e| GqlError::new(e.to_string()))?;
    let res = crate::history_sensors::filter_co_change_result(
      res,
      top.map(|value| value as usize),
      min_score,
      exclude_globs.as_deref().unwrap_or(&[]),
      file_type.as_deref(),
    );
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
  /// `history todos [--since X]`.
  async fn todos(
    &self,
    ctx: &async_graphql::Context<'_>,
    since: Option<String>,
    path: Option<String>,
  ) -> std::result::Result<crate::schema::HistoryTodosResult, GqlError> {
    let root = path.as_deref().unwrap_or(".");
    let resolved = crate::history_sensors::resolve_root(std::path::Path::new(root));
    let res = crate::history_sensors::todos(since.as_deref(), &resolved)
      .map_err(|e| GqlError::new(e.to_string()))?;
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }
}

// ─────────────────────────── Mutation ─────────────────────────────
//
// Phase 4E §6 — `fix`, `trace`, `untrace`. Per the design correction
// the GraphQL surface honors D7 identically to the CLI: the
// resolvers call the same `fix_dry_run` / `fix_apply` /
// `trace_with_safety` library functions the clap dispatch arms
// invoke. Single function, two callers.
//
// Errors that fail the request (validation failures, dirty-tree
// refusals on apply mode) surface as `GqlError` with an
// `extensions.code` set to a stable diagnostic code (DIRTY_TREE,
// MISSING_SCOPE, AMBIGUOUS_SCOPE, NOT_IMPLEMENTED, INVALID_MARKER,
// INVALID_TRACE_MODE, AUDIT_LOG_MISSING, WORKSPACE_READ_ONLY).
// `lmprobe query` mirrors `errors[].extensions.code` to stderr
// alongside the message.

pub(crate) struct Mutation;

/// Convenience: build a `GqlError` with an `extensions.code` field.
/// Centralized so the diagnostic vocabulary stays consistent across
/// every mutation resolver.
fn coded_error(code: &'static str, msg: impl Into<String>) -> GqlError {
  GqlError::new(msg).extend_with(|_, e| e.set("code", code))
}

#[Object]
impl Mutation {
  /// `fix` — generic structural rewrite. `scope.paths[0]` →
  /// walker root (defaults to `.`); `scope.pattern.lang` →
  /// language hint. The top-level `pattern` / `replacement` args
  /// are the WHAT (matched + rewritten); scope is the WHERE.
  ///
  /// `dryRun: true` (default) calls `fix_dry_run`; `false` calls
  /// `fix_apply` which enforces the D7 contract internally.
  async fn fix(
    &self,
    ctx: &async_graphql::Context<'_>,
    scope: crate::gql_scalars::ScopeInput,
    pattern: String,
    replacement: String,
    dry_run: Option<bool>,
  ) -> std::result::Result<crate::schema::FixResult, GqlError> {
    use crate::gql_scalars::ScopeValidation;
    // §4.7 — refuse mutations under --workspace. The
    // workspace_ctx is stashed in schema data by `build_schema`
    // when the CLI was invoked with `--workspace`.
    if ctx.data_opt::<crate::workspace::WorkspaceCtx>().is_some() {
      return Err(coded_error(
        "WORKSPACE_READ_ONLY",
        "fix mutation refused under --workspace mode",
      ));
    }
    match scope.validate_variant() {
      ScopeValidation::Missing => {
        return Err(coded_error(
                    "MISSING_SCOPE",
                    "scope: exactly one of pattern/symbol/callers/callees/decorated/raw/union/intersection/exclusion must be set",
                ));
      }
      ScopeValidation::Ambiguous(names) => {
        return Err(coded_error(
          "AMBIGUOUS_SCOPE",
          format!(
            "scope: exactly one variant must be set, got {}",
            names.join(", ")
          ),
        ));
      }
      ScopeValidation::NotImplemented(name) => {
        return Err(coded_error(
          "NOT_IMPLEMENTED",
          format!(
            "scope variant `{}` is reserved for post-1.0; use `pattern` or `symbol` for now",
            name
          ),
        ));
      }
      ScopeValidation::Ok("pattern") | ScopeValidation::Ok("symbol") => {}
      ScopeValidation::Ok(other) => {
        return Err(coded_error(
          "NOT_IMPLEMENTED",
          format!("scope variant `{}` not yet wired in 4E-C", other),
        ));
      }
    }

    let root = scope.primary_path();
    let lang_hint: Option<SgLang> = scope
      .pattern
      .as_ref()
      .and_then(|p| p.lang)
      .map(|l| l.into())
      .or_else(|| scope.symbol.as_ref().and_then(|s| s.lang).map(|l| l.into()));

    let dry_run = dry_run.unwrap_or(true);
    if crate::program_output::target().is_some() {
      let preview =
        crate::fix_sensor::fix_dry_run(&pattern, &replacement, &root, lang_hint, &gql_walk_opts())
          .map_err(|error| GqlError::new(error.to_string()))?;
      crate::program_output::protect(
        preview
          .diffs
          .iter()
          .map(|diff| PathBuf::from(&diff.file_path)),
      )
      .map_err(|error| coded_error("OUTFILE_SOURCE_CONFLICT", error))?;
    }
    let mut res = if dry_run {
      crate::fix_sensor::fix_dry_run(&pattern, &replacement, &root, lang_hint, &gql_walk_opts())
    } else {
      crate::fix_sensor::fix_apply(
        &pattern,
        &replacement,
        &root,
        lang_hint,
        false,
        &gql_walk_opts(),
      )
    }
    .map_err(|e| coded_error("FIX_FAILED", e.to_string()))?;

    // If a refusal diagnostic landed in warnings (DIRTY_TREE,
    // NOT_GIT_REPO, etc.), surface the strongest one as a
    // request-level error so `query`'s `errors[].extensions.code`
    // mirrors the CLI's exit-2 refusal contract.
    if res.exit_status == crate::schema::ExitStatus::Error {
      if let Some(d) = res.warnings.iter().find(|d| {
        matches!(
          d.code.as_str(),
          "DIRTY_TREE" | "NOT_GIT_REPO" | "WORKSPACE_READ_ONLY" | "PATCH_APPLY_FAILED"
        )
      }) {
        let code: &'static str = match d.code.as_str() {
          "DIRTY_TREE" => "DIRTY_TREE",
          "NOT_GIT_REPO" => "NOT_GIT_REPO",
          "WORKSPACE_READ_ONLY" => "WORKSPACE_READ_ONLY",
          "PATCH_APPLY_FAILED" => "PATCH_APPLY_FAILED",
          _ => "FIX_REFUSED",
        };
        return Err(coded_error(code, d.message.clone()));
      }
    }

    if scope.extra_paths() > 0 {
      res.push_warning(crate::schema::Diagnostic::new(
        "MULTI_PATH_DROPPED",
        format!(
          "scope.paths has {} entries; only paths[0] is walked in 4E-C",
          scope.extra_paths() + 1
        ),
      ));
    }

    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }

  /// `trace` — instrument code. Two modes per design correction §6.4:
  ///
  /// - **transient** (`worktree: true`, `exec: <cmd>`): provision
  ///   a fresh git worktree, instrument, run, rollback. The user's
  ///   tree is byte-identical post-trace. Reuses
  ///   `sensors::trace_with_safety`.
  /// - **persistent** (`worktree: false`, `exec: null`): instrument
  ///   the user's tree in place; D7 dirty-tree refusal applies.
  ///   Audit log written to `trace-<marker>.audit.json`. Reuses
  ///   `sensors::trace_persistent`. The marker is required and
  ///   appears in every inserted line so `untrace` can revert.
  ///
  /// `INVALID_TRACE_MODE` for the two contradictory combos:
  /// `worktree: true + exec: null` (transient with no exec) and
  /// `worktree: false + exec: <cmd>` (persistent + auto-rollback).
  async fn trace(
    &self,
    ctx: &async_graphql::Context<'_>,
    scope: crate::gql_scalars::ScopeInput,
    template: crate::gql_scalars::TraceTemplate,
    marker: String,
    worktree: Option<bool>,
    exec: Option<String>,
  ) -> std::result::Result<crate::schema::TraceRunResult, GqlError> {
    use crate::gql_scalars::ScopeValidation;
    if ctx.data_opt::<crate::workspace::WorkspaceCtx>().is_some() {
      return Err(coded_error(
        "WORKSPACE_READ_ONLY",
        "trace mutation refused under --workspace mode",
      ));
    }
    // Marker validation (per arbitration F).
    if let Err(reason) = crate::gql_scalars::validate_marker(&marker) {
      return Err(coded_error("INVALID_MARKER", reason));
    }
    // Scope validation (same as `fix`).
    match scope.validate_variant() {
      ScopeValidation::Missing => {
        return Err(coded_error(
          "MISSING_SCOPE",
          "scope: at least one variant required",
        ));
      }
      ScopeValidation::Ambiguous(names) => {
        return Err(coded_error(
          "AMBIGUOUS_SCOPE",
          format!("scope: exactly one variant, got {}", names.join(", ")),
        ));
      }
      ScopeValidation::NotImplemented(name) => {
        return Err(coded_error(
          "NOT_IMPLEMENTED",
          format!("scope variant `{}` reserved for post-1.0", name),
        ));
      }
      ScopeValidation::Ok("pattern") | ScopeValidation::Ok("symbol") => {}
      ScopeValidation::Ok(other) => {
        return Err(coded_error(
          "NOT_IMPLEMENTED",
          format!("scope variant `{}` not yet wired in 4E-C", other),
        ));
      }
    }

    let worktree = worktree.unwrap_or(true);
    // Mode validation per arbitration F.
    match (worktree, exec.as_deref()) {
      (true, None) => {
        return Err(coded_error(
          "INVALID_TRACE_MODE",
          "worktree: true requires exec: <cmd> (transient mode runs the exec then rolls back)",
        ));
      }
      (false, Some(_)) => {
        return Err(coded_error(
          "INVALID_TRACE_MODE",
          "worktree: false + exec: <cmd> is contradictory (persistent mode does not auto-rollback)",
        ));
      }
      _ => {}
    }

    // Resolve the AST-grep query string from the scope. For
    // `pattern` variant: use scope.pattern.p. For `symbol`
    // variant: synthesize a name-only pattern (best-effort —
    // matches symbol references; a richer materialization
    // lands when ScopeInput.symbol's full surface ships).
    let query = if let Some(p) = scope.pattern.as_ref().and_then(|p| p.p.clone()) {
      p
    } else if let Some(s) = scope.symbol.as_ref() {
      s.name.clone()
    } else {
      return Err(coded_error(
                "INVALID_SCOPE",
                "trace requires `scope.pattern.p` (pattern variant) or `scope.symbol.name` (symbol variant)",
            ));
    };

    // Render the template. 4E-C uses `default` only — per-language
    // overrides reserved for a later chunk (see `TraceTemplate`).
    let template_src = template.default.clone().ok_or_else(|| {
      coded_error(
        "INVALID_TRACE_MODE",
        "trace requires `template.default: String!` in 4E-C; per-language templates are reserved",
      )
    })?;

    let root = scope.primary_path();

    crate::program_output::protect_trace_root(&root)
      .map_err(|error| coded_error("OUTFILE_SOURCE_CONFLICT", error))?;
    let res = if worktree {
      // Transient mode: existing trace_with_safety with worktree=Some(None).
      let safety = sensors::TraceSafety {
        worktree: Some(None),
        keep_patch: None,
      };
      let exec_cmd = exec.as_deref().unwrap_or("true");
      sensors::trace_with_safety(
        &query,
        &root,
        exec_cmd,
        Some(template_src),
        false,
        &safety,
        &gql_walk_opts(),
      )
      .map_err(|e| coded_error("TRACE_FAILED", e.to_string()))?
    } else {
      // Persistent mode: new code path.
      sensors::trace_persistent(
        &query,
        &root,
        &template_src,
        &marker,
        false,
        &gql_walk_opts(),
      )
      .map_err(|e| coded_error("TRACE_FAILED", e.to_string()))?
    };

    // Surface refusal warnings as request-level errors so
    // `errors[].extensions.code` mirrors the CLI's exit-2 contract.
    if res.exit_status == crate::schema::ExitStatus::Error {
      if let Some(d) = res.warnings.iter().find(|d| {
        matches!(
          d.code.as_str(),
          "DIRTY_TREE" | "NOT_GIT_REPO" | "WORKSPACE_READ_ONLY" | "INVALID_TRACE_MODE"
        )
      }) {
        let code: &'static str = match d.code.as_str() {
          "DIRTY_TREE" => "DIRTY_TREE",
          "NOT_GIT_REPO" => "NOT_GIT_REPO",
          "WORKSPACE_READ_ONLY" => "WORKSPACE_READ_ONLY",
          "INVALID_TRACE_MODE" => "INVALID_TRACE_MODE",
          _ => "TRACE_REFUSED",
        };
        return Err(coded_error(code, d.message.clone()));
      }
    }
    record_query_status(ctx, res.exit_status);
    record_query_diagnostics(ctx, &res.warnings, false);
    Ok(res)
  }

  /// `untrace` — revert insertions tagged with `marker`. Reads
  /// `$XDG_CACHE_HOME/lmprobe/trace-<marker>.audit.json` if
  /// present (`mode: "audit_log"`); falls back to regex scan on
  /// `[TRACE-<marker>]` (`mode: "regex_fallback"`,
  /// `AUDIT_LOG_MISSING` warning).
  ///
  /// D7: dirty-tree refusal applies (the audit-log path mutates
  /// the user's tree in place; same contract as `trace`
  /// persistent mode).
  async fn untrace(
    &self,
    ctx: &async_graphql::Context<'_>,
    marker: String,
    worktree: Option<bool>,
    path: Option<String>,
  ) -> std::result::Result<crate::schema::UntraceResult, GqlError> {
    if ctx.data_opt::<crate::workspace::WorkspaceCtx>().is_some() {
      return Err(coded_error(
        "WORKSPACE_READ_ONLY",
        "untrace mutation refused under --workspace mode",
      ));
    }
    if let Err(reason) = crate::gql_scalars::validate_marker(&marker) {
      return Err(coded_error("INVALID_MARKER", reason));
    }
    if worktree.unwrap_or(false) {
      return Err(coded_error(
        "NOT_IMPLEMENTED",
        "untrace --worktree is reserved for a later chunk; pass worktree: false (default)",
      ));
    }
    let root = path.unwrap_or_else(|| ".".to_string());
    let root_path = std::path::Path::new(&root);

    crate::program_output::protect_trace_root(root_path)
      .map_err(|error| coded_error("OUTFILE_SOURCE_CONFLICT", error))?;
    if crate::program_output::target().is_some() {
      let audit = sensors::persistent_audit_path(&marker);
      crate::program_output::protect([audit.clone()])
        .map_err(|error| coded_error("OUTFILE_SOURCE_CONFLICT", error))?;
      if let Ok(text) = std::fs::read_to_string(&audit) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
          let paths = value
            .get("insertions")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.get("file_path").and_then(serde_json::Value::as_str))
            .map(PathBuf::from);
          crate::program_output::protect(paths)
            .map_err(|error| coded_error("OUTFILE_SOURCE_CONFLICT", error))?;
        }
      }
    }

    if crate::program_output::target().is_some() {
      let fallback_targets =
        sensors::untrace_fallback_targets(root_path, &marker, &gql_walk_opts());
      crate::program_output::protect(fallback_targets)
        .map_err(|error| coded_error("OUTFILE_SOURCE_CONFLICT", error))?;
    }

    // D7 dirty-tree refusal — `untrace` mutates in place.
    if let Err(diag) = sensors::check_trace_safety(root_path, false) {
      let code: &'static str = match diag.code.as_str() {
        "DIRTY_TREE" => "DIRTY_TREE",
        "NOT_GIT_REPO" => "NOT_GIT_REPO",
        _ => "UNTRACE_REFUSED",
      };
      return Err(coded_error(code, diag.message));
    }

    // Try audit-log path first.
    match sensors::untrace_from_audit_log(&marker) {
      Ok(sites) => {
        let res = crate::schema::UntraceResult::new(marker, "audit_log", sites);
        record_query_status(ctx, res.exit_status);
        record_query_diagnostics(ctx, &res.warnings, false);
        Ok(res)
      }
      Err(_) => {
        // Fallback: regex scan.
        let sites = sensors::untrace_regex_fallback(root_path, &marker, &gql_walk_opts())
          .map_err(|e| coded_error("UNTRACE_FAILED", e.to_string()))?;
        let mut res = crate::schema::UntraceResult::new(marker.clone(), "regex_fallback", sites);
        res.push_warning(
                    crate::schema::Diagnostic::new(
                        "AUDIT_LOG_MISSING",
                        format!(
                            "no audit log at {} — fell back to regex scan for `[TRACE-{}]`",
                            sensors::persistent_audit_path(&marker).display(),
                            marker
                        ),
                    )
                    .with_hint("audit logs are content-addressable; if missing, the trace mutation may have crashed before writing"),
                );
        record_query_status(ctx, res.exit_status);
        record_query_diagnostics(ctx, &res.warnings, false);
        Ok(res)
      }
    }
  }
}

#[cfg(feature = "serve")]
async fn graphql_handler(
  State(schema): State<Schema<Query, Mutation, EmptySubscription>>,
  req: GraphQLRequest,
) -> GraphQLResponse {
  schema.execute(req.into_inner()).await.into()
}

/// §1.7 — bearer-token middleware. When `AuthState.token` is `Some`,
/// every request must carry `Authorization: Bearer <token>` matching
/// the configured token byte-for-byte (constant-time compare via
/// `subtle::ConstantTimeEq`). When `None` (loopback default), the
/// middleware is a no-op.
#[cfg(feature = "serve")]
#[derive(Clone)]
pub(crate) struct AuthState {
  pub token: Option<String>,
}

#[cfg(feature = "serve")]
pub(crate) async fn require_bearer_token(
  axum::extract::State(state): axum::extract::State<AuthState>,
  req: axum::http::Request<axum::body::Body>,
  next: axum::middleware::Next,
) -> std::result::Result<axum::response::Response, axum::http::StatusCode> {
  let Some(expected) = state.token.as_deref() else {
    // No auth configured — pass through (loopback case).
    return Ok(next.run(req).await);
  };
  let header = req
    .headers()
    .get(axum::http::header::AUTHORIZATION)
    .and_then(|v| v.to_str().ok())
    .unwrap_or("");
  let presented = header.strip_prefix("Bearer ").unwrap_or("");
  use subtle::ConstantTimeEq;
  // Length-mismatched compare is rejected by `ct_eq` on length, but
  // we explicitly check length first to keep the bytes-compare
  // running on equal-length slices (timing oracle hardening).
  if presented.len() != expected.len() {
    return Err(axum::http::StatusCode::UNAUTHORIZED);
  }
  if presented.as_bytes().ct_eq(expected.as_bytes()).unwrap_u8() == 1 {
    Ok(next.run(req).await)
  } else {
    Err(axum::http::StatusCode::UNAUTHORIZED)
  }
}

#[cfg(test)]
mod help_tests {
  use super::*;

  // Our examples use simple shell words and quotes, without expansions/pipelines.
  // Parse them as argv only: never execute mutation, network, or daemon examples.
  fn example_words(example: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    for c in example.chars() {
      match c {
        '\'' | '"' if quote == Some(c) => quote = None,
        '\'' | '"' if quote.is_none() => quote = Some(c),
        c if c.is_whitespace() && quote.is_none() => {
          if !word.is_empty() {
            words.push(std::mem::take(&mut word));
          }
        }
        _ => word.push(c),
      }
    }
    assert!(quote.is_none(), "unclosed quote in {example}");
    if !word.is_empty() {
      words.push(word);
    }
    words
  }

  #[test]
  fn every_command_has_help_examples_and_complete_developer_memory() {
    fn visit(command: &mut clap::Command, path: Vec<String>, common: &str) {
      if command.get_name() == "help" {
        return;
      }
      let name = path.join(" ");
      let after = command
        .get_after_help()
        .expect("command example")
        .to_string();
      let example = after
        .lines()
        .find(|line| line.trim().starts_with("lmprobe "))
        .unwrap();
      let args = example_words(example.trim());
      assert!(
        Cli::try_parse_from(&args).is_ok(),
        "invalid example: {example}"
      );
      for help in [
        command.render_help().to_string(),
        command.render_long_help().to_string(),
      ] {
        assert!(help.contains(example.trim()), "example missing for {name}");
        assert!(
          !help.contains("Phase ") && !help.contains('§'),
          "internal prose in {name}"
        );
      }
      if !path.is_empty() {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
          .join("../../durable-memory/commands")
          .join(format!("{}.md", path.join("-")));
        let doc =
          std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        for arg in command.get_arguments() {
          if let Some(long) = arg.get_long() {
            let flag = format!("--{long}");
            assert!(
              doc.contains(&flag) || (arg.is_global_set() && common.contains(&flag)),
              "{name}: undocumented {flag}"
            );
          }
        }
        for line in doc.lines() {
          if let Some((_, recipe)) = line.split_once("| `lmprobe ") {
            let recipe = format!(
              "lmprobe {}",
              recipe.split('`').next().unwrap().replace("\\|", "|")
            );
            let args = example_words(&recipe);
            match Cli::try_parse_from(normalize_invocation(
              args.iter().map(OsString::from).collect(),
            )) {
              Ok(_) => {}
              Err(e) if e.kind() == clap::error::ErrorKind::DisplayHelp => {}
              Err(e) => panic!("invalid developer recipe {recipe}: {e}"),
            }
          }
        }
      }
      for child in command.get_subcommands_mut() {
        let mut child_path = path.clone();
        child_path.push(child.get_name().to_string());
        visit(child, child_path, common);
      }
    }
    let common = include_str!("../../../durable-memory/commands/common.md");
    let mut command = Cli::command();
    command.build();
    visit(&mut command, Vec::new(), common);
  }

  #[test]
  fn common_invocation_modes_parse() {
    for args in [
      vec!["lmprobe", "search", "--name", "Parser", "."],
      vec!["lmprobe", "search", "-p", "foo($X)", "-l", "python", "."],
      vec![
        "lmprobe",
        "grep",
        "TODO|FIXME",
        "--path",
        "src",
        "--path",
        "tests",
      ],
      vec!["lmprobe", "find", "\\.rs$", "--paths-only", "--null", "."],
      vec!["lmprobe", "def", "Parser", "--body", "."],
      vec!["lmprobe", "history", "churn", "--since", "30d", "."],
      vec!["lmprobe", "secrets-recipes", "list", "--provider", "aws"],
      vec!["lmprobe", "secrets-sources", "show", "env-aws/python"],
      vec![
        "lmprobe",
        "query",
        "{ discover(path: \".\") { projects { name } } }",
      ],
      vec!["lmprobe", "fix", "-p", "foo($X)", "-r", "bar($X)", "."],
    ] {
      assert!(Cli::try_parse_from(&args).is_ok(), "{args:?}");
    }
  }

  #[test]
  fn required_arguments_and_conflicts_remain_errors() {
    for args in [
      vec!["lmprobe", "def"],
      vec!["lmprobe", "py-slice-candidates"],
      vec!["lmprobe", "trace", "foo($X)"],
      vec!["lmprobe", "callsites", "foo"],
      vec!["lmprobe", "search", "--name", "foo", "-p", "foo($X)"],
      vec!["lmprobe", "context", "foo", "--def", "--usage"],
      vec![
        "lmprobe",
        "impact",
        "foo",
        "--tests-only",
        "--exclude-tests",
      ],
    ] {
      assert!(Cli::try_parse_from(&args).is_err(), "{args:?}");
    }
  }
}
