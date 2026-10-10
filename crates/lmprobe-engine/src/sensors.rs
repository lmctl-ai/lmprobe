//! Sensor implementations.
//!
//! Each public function here is a thin schema-producing wrapper
//! around the workspace walk. CLI and GraphQL resolvers (`query` /
//! `serve`) call into these functions and render with `crate::output`.
//!
//! Phase 0 Chunk 0B brings:
//! - §0.4 panic fixes (cross-language `callers`, quoted `imports`, broken pipe)
//! - §0.6 `def` anchored to full identifier (with `--regex` opt-in)
//! - §0.7 schema-side diagnostics + envelope `exit_status`
//! - §0.8 `discover` top-6 marker table
//! - §0.9 `trace` mutation-safety guard (dirty-tree, non-git, audit log)
//! - §0.10 XDG cache for `evolution` (with legacy back-compat)

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use ast_grep_core::matcher::{KindMatcher, Pattern};
use ast_grep_language::{Language, LanguageExt};
use git2::{Repository, StatusOptions};
use ignore::WalkBuilder;
use regex::Regex;
use rusqlite::params;
use serde::Deserialize;

use crate::history::{
  init_history_db, legacy_history_db_path, trace_audit_log_path, xdg_cache_dir, xdg_history_db_path,
};
use crate::index::{create_skeleton, EVOLUTION_KINDS};
use lmprobe_lang_core::langs::SgLang;
use lmprobe_registry::{
  call_grammar_for, decoration_grammar_for, name_extractor_for, taxonomy_for, trace_template_for,
};
use lmprobe_types::error::{anyhow, Context, Result};
use lmprobe_types::schema::{
  CallersResult, CallsiteArgsHit, CallsiteArgument, CallsiteFilterInput, CallsiteKeywordArg,
  CallsiteKind, CallsiteLanguage, CallsiteRole, CallsitesResult, DecorationsResult, DefResult,
  Diagnostic, DiscoverResult, EvolutionResult, ExitStatus, FindResult, GrepHit, GrepResultEnvelope,
  ImportsResult, LineContext, Milestone, Project, PySliceCandidate, PySliceCandidatesResult,
  PySliceTracePlan, RefResult, ScanStats, SearchResult, SourceReadFilterInput, SourceReadHit,
  SourceReadLanguage, SourceReadsResult, StructuralBody, StructuralParent, SymbolHit, TraceEvent,
  TraceRunResult,
};

// ──────────────────────────── walker opts ─────────────────────────
//
// Phase 1B push-back closure — every sensor accepts a `&WalkOpts`
// derived from `cli::CommonArgs`. The CommonArgs --help surface
// (-G / --globs, -j / --threads, --no-ignore, --max-filesize, -t /
// --type, -l / --lang) now reaches `WalkBuilder` for real.

#[derive(Default, Clone, Debug)]
pub struct WalkOpts {
  pub threads: Option<usize>,
  pub no_ignore: bool,
  pub no_default_excludes: bool,
  pub quick_project_roots: bool,
  pub max_filesize: Option<u64>,
  /// Primary result cap. Today this is consumed by `find_many`
  /// (post-rank truncate) and `grep_text_hits` (early stop).
  pub result_limit: Option<usize>,
  pub globs: Vec<String>,
  /// Phase 6A #9: explicit exclude globs, internally prefixed with
  /// `!` and appended to the same `OverrideBuilder` so include +
  /// exclude rules compose. Distinct flag (vs. requiring users to
  /// know the `!`-prefix syntax of the include flag).
  pub exclude_globs: Vec<String>,
  pub type_filter: Option<String>,
  pub lang_filter: Option<SgLang>,
}

pub const DEFAULT_EXCLUDE_DIRS: &[&str] = &[
  ".git",
  ".hg",
  ".svn",
  "node_modules",
  "bower_components",
  ".pnpm-store",
  ".yarn/cache",
  "target",
  "dist",
  "build",
  "out",
  ".next",
  ".nuxt",
  ".svelte-kit",
  "coverage",
  ".cache",
  ".parcel-cache",
  ".turbo",
  "__pycache__",
  ".pytest_cache",
  ".mypy_cache",
  ".ruff_cache",
  ".tox",
  ".venv",
  "venv",
];

const QUICK_PROJECT_MARKERS: &[&str] = &["package.json"];
const QUICK_PERL_DENSE_FILE_THRESHOLD: usize = 5;
const QUICK_PROJECT_DISCOVERY_MAX_DEPTH: usize = 6;

/// Apply `WalkOpts` to a freshly-built `WalkBuilder`. Returns the
/// builder so call sites can chain additional configuration if
/// needed.
/// §4.D — walk a NodeMatch's parent chain to find the nearest
/// enclosing declaration; extract its name via the lang's
/// NameExtractor. Returns `None` when no declaration is found
/// upstream (e.g. the match is at the top level of a file).
///
/// Used by `callers` so each call-site hit carries the caller's
/// own decl name in `enclosing_decl`. The `impact` upstream BFS
/// reads this field instead of doing a line-walk-back heuristic.
///
/// Phase 4E §4E-C.5: returns `(name, kind)` so the GraphQL
/// `EnclosingDecl.kind` resolver can ship a populated kind. `kind`
/// is the tree-sitter literal kind of the containing declaration
/// node (`"function_item"`, `"class_definition"`, etc.) — same
/// vocabulary as `SymbolHit.kind` on a regular def hit.
pub fn enclosing_decl_name(
  m: &ast_grep_core::NodeMatch<'_, ast_grep_core::tree_sitter::StrDoc<SgLang>>,
  lang: SgLang,
) -> Option<(String, String)> {
  let extractor = lmprobe_registry::name_extractor_for(lang);
  let mut current = m.parent();
  while let Some(node) = current {
    let nk: String = node.kind().to_string();
    if is_structural_parent_kind(&nk, lang) {
      let parent_match = ast_grep_core::NodeMatch::from(node.clone());
      if let Some(name) = extractor.extract(&parent_match) {
        if !name.is_empty() {
          return Some((name, nk));
        }
      }
    }
    current = node.parent();
  }
  None
}

fn is_structural_parent_kind(kind: &str, lang: SgLang) -> bool {
  taxonomy_for(lang)
    .all_declaration_kinds()
    .iter()
    .any(|decl_kind| *decl_kind == kind)
    || is_container_decl_kind(kind)
}

fn is_grep_parent_kind(kind: &str) -> bool {
  is_container_decl_kind(kind)
    || kind.contains("function")
    || (kind.contains("method") && !kind.contains("invocation") && !kind.contains("call"))
}

fn is_grep_body_declaration_kind(kind: &str) -> bool {
  is_container_decl_kind(kind)
    || kind.contains("function")
    || kind == "method_declaration"
    || kind == "method_definition"
}

fn is_grep_body_kind(kind: &str) -> bool {
  is_grep_body_declaration_kind(kind)
    || matches!(
      kind,
      "if_statement"
        | "if_expression"
        | "for_statement"
        | "enhanced_for_statement"
        | "for_in_statement"
        | "while_statement"
        | "while_expression"
        | "with_statement"
        | "try_statement"
        | "except_clause"
        | "finally_clause"
        | "match_statement"
        | "match_expression"
        | "case_clause"
        | "switch_statement"
        | "switch_expression"
        | "catch_clause"
        | "do_statement"
        | "loop_expression"
    )
}

fn is_container_decl_kind(kind: &str) -> bool {
  kind.contains("class")
    || kind.contains("struct")
    || kind.contains("interface")
    || kind.contains("trait")
    || kind.contains("enum")
    || kind.contains("impl")
    || kind.contains("object")
}

pub fn apply_walk_opts<'a>(
  wb: &'a mut WalkBuilder,
  opts: &WalkOpts,
  root: &Path,
) -> &'a mut WalkBuilder {
  if let Some(t) = opts.threads {
    wb.threads(t);
  }
  if opts.no_ignore {
    wb.git_ignore(false)
      .git_global(false)
      .git_exclude(false)
      .ignore(false)
      .require_git(false);
  }
  let use_default_excludes = !opts.no_default_excludes;
  if !opts.globs.is_empty() || !opts.exclude_globs.is_empty() || use_default_excludes {
    let mut ob = ignore::overrides::OverrideBuilder::new(root);
    for g in &opts.globs {
      let _ = ob.add(g);
    }
    if use_default_excludes {
      for dir in DEFAULT_EXCLUDE_DIRS {
        let _ = ob.add(&format!("!**/{dir}"));
        let _ = ob.add(&format!("!**/{dir}/**"));
      }
    }
    // Phase 6A #9: prefix each exclude glob with `!` per the
    // `ignore` crate's override DSL.
    for g in &opts.exclude_globs {
      let _ = ob.add(&format!("!{}", g));
    }
    if let Ok(overrides) = ob.build() {
      wb.overrides(overrides);
    }
  }
  if let Some(t) = &opts.type_filter {
    let mut tb = ignore::types::TypesBuilder::new();
    tb.add_defaults();
    let _ = tb.select(t);
    if let Ok(types) = tb.build() {
      wb.types(types);
    }
  }
  wb
}

/// Counts files a same-shaped walk would additionally surface with
/// `.gitignore`/`.ignore`/global-ignore rules disabled — i.e. how many
/// files the default (ignore-respecting) walk silently dropped.
///
/// Dogfood finding (2026-09): `find`/`grep` honour `.gitignore` by
/// default (correct), but said nothing when that hid most of a
/// matching corpus (a fleet where `*.lmctl` is gitignore'd by policy
/// saw 140/357 files — 61% silently excluded — with zero indication in
/// the envelope). An empty or short result reads as "nothing is
/// there," not "nothing was looked at." Callers push this count as a
/// `GITIGNORE_PATHS_EXCLUDED` warning so the default path fails
/// informatively instead of silently.
///
/// Deliberately a plain file count, not a per-pattern match count:
/// the `ignore` crate drops gitignored entries before they ever reach
/// a walk callback (no hook observes what was skipped), so the only
/// way to measure it through the public API is to diff two full
/// metadata-only walks — one respecting ignore rules (matching the
/// real walk's `opts`), one with them disabled. Both walks apply the
/// same overrides/type-filter/default-exclude-dirs, so the diff is
/// attributable to ignore rules specifically, not to this tool's own
/// default excludes or user-supplied globs. No file contents are
/// read, so the added cost is one extra parallel directory walk, not
/// a second content scan.
fn count_gitignore_excluded(root: &Path, opts: &WalkOpts) -> usize {
  count_gitignore_excluded_with_prune(root, opts, None)
}

fn count_gitignore_excluded_with_prune(
  root: &Path,
  opts: &WalkOpts,
  prune: Option<&Arc<crate::file_predicate::FilePredicate>>,
) -> usize {
  if opts.no_ignore {
    return 0;
  }
  let count_files = |walk_opts: &WalkOpts| -> usize {
    let mut wb = WalkBuilder::new(root);
    apply_walk_opts(&mut wb, walk_opts, root);
    let walker = wb.build_parallel();
    let counter = Arc::new(AtomicUsize::new(0));
    walker.run(|| {
      let counter = counter.clone();
      let prune = prune.cloned();
      Box::new(move |result| {
        if let Ok(entry) = result {
          if let Some(kind) = entry.file_type() {
            if kind.is_dir()
              && prune
                .as_ref()
                .is_some_and(|rule| rule.matches(entry.path(), kind).unwrap_or(true))
            {
              return ignore::WalkState::Skip;
            }
          }
          if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            counter.fetch_add(1, Ordering::Relaxed);
          }
        }
        ignore::WalkState::Continue
      })
    });
    Arc::try_unwrap(counter)
      .map(AtomicUsize::into_inner)
      .unwrap_or(0)
  };

  let mut without_ignore = opts.clone();
  without_ignore.no_ignore = true;
  count_files(&without_ignore).saturating_sub(count_files(opts))
}

/// Sums `count_gitignore_excluded` across every scan root. Call once per
/// invocation and share the result — the warning and (for `grep`)
/// `scan_stats.files_ignored` both need it, and it's a real second walk,
/// not free.
fn count_gitignore_excluded_total(roots: &[PathBuf], opts: &WalkOpts) -> usize {
  roots
    .iter()
    .map(|r| count_gitignore_excluded(r, opts))
    .sum()
}

/// Builds the warning diagnostic from an already-computed total.
/// Returns `None` when nothing was excluded (including when
/// `--no-ignore` was passed) so call sites can push unconditionally.
fn gitignore_excluded_warning(total: usize) -> Option<Diagnostic> {
  if total == 0 {
    return None;
  }
  let noun = if total == 1 { "path" } else { "paths" };
  Some(
    Diagnostic::new(
      "GITIGNORE_PATHS_EXCLUDED",
      format!("{total} {noun} excluded by .gitignore/.ignore rules"),
    )
    .with_hint("pass --no-ignore to include them"),
  )
}

/// Per-entry triage. Returns:
/// - `Process` — caller should read the file and run sensor logic.
/// - `Skip`    — entry isn't a file, no language matched, or
///               `lang_filter` excludes it. Silent skip.
/// - `SkipWithDiag` — the entry was a candidate but exceeded
///                    `max_filesize`; caller should record the
///                    diagnostic and skip.
pub enum PreCheck {
  Process(PathBuf, SgLang),
  Skip,
  SkipWithDiag(Diagnostic),
}

/// Phase 6A #8 — shebang-aware language detection for extensionless
/// files. Reads the first ~256 bytes; if the line starts with `#!`,
/// scans the interpreter token for a known language. Returns None
/// for files with extensions (extension takes priority) or unknown
/// shebangs. Cheap: bounded read, no full parse.
fn sniff_shebang(path: &Path) -> Option<SgLang> {
  if path.extension().is_some() {
    return None;
  }
  let mut f = std::fs::File::open(path).ok()?;
  let mut buf = [0u8; 256];
  let n = std::io::Read::read(&mut f, &mut buf).ok()?;
  let s = std::str::from_utf8(&buf[..n]).ok()?;
  if !s.starts_with("#!") {
    return None;
  }
  let first_line = s.split('\n').next().unwrap_or("");
  if first_line.contains("bash") || first_line.contains("/sh") || first_line.contains(" sh") {
    return Some(SgLang::Bash);
  }
  if first_line.contains("python") {
    return Some(SgLang::Python);
  }
  if first_line.contains("node") {
    return Some(SgLang::JavaScript);
  }
  if first_line.contains("ruby") {
    return Some(SgLang::Ruby);
  }
  if first_line.contains("lua") {
    return Some(SgLang::Lua);
  }
  if first_line.contains("php") {
    return Some(SgLang::Php);
  }
  None
}

fn is_cython_path(path: &Path) -> bool {
  path
    .extension()
    .and_then(|ext| ext.to_str())
    .is_some_and(|ext| ext.eq_ignore_ascii_case("pyx"))
}

fn cython_not_supported_diagnostic(path: &Path) -> Diagnostic {
  Diagnostic::new(
    "CYTHON_NOT_SUPPORTED",
    format!(
      "structural analysis does not support Cython .pyx files: {}",
      path.display()
    ),
  )
  .with_hint("use `lmprobe grep` / `lmprobe find` for .pyx files, or force `-l python` for best-effort Python parsing")
  .with_lang("Cython")
}

pub fn walk_pre_check(entry: &ignore::DirEntry, opts: &WalkOpts) -> PreCheck {
  let path = entry.path();
  if !path.is_file() {
    return PreCheck::Skip;
  }
  // Phase 6A #8 — lang resolution priority:
  //   1. `-l <lang>` (lang_filter) — authoritative force; overrides
  //      extension AND shebang detection. The walker still skips
  //      non-files / oversized / gitignored as usual; for everything
  //      visited, the parser uses `-l`'s lang. Users who want
  //      extension-class filtering should pass `--type <lang>`.
  //   2. Extension via `SgLang::from_path`.
  //   3. Shebang sniff (extensionless files only).
  //   4. Skip.
  let lang = if let Some(forced) = opts.lang_filter {
    forced
  } else if let Some(l) = SgLang::from_path(path) {
    l
  } else if is_cython_path(path) {
    return PreCheck::SkipWithDiag(cython_not_supported_diagnostic(path));
  } else if let Some(l) = sniff_shebang(path) {
    l
  } else {
    return PreCheck::Skip;
  };
  if let Some(max) = opts.max_filesize {
    if let Ok(meta) = path.metadata() {
      if meta.len() > max {
        let d = Diagnostic::new(
          "FILE_TOO_LARGE",
          format!(
            "skipped {} ({} bytes > --max-filesize {})",
            path.display(),
            meta.len(),
            max
          ),
        );
        return PreCheck::SkipWithDiag(d);
      }
    }
  }
  PreCheck::Process(path.to_path_buf(), lang)
}

// ─────────────────────────── search / def ─────────────────────────

/// Caller-supplied filters for `search` (and indirectly `def`).
#[derive(Clone)]
pub struct SearchConstraints {
  pub query: Option<String>,
  pub has_decoration: Option<String>,
  pub has_child: Option<String>,
  pub has_attr: Option<String>,
  /// Portable kind (function, class, etc.) — taxonomy-mapped.
  pub kind: Option<String>,
  /// Tree-sitter literal kind — bypasses the taxonomy.
  pub raw_kind: Option<String>,
  /// `false` = today's substring-on-name behaviour (used by `search`).
  /// `true`  = exact-token match (used by `def`; QA §2.7 §0.6).
  pub exact_name: bool,
  /// §1.2 — YAML SerializableRule wrapped in `has:` form. Filters
  /// candidate matches: must contain a descendant matching this
  /// inner rule.
  pub has_rule: Option<String>,
  /// Optional containing class/type name filter, used by `def --class`.
  pub class_filter: Option<String>,
  /// §1.3 — `code` (default) only walks programming-language files;
  /// `all` walks every file ast-grep recognises.
  pub scope_code_only: bool,
  /// §1.1 — pattern-mode metadata.
  pub pattern: Option<PatternMode>,
}

/// §1.1 — pattern-mode payload threaded through to the sensor.
#[derive(Clone)]
pub struct PatternMode {
  pub source: String,
  pub language: SgLang,
  pub strictness: Option<ast_grep_core::MatchStrictness>,
}

impl Default for SearchConstraints {
  fn default() -> Self {
    Self {
      query: None,
      has_decoration: None,
      has_child: None,
      has_attr: None,
      kind: None,
      raw_kind: None,
      exact_name: false,
      has_rule: None,
      class_filter: None,
      scope_code_only: false,
      pattern: None,
    }
  }
}

#[derive(Default)]
struct ScanStatsCounters {
  files_considered: AtomicUsize,
  files_scanned: AtomicUsize,
  files_skipped: AtomicUsize,
}

impl ScanStatsCounters {
  fn considered(&self) {
    self.files_considered.fetch_add(1, Ordering::Relaxed);
  }

  fn scanned(&self) {
    self.files_scanned.fetch_add(1, Ordering::Relaxed);
  }

  fn skipped(&self) {
    self.files_skipped.fetch_add(1, Ordering::Relaxed);
  }

  fn snapshot(&self) -> ScanStats {
    ScanStats {
      files_considered: self.files_considered.load(Ordering::Relaxed),
      files_scanned: self.files_scanned.load(Ordering::Relaxed),
      files_skipped: self.files_skipped.load(Ordering::Relaxed),
      // `search` doesn't compute gitignore-exclusion counts yet (only
      // `grep` does, see `grep_many`) — `None`, not `Some(0)`, so this
      // stays an honest "not measured" rather than a false zero.
      files_ignored: None,
    }
  }
}

pub fn search(
  root: &Path,
  outline: bool,
  constraints: SearchConstraints,
  opts: &WalkOpts,
) -> Result<SearchResult> {
  let (hits, diags, scan_stats) = search_symbol_hits_with_stats(root, outline, constraints, opts)?;
  let mut env = SearchResult::new(hits);
  if env.exit_status == ExitStatus::NoMatch {
    env.scan_stats = Some(scan_stats);
  }
  for d in diags {
    env.push_warning(d);
  }
  Ok(env)
}

/// `def` is implemented as a search anchored on the full token. The
/// `--regex` opt-in (carried as `regex_mode` here) restores the old
/// substring behaviour for callers that depended on it. §1.3 adds
/// `--scope code|all`.
pub fn def(
  name: String,
  root: &Path,
  regex_mode: bool,
  class_filter: Option<String>,
  scope_code: bool,
  this_file: bool,
  body: bool,
  opts: &WalkOpts,
) -> Result<DefResult> {
  let constraints = SearchConstraints {
    query: Some(name),
    exact_name: !regex_mode,
    class_filter,
    scope_code_only: scope_code,
    ..Default::default()
  };
  let (mut hits, diags) = search_symbol_hits(root, !body, constraints, opts)?;
  if this_file {
    prefer_concrete_definition_hits(&mut hits);
  }
  let mut env = DefResult::new(hits);
  for d in diags {
    env.push_warning(d);
  }
  Ok(env)
}

fn prefer_concrete_definition_hits(hits: &mut Vec<SymbolHit>) {
  if hits.iter().any(is_concrete_definition_hit) {
    hits.retain(is_concrete_definition_hit);
  }
}

fn is_concrete_definition_hit(hit: &SymbolHit) -> bool {
  let kind = hit.kind.as_str();
  !(kind.contains("interface")
    || kind.contains("trait")
    || kind.contains("protocol")
    || kind.contains("signature")
    || kind.contains("declaration_signature")
    || hit.content.trim_end().ends_with(';'))
}

/// §1.1 — pattern-mode walker. Parses the pattern once with the
/// caller-supplied language (clap `-l`) and applies it across files
/// matching that language. The §1.4 KindTaxonomy prefilter still
/// applies via the `--kind` constraint when supplied.
fn search_pattern_mode(
  root: &Path,
  pm: &PatternMode,
  constraints: &SearchConstraints,
  opts: &WalkOpts,
) -> Result<(Vec<SymbolHit>, Vec<Diagnostic>, ScanStats)> {
  use ast_grep_core::matcher::Pattern;

  let lang = pm.language;
  if matches!(
    pm.strictness.as_ref(),
    Some(ast_grep_core::MatchStrictness::Signature)
  ) && pm.source.contains("$$$")
  {
    return Ok((
      Vec::new(),
      vec![
        Diagnostic::new(
          "PATTERN_STRICTNESS_ELLIPSIS_UNSUPPORTED",
          "`--strictness signature` cannot be used with ellipsis/multi-metavariable patterns such as `$$$`",
        )
        .with_hint("drop `--strictness signature`, or rewrite the pattern without `$$$`")
        .with_lang(format!("{:?}", lang)),
      ],
      ScanStats::default(),
    ));
  }

  // Build the pattern once.
  let mut pattern: Pattern = match Pattern::try_new(&pm.source, lang) {
    Ok(p) => p,
    Err(e) => return Err(anyhow!("pattern parse failed: {}", e)),
  };
  if let Some(s) = pm.strictness.clone() {
    pattern = pattern.with_strictness(s);
  }

  // §1.2 PUSH-BACK 2 — build the rule once at the language level
  // (pattern-mode binds a single language) and emit
  // RULE_PARSE_FAILED to the diagnostic channel on failure.
  let (has_rule, diagnostics): (Option<Arc<ast_grep_config::Rule>>, Vec<Diagnostic>) =
    if let Some(yaml) = &constraints.has_rule {
      match crate::rules::build_has_rule(yaml, lang) {
        Ok(r) => (Some(Arc::new(r)), Vec::new()),
        Err(e) => (
          None,
          vec![Diagnostic::new(
            "RULE_PARSE_FAILED",
            format!("relational rule failed to parse: {}", e),
          )
          .with_lang(format!("{:?}", lang))],
        ),
      }
    } else {
      (None, Vec::new())
    };
  // If the rule is the only filter and it failed to parse, return
  // empty hits + the diagnostic (don't silently match every node).
  let rule_failed = constraints.has_rule.is_some() && has_rule.is_none();
  let too_large: Arc<Mutex<Vec<Diagnostic>>> = Arc::new(Mutex::new(Vec::new()));
  let scan_stats = Arc::new(ScanStatsCounters::default());

  let (tx, rx) = mpsc::channel();
  let mut wb = WalkBuilder::new(root);
  apply_walk_opts(&mut wb, opts, root);
  let walker = wb.build_parallel();
  walker.run(|| {
    let tx = tx.clone();
    let too_large = too_large.clone();
    let scan_stats = scan_stats.clone();
    let pattern = pattern.clone();
    let has_rule = has_rule.clone();
    let rule_failed = rule_failed;
    let opts = opts;
    Box::new(move |result| {
      if rule_failed {
        return ignore::WalkState::Continue;
      }
      if let Ok(entry) = result {
        let entry_path = entry.path();
        let entry_is_file = entry_path.is_file();
        if entry_is_file {
          scan_stats.considered();
        }
        let (path_buf, file_lang) = match walk_pre_check(&entry, opts) {
          PreCheck::Process(p, l) => (p, l),
          PreCheck::Skip => {
            if entry_is_file {
              scan_stats.skipped();
            }
            return ignore::WalkState::Continue;
          }
          PreCheck::SkipWithDiag(d) => {
            if entry_is_file {
              scan_stats.skipped();
            }
            too_large.lock().unwrap().push(d);
            return ignore::WalkState::Continue;
          }
        };
        let path = path_buf.as_path();
        {
          {
            if file_lang != lang {
              if entry_is_file {
                scan_stats.skipped();
              }
              return ignore::WalkState::Continue;
            }
            if let Ok(content) = fs::read_to_string(path) {
              scan_stats.scanned();
              let grep = lang.ast_grep(content);
              let root_node = grep.root();
              for m in root_node.find_all(pattern.clone()) {
                if let Some(ref hr) = has_rule {
                  if !crate::rules::node_has_descendant(&m, hr.as_ref(), lang) {
                    continue;
                  }
                }
                let _ = tx.send(SymbolHit {
                  name: m
                    .get_env()
                    .get_match("NAME")
                    .map(|n| n.text().to_string())
                    .unwrap_or_else(|| pm.short_label()),
                  kind: m.kind().to_string(),
                  lang: format!("{:?}", lang),
                  file_path: path.to_str().unwrap_or("").to_string(),
                  line_start: (m.start_pos().line() + 1) as u32,
                  content: m.text().to_string(),
                  ..Default::default()
                });
              }
            } else if entry_is_file {
              scan_stats.skipped();
            }
          }
        }
      }
      ignore::WalkState::Continue
    })
  });
  drop(tx);
  let mut results: Vec<_> = rx.into_iter().collect();
  results.sort_by(|a, b| a.file_path.cmp(&b.file_path));
  let mut diagnostics = diagnostics;
  diagnostics.extend(too_large.lock().unwrap().drain(..));
  if results.is_empty() && pm.language == SgLang::Python {
    if let Some(alternate) = python_typed_def_pattern_alternate(&pm.source) {
      let mut alt_pm = pm.clone();
      alt_pm.source = alternate;
      let (alt_hits, alt_diags, alt_stats) = search_pattern_mode(root, &alt_pm, constraints, opts)?;
      diagnostics.extend(alt_diags);
      if !alt_hits.is_empty() {
        diagnostics.push(
          Diagnostic::new(
            "PYTHON_TYPED_DEF_PATTERN_FALLBACK",
            "retried Python def pattern with a return-annotation metavariable",
          )
          .with_hint("for annotated Python defs, use `def name($$$) -> $RET: $$$`"),
        );
        return Ok((alt_hits, diagnostics, alt_stats));
      }
    }
  }
  Ok((results, diagnostics, scan_stats.snapshot()))
}

fn python_typed_def_pattern_alternate(source: &str) -> Option<String> {
  let def_idx = source.find("def ")?;
  let colon = python_def_header_colon(source, def_idx)?;
  let header = &source[..colon];
  if header.contains("->") || !header.contains('(') || !header.contains(')') {
    return None;
  }
  let mut alternate = String::with_capacity(source.len() + " -> $RET".len());
  alternate.push_str(&source[..colon]);
  alternate.push_str(" -> $RET");
  alternate.push_str(&source[colon..]);
  Some(alternate)
}

fn python_def_header_colon(source: &str, def_idx: usize) -> Option<usize> {
  let mut depth = 0usize;
  for (offset, ch) in source[def_idx..].char_indices() {
    match ch {
      '(' => depth += 1,
      ')' => depth = depth.saturating_sub(1),
      ':' if depth == 0 => return Some(def_idx + offset),
      _ => {}
    }
  }
  None
}

impl PatternMode {
  fn short_label(&self) -> String {
    // Hint label when the pattern doesn't bind $NAME — first 32
    // chars of the pattern source, single-line.
    let line: String = self
      .source
      .chars()
      .take_while(|c| *c != '\n')
      .take(32)
      .collect();
    line
  }
}

pub fn search_symbol_hits(
  root: &Path,
  outline: bool,
  constraints: SearchConstraints,
  opts: &WalkOpts,
) -> Result<(Vec<SymbolHit>, Vec<Diagnostic>)> {
  let (hits, diagnostics, _) = search_symbol_hits_with_stats(root, outline, constraints, opts)?;
  Ok((hits, diagnostics))
}

fn search_symbol_hits_with_stats(
  root: &Path,
  outline: bool,
  constraints: SearchConstraints,
  opts: &WalkOpts,
) -> Result<(Vec<SymbolHit>, Vec<Diagnostic>, ScanStats)> {
  // §1.1 — pattern-mode short-circuit. When the caller supplied a
  // PatternMode payload we run a single language's parser instead
  // of the multi-language kind walk.
  if let Some(ref pm) = constraints.pattern {
    return search_pattern_mode(root, pm, &constraints, opts);
  }

  let (tx, rx) = mpsc::channel();
  // §1.2 / Reviewer must-fix #2 — diagnostics flow through the
  // envelope, not just stderr. Per-language rule-parse failures
  // are deduped by lang name in this map.
  let parse_fail_map: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
  // §1.5 — per-file FILE_TOO_LARGE diagnostics from walk_pre_check.
  let too_large: Arc<Mutex<Vec<Diagnostic>>> = Arc::new(Mutex::new(Vec::new()));
  let scan_stats = Arc::new(ScanStatsCounters::default());
  let query_lower = constraints.query.as_ref().map(|q| q.to_lowercase());
  let dec_lower = constraints
    .has_decoration
    .as_ref()
    .map(|d| d.to_lowercase());
  let child_lower = constraints.has_child.as_ref().map(|c| c.to_lowercase());

  let mut wb = WalkBuilder::new(root);
  apply_walk_opts(&mut wb, opts, root);
  let walker = wb.build_parallel();
  walker.run(|| {
    let tx = tx.clone();
    let parse_fail_map = parse_fail_map.clone();
    let too_large = too_large.clone();
    let scan_stats = scan_stats.clone();
    let query_lower = query_lower.clone();
    let dec_lower = dec_lower.clone();
    let child_lower = child_lower.clone();
    let constraints = &constraints;
    let opts = opts;
    Box::new(move |result| {
      if let Ok(entry) = result {
        let entry_path = entry.path();
        let entry_is_file = entry_path.is_file();
        if entry_is_file {
          scan_stats.considered();
        }
        let (path_buf, lang) = match walk_pre_check(&entry, opts) {
          PreCheck::Process(p, l) => (p, l),
          PreCheck::Skip => {
            if entry_is_file {
              scan_stats.skipped();
            }
            return ignore::WalkState::Continue;
          }
          PreCheck::SkipWithDiag(d) => {
            if entry_is_file {
              scan_stats.skipped();
            }
            too_large.lock().unwrap().push(d);
            return ignore::WalkState::Continue;
          }
        };
        let path = path_buf.as_path();
        {
          {
            // §1.3 — `--scope code` skips data-language files.
            if constraints.scope_code_only && !lmprobe_lang_core::langs::is_code_lang(lang) {
              if entry_is_file {
                scan_stats.skipped();
              }
              return ignore::WalkState::Continue;
            }
            if let Ok(content) = fs::read_to_string(path) {
              scan_stats.scanned();
              if let Some(ref q) = query_lower {
                if !content.to_lowercase().contains(q) {
                  return ignore::WalkState::Continue;
                }
              }
              if let Some(ref d) = dec_lower {
                if !content.to_lowercase().contains(d) {
                  return ignore::WalkState::Continue;
                }
              }
              if let Some(ref c) = child_lower {
                if !content.to_lowercase().contains(c) {
                  return ignore::WalkState::Continue;
                }
              }

              // §1.4 — kind selection priority:
              //  1. --raw-kind  (tree-sitter literal)
              //  2. --kind      (portable taxonomy lookup;
              //     falls back to literal if not portable)
              //  3. taxonomy_for(lang).all_declaration_kinds()
              let kinds: Vec<&str> = if let Some(rk) = constraints.raw_kind.as_deref() {
                vec![rk]
              } else if let Some(k) = constraints.kind.as_deref() {
                use lmprobe_lang_core::langs::PortableKind;
                if let Some(p) = PortableKind::parse(k) {
                  let mapped = taxonomy_for(lang).kind_for(p);
                  if mapped.is_empty() {
                    return ignore::WalkState::Continue;
                  }
                  mapped.iter().copied().collect()
                } else {
                  vec![k]
                }
              } else {
                taxonomy_for(lang).all_declaration_kinds()
              };
              // §1.4 PUSH-BACK 1 — DFS-once kind probe.
              // Replaces N `find_all(KindMatcher)` walks
              // with a single tree traversal + HashSet
              // lookup. Cuts search/def's per-file cost
              // by ~Nx (N = portable kinds for the lang).
              let kind_set: std::collections::HashSet<&str> = kinds.iter().copied().collect();

              let grep = lang.ast_grep(content);
              let root_node = grep.root();
              let extractor = name_extractor_for(lang);

              // §1.2 — pre-build the relational `has` rule
              // once per file (it's language-bound).
              let has_rule_obj: Option<std::result::Result<ast_grep_config::Rule, ()>> =
                if let Some(ref yaml) = constraints.has_rule {
                  match crate::rules::build_has_rule(yaml, lang) {
                    Ok(r) => Some(Ok(r)),
                    Err(e) => {
                      // §1.2 PUSH-BACK 2 — funnel
                      // RULE_PARSE_FAILED into the
                      // envelope's warnings via the
                      // shared dedup map.
                      parse_fail_map
                        .lock()
                        .unwrap()
                        .entry(format!("{:?}", lang))
                        .or_insert(e);
                      Some(Err(()))
                    }
                  }
                } else {
                  None
                };

              for n in root_node.dfs() {
                let nk_owned: String = n.kind().to_string();
                if !kind_set.contains(nk_owned.as_str()) {
                  continue;
                }
                let m = ast_grep_core::NodeMatch::from(n);
                let nk = nk_owned.as_str();
                // §0.5 — D5 NameExtractor trait dispatch.
                let name = extractor
                  .extract(&m)
                  .unwrap_or_else(|| "unknown".to_string());
                if let Some(ref q) = query_lower {
                  let matches_query = if constraints.exact_name {
                    // §0.6: anchored to full identifier
                    // (case-sensitive). Closes QA §2.7.
                    constraints
                      .query
                      .as_ref()
                      .map(|raw| name == *raw)
                      .unwrap_or(false)
                  } else {
                    name.to_lowercase().contains(q)
                  };
                  if !matches_query {
                    continue;
                  }
                }
                if let Some(ref d) = dec_lower {
                  let mut has_dec = false;
                  if let Some(parent) = m.parent() {
                    if parent.dfs().any(|nn| {
                      (nn.kind().contains("annotation") || nn.kind().contains("decorator"))
                        && nn.text().to_lowercase().contains(d)
                    }) {
                      has_dec = true;
                    }
                  }
                  if !has_dec
                    && m.dfs().any(|nn| {
                      (nn.kind().contains("annotation") || nn.kind().contains("decorator"))
                        && nn.text().to_lowercase().contains(d)
                    })
                  {
                    has_dec = true;
                  }
                  if !has_dec {
                    continue;
                  }
                }
                if let Some(ref c) = child_lower {
                  let data_child_kinds: &[&str] =
                    lmprobe_registry::top_level_kinds_for(lang).child_kinds();
                  if !m.dfs().any(|nn| {
                    let k = nn.kind();
                    let is_code_child = k.contains("method")
                      || k.contains("function")
                      || k.contains("field")
                      || k.contains("declaration");
                    let is_data_child = data_child_kinds.iter().any(|dk| *dk == k);
                    (is_code_child || is_data_child) && nn.text().to_lowercase().contains(c)
                  }) {
                    continue;
                  }
                }
                if let Some(ref a) = constraints.has_attr {
                  if !m.text().contains(a) {
                    continue;
                  }
                }
                let enclosing = if constraints.class_filter.is_some() {
                  enclosing_decl_name(&m, lang)
                } else {
                  None
                };
                if let Some(ref class_filter) = constraints.class_filter {
                  if !enclosing.as_ref().is_some_and(|(enclosing_name, kind)| {
                    enclosing_name == class_filter && is_container_decl_kind(kind)
                  }) {
                    continue;
                  }
                }
                // §1.2 relational `has` filter.
                if let Some(ref hr) = has_rule_obj {
                  match hr {
                    Ok(rule) => {
                      if !crate::rules::node_has_descendant(&m, rule, lang) {
                        continue;
                      }
                    }
                    Err(()) => continue,
                  }
                }
                let mut content_str = m.text().to_string();
                if outline {
                  content_str = create_skeleton(&m);
                }
                let (enclosing_decl, enclosing_decl_kind) = enclosing
                  .map(|(name, kind)| (Some(name), Some(kind)))
                  .unwrap_or((None, None));
                let _ = tx.send(SymbolHit {
                  name,
                  kind: nk.to_string(),
                  lang: format!("{:?}", lang),
                  file_path: path.to_str().unwrap_or("").to_string(),
                  line_start: (m.start_pos().line() + 1) as u32,
                  content: content_str,
                  enclosing_decl,
                  enclosing_decl_kind,
                  ..Default::default()
                });
              }
            } else if entry_is_file {
              scan_stats.skipped();
            }
          }
        }
      }
      ignore::WalkState::Continue
    })
  });
  drop(tx);
  let mut results: Vec<_> = rx.into_iter().collect();
  results.sort_by(|a, b| a.file_path.cmp(&b.file_path));
  // Dedup by (file, line, kind, name) — DFS visits both descendant
  // and ancestor nodes when kinds nest, occasionally yielding the
  // same logical hit twice. Cheap to drop here.
  results.dedup_by(|a, b| {
    a.file_path == b.file_path
      && a.line_start == b.line_start
      && a.kind == b.kind
      && a.name == b.name
  });

  let mut diagnostics: Vec<Diagnostic> = Vec::new();
  for (lang_name, msg) in parse_fail_map.lock().unwrap().drain() {
    diagnostics.push(
      Diagnostic::new(
        "RULE_PARSE_FAILED",
        format!("relational rule failed to parse: {}", msg),
      )
      .with_lang(lang_name),
    );
  }
  diagnostics.extend(too_large.lock().unwrap().drain(..));
  Ok((results, diagnostics, scan_stats.snapshot()))
}

// ──────────────────────────── discover ────────────────────────────

#[derive(Deserialize)]
struct CargoToml {
  package: Option<CargoPackage>,
  dependencies: Option<HashMap<String, toml::Value>>,
}
#[derive(Deserialize)]
struct CargoPackage {
  name: String,
  version: String,
}
#[derive(Deserialize)]
struct PackageJson {
  name: Option<String>,
  version: Option<String>,
  dependencies: Option<HashMap<String, String>>,
  #[serde(rename = "devDependencies")]
  dev_dependencies: Option<HashMap<String, String>>,
}
#[derive(Deserialize)]
struct PyProjectToml {
  project: Option<PyProjectProject>,
  #[serde(rename = "build-system")]
  build_system: Option<PyProjectBuildSystem>,
  tool: Option<HashMap<String, toml::Value>>,
}
#[derive(Deserialize)]
struct PyProjectProject {
  name: Option<String>,
  version: Option<String>,
  dependencies: Option<Vec<String>>,
}
#[derive(Deserialize)]
struct PyProjectBuildSystem {
  requires: Option<Vec<String>>,
}

pub fn discover(
  root: &Path,
  opts: &WalkOpts,
  depth: Option<usize>,
  top_level_only: bool,
) -> Result<DiscoverResult> {
  let mut wb = WalkBuilder::new(root);
  wb.hidden(false);
  if let Some(d) = depth {
    wb.max_depth(Some(d));
  }
  apply_walk_opts(&mut wb, opts, root);
  let walker = wb.build();
  let mut projects: Vec<Project> = Vec::new();
  let mut ansible_dirs: std::collections::HashSet<String> = std::collections::HashSet::new();

  for entry in walker {
    let entry = entry?;
    let path = entry.path();
    let file_name_str = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let parent_str = path
      .parent()
      .and_then(|p| p.to_str())
      .unwrap_or(".")
      .to_string();

    let project = match file_name_str {
      // §0.8 set.
      "Cargo.toml" => parse_cargo(path, &parent_str),
      "package.json" => parse_npm(path, &parent_str),
      "pom.xml" => Some(parse_maven(path, &parent_str)),
      "build.gradle" | "build.gradle.kts" => Some(parse_gradle(path, file_name_str, &parent_str)),
      "go.mod" => parse_go_mod(path, &parent_str),
      "pyproject.toml" => parse_pyproject(path, &parent_str),
      "setup.py" => Some(parse_setup_py(path, &parent_str)),
      "Gemfile" => Some(parse_gemfile(path, &parent_str)),
      // §2.1 net-new markers.
      "build.xml" => Some(parse_ant(path, &parent_str)),
      "Makefile" | "GNUmakefile" => Some(parse_make(path, &parent_str)),
      "CMakeLists.txt" => Some(parse_cmake(path, &parent_str)),
      "requirements.txt" => Some(parse_requirements(path, &parent_str)),
      "composer.json" => parse_composer(path, &parent_str),
      "BUILD.bazel" | "BUILD" => Some(parse_bazel(path, &parent_str)),
      "Chart.yaml" => Some(parse_helm(path, &parent_str)),
      "Package.swift" => Some(parse_swiftpm(path, &parent_str)),
      "flake.nix" | "default.nix" => Some(parse_nix(file_name_str, path, &parent_str)),
      "build.sbt" => Some(parse_sbt(path, &parent_str)),
      // §2C addendum (Reviewer issue 1) — markers missed by 2A.
      "pubspec.yaml" => Some(parse_pubspec(path, &parent_str)),
      "mix.exs" => Some(parse_mix(path, &parent_str)),
      // Terraform: `versions.tf` is the conventional pinned-
      // version file. Other `.tf` files don't fire — document.
      "versions.tf" => Some(parse_terraform(path, &parent_str)),
      // Lua/Neovim: `init.lua` is the canonical entry-point.
      // A repo can have multiple (root + sub-module inits).
      "init.lua" => Some(parse_initlua(&parent_str)),
      // Static-web: `index.html` per directory — same model as
      // init.lua. Picks up project-per-subdir layouts like
      // 50projects50days or any tutorial corpus.
      "index.html" => Some(parse_index_html(&parent_str)),
      // Ansible — `site.yml` adjacent to a `roles/` dir.
      "site.yml" | "playbook.yml" => {
        let parent_path = path.parent().unwrap_or(Path::new("."));
        if parent_path.join("roles").is_dir() && ansible_dirs.insert(parent_str.clone()) {
          Some(parse_ansible(file_name_str, &parent_str))
        } else {
          None
        }
      }
      _ => {
        if file_name_str.ends_with(".csproj") {
          Some(parse_csproj(path, file_name_str, &parent_str))
        } else if file_name_str.ends_with(".sln") {
          Some(parse_sln(file_name_str, &parent_str))
        } else if file_name_str.ends_with(".gemspec") {
          Some(parse_gemspec(path, file_name_str, &parent_str))
        } else if file_name_str.ends_with(".rockspec") {
          Some(parse_rockspec(path, file_name_str, &parent_str))
        } else {
          None
        }
      }
    };
    if let Some(p) = project {
      projects.push(p);
    }
  }

  if projects.is_empty() {
    if let Some(project) = discover_go_subpackage(root) {
      projects.push(project);
    }
  }

  // Sort by path so parents come before deeper children.
  projects.sort_by(|a, b| a.path.cmp(&b.path));

  // Compute parent → children hierarchy via path-prefix containment.
  // O(n²) but n is small (typical projects: <50 markers).
  let paths: Vec<String> = projects.iter().map(|p| p.path.clone()).collect();
  for i in 0..projects.len() {
    let mut kids = Vec::new();
    for (j, child_path) in paths.iter().enumerate() {
      if i == j {
        continue;
      }
      let parent = &projects[i].path;
      // Direct child = strict prefix + first non-prefix char is `/`
      // (or `\` on win) AND no other project sits between.
      if is_direct_child_path(parent, child_path, &paths) {
        kids.push(child_path.clone());
      }
    }
    projects[i].children = kids;
  }

  if top_level_only {
    let child_paths: std::collections::HashSet<String> = projects
      .iter()
      .flat_map(|project| project.children.iter().cloned())
      .collect();
    projects.retain(|project| !child_paths.contains(&project.path));
    for project in &mut projects {
      project.children.clear();
    }
  }

  Ok(DiscoverResult::new(projects))
}

/// True when `child` is a directly-nested project relative to
/// `parent`: `child` starts with `parent + "/"` (or `\\`), and no
/// other project in `all_paths` sits strictly between them.
fn is_direct_child_path(parent: &str, child: &str, all_paths: &[String]) -> bool {
  if child.len() <= parent.len() || !child.starts_with(parent) {
    return false;
  }
  let next = child.as_bytes()[parent.len()];
  if next != b'/' && next != b'\\' {
    return false;
  }
  // Reject if another path is strictly between parent and child.
  for p in all_paths {
    if p == parent || p == child {
      continue;
    }
    if p.starts_with(parent) && child.starts_with(p) && p.len() > parent.len() {
      return false;
    }
  }
  true
}

fn parse_cargo(path: &Path, parent: &str) -> Option<Project> {
  let content = fs::read_to_string(path).ok()?;
  let cargo: CargoToml = toml::from_str(&content).ok()?;
  let pkg = cargo.package?;
  Some(Project {
    name: pkg.name,
    project_type: "Cargo".to_string(),
    version: pkg.version,
    path: parent.to_string(),
    dependencies_count: cargo.dependencies.map(|d| d.len()).unwrap_or(0),
    children: Vec::new(),
  })
}

fn parse_npm(path: &Path, parent: &str) -> Option<Project> {
  let content = fs::read_to_string(path).ok()?;
  let pkg: PackageJson = serde_json::from_str(&content).ok()?;
  Some(Project {
    name: pkg.name.unwrap_or_else(|| "unnamed".to_string()),
    project_type: "Npm".to_string(),
    version: pkg.version.unwrap_or_else(|| "0.0.0".to_string()),
    path: parent.to_string(),
    dependencies_count: pkg.dependencies.as_ref().map(|d| d.len()).unwrap_or(0)
      + pkg.dev_dependencies.as_ref().map(|d| d.len()).unwrap_or(0),
    children: Vec::new(),
  })
}

/// Maven: best-effort parse of `<artifactId>` / `<version>` /
/// `<dependencies>` count via line scan. Avoids pulling in an XML
/// parser dep for Phase 0; Phase 2.1 promotes this to a real parser.
fn parse_maven(path: &Path, parent: &str) -> Project {
  let content = fs::read_to_string(path).unwrap_or_default();
  let extract = |tag: &str| -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let i = content.find(&open)? + open.len();
    let j = content[i..].find(&close)? + i;
    Some(content[i..j].trim().to_string())
  };
  let name = extract("artifactId").unwrap_or_else(|| "unnamed".to_string());
  let version = extract("version").unwrap_or_else(|| "0.0.0".to_string());
  let deps_count = content.matches("<dependency>").count();
  Project {
    name,
    project_type: "Maven".to_string(),
    version,
    path: parent.to_string(),
    dependencies_count: deps_count,
    children: Vec::new(),
  }
}

/// Gradle: line-scan for `dependencies { … }` blocks; pull rootProject
/// name from settings.gradle elsewhere if needed (Phase 2 will refine).
fn parse_gradle(path: &Path, file_name: &str, parent: &str) -> Project {
  let content = fs::read_to_string(path).unwrap_or_default();
  // Count dependency lines: configurations followed by 'group:name:version'.
  let deps_count = content
    .lines()
    .filter(|l| {
      let t = l.trim_start();
      t.starts_with("implementation ")
        || t.starts_with("api ")
        || t.starts_with("compileOnly ")
        || t.starts_with("runtimeOnly ")
        || t.starts_with("testImplementation ")
        || t.starts_with("compile ")
    })
    .count();
  let name = parent.rsplit('/').next().unwrap_or("unnamed").to_string();
  let lang = if file_name.ends_with(".kts") {
    "Kotlin"
  } else {
    "Groovy"
  };
  Project {
    name,
    project_type: format!("Gradle/{}", lang),
    version: "0.0.0".to_string(),
    path: parent.to_string(),
    dependencies_count: deps_count,
    children: Vec::new(),
  }
}

fn parse_go_mod(path: &Path, parent: &str) -> Option<Project> {
  let content = fs::read_to_string(path).ok()?;
  let mut module: Option<String> = None;
  let mut go_version: Option<String> = None;
  let mut deps_count = 0usize;
  let mut in_require_block = false;
  for line in content.lines() {
    let t = line.trim();
    if let Some(rest) = t.strip_prefix("module ") {
      module = Some(rest.trim().trim_matches('"').to_string());
    } else if let Some(rest) = t.strip_prefix("go ") {
      go_version = Some(rest.trim().to_string());
    } else if let Some(rest) = t.strip_prefix("require ") {
      if rest.trim_start().starts_with('(') {
        in_require_block = true;
      } else if !rest.is_empty() {
        deps_count += 1;
      }
    } else if in_require_block {
      if t == ")" {
        in_require_block = false;
      } else if !t.is_empty() && !t.starts_with("//") {
        deps_count += 1;
      }
    }
  }
  Some(Project {
    name: module.unwrap_or_else(|| "unnamed".to_string()),
    project_type: "Go".to_string(),
    version: go_version.unwrap_or_else(|| "0.0.0".to_string()),
    path: parent.to_string(),
    dependencies_count: deps_count,
    children: Vec::new(),
  })
}

fn discover_go_subpackage(root: &Path) -> Option<Project> {
  if !root.is_dir() || !directory_contains_go_files(root) {
    return None;
  }
  let go_mod = find_ancestor_marker(root, "go.mod")?;
  let module = parse_go_mod(&go_mod, go_mod.parent()?.to_str()?)?;
  let package_name = go_package_name(root).unwrap_or_else(|| {
    root
      .file_name()
      .and_then(|name| name.to_str())
      .unwrap_or("go-package")
      .to_string()
  });
  let rel = root
    .strip_prefix(go_mod.parent()?)
    .ok()
    .and_then(|path| path.to_str())
    .filter(|path| !path.is_empty())
    .unwrap_or(".");
  Some(Project {
    name: format!("{}/{}", module.name, rel.replace('\\', "/")),
    project_type: format!("Go/Package({package_name})"),
    version: module.version,
    path: root.to_string_lossy().into_owned(),
    dependencies_count: module.dependencies_count,
    children: Vec::new(),
  })
}

fn directory_contains_go_files(root: &Path) -> bool {
  fs::read_dir(root).ok().into_iter().flatten().any(|entry| {
    entry
      .ok()
      .map(|entry| entry.path())
      .filter(|path| path.is_file())
      .and_then(|path| {
        path
          .extension()
          .and_then(|ext| ext.to_str())
          .map(str::to_string)
      })
      .is_some_and(|ext| ext == "go")
  })
}

fn find_ancestor_marker(start: &Path, marker: &str) -> Option<PathBuf> {
  let mut current = Some(start);
  while let Some(dir) = current {
    let candidate = dir.join(marker);
    if candidate.is_file() {
      return Some(candidate);
    }
    current = dir.parent();
  }
  None
}

fn go_package_name(root: &Path) -> Option<String> {
  let re = Regex::new(r"^\s*package\s+([A-Za-z_][A-Za-z0-9_]*)").ok()?;
  let mut entries = fs::read_dir(root)
    .ok()?
    .filter_map(|entry| entry.ok().map(|entry| entry.path()))
    .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("go"))
    .collect::<Vec<_>>();
  entries.sort();
  for path in entries {
    let content = fs::read_to_string(path).ok()?;
    for line in content.lines() {
      if let Some(caps) = re.captures(line) {
        return caps.get(1).map(|m| m.as_str().to_string());
      }
    }
  }
  None
}

fn parse_pyproject(path: &Path, parent: &str) -> Option<Project> {
  let content = fs::read_to_string(path).ok()?;
  let py: PyProjectToml = toml::from_str(&content).ok()?;
  let project_section = py.project.as_ref();
  let name = project_section
    .and_then(|p| p.name.clone())
    .or_else(|| {
      py.tool
        .as_ref()
        .and_then(|t| t.get("poetry"))
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
    })
    .unwrap_or_else(|| "unnamed".to_string());
  let version = project_section
    .and_then(|p| p.version.clone())
    .unwrap_or_else(|| "0.0.0".to_string());
  let deps_count = project_section
    .and_then(|p| p.dependencies.as_ref())
    .map(|v| v.len())
    .unwrap_or(0)
    + py
      .build_system
      .as_ref()
      .and_then(|b| b.requires.as_ref())
      .map(|v| v.len())
      .unwrap_or(0);
  Some(Project {
    name,
    project_type: "PyProject".to_string(),
    version,
    path: parent.to_string(),
    dependencies_count: deps_count,
    children: Vec::new(),
  })
}

/// `setup.py`: count `install_requires=[…]` entries by line, lift name
/// from `name="…"`. Best-effort; legacy projects vary widely.
fn parse_setup_py(path: &Path, parent: &str) -> Project {
  let content = fs::read_to_string(path).unwrap_or_default();
  let extract_str = |key: &str| -> Option<String> {
    let needle = format!("{}=", key);
    let i = content.find(&needle)? + needle.len();
    let rest = &content[i..];
    let opened = rest.chars().find(|&c| c == '"' || c == '\'')?;
    let after = rest.find(opened)? + 1;
    let close = rest[after..].find(opened)? + after;
    Some(rest[after..close].trim().to_string())
  };
  let name = extract_str("name").unwrap_or_else(|| "unnamed".to_string());
  let version = extract_str("version").unwrap_or_else(|| "0.0.0".to_string());
  // Cheap install_requires count.
  let deps_count = if let Some(idx) = content.find("install_requires") {
    let after = &content[idx..];
    if let Some(open) = after.find('[') {
      let after = &after[open + 1..];
      if let Some(close) = after.find(']') {
        after[..close]
          .split(',')
          .filter(|s| !s.trim().is_empty())
          .count()
      } else {
        0
      }
    } else {
      0
    }
  } else {
    0
  };
  Project {
    name,
    project_type: "SetupPy".to_string(),
    version,
    path: parent.to_string(),
    dependencies_count: deps_count,
    children: Vec::new(),
  }
}

/// `Gemfile`: count `gem '...'` lines.
fn parse_gemfile(path: &Path, parent: &str) -> Project {
  let content = fs::read_to_string(path).unwrap_or_default();
  let deps_count = content
    .lines()
    .filter(|l| l.trim_start().starts_with("gem "))
    .count();
  let name = parent.rsplit('/').next().unwrap_or("unnamed").to_string();
  Project {
    name,
    project_type: "Bundler".to_string(),
    version: "0.0.0".to_string(),
    path: parent.to_string(),
    dependencies_count: deps_count,
    children: Vec::new(),
  }
}

/// `.csproj`: count `<PackageReference …>` lines; lift `<Version>`.
fn parse_csproj(path: &Path, file_name: &str, parent: &str) -> Project {
  let content = fs::read_to_string(path).unwrap_or_default();
  let deps_count = content.matches("<PackageReference").count();
  let name = file_name.trim_end_matches(".csproj").to_string();
  let version = {
    let open = "<Version>";
    let close = "</Version>";
    if let Some(i) = content.find(open) {
      let s = i + open.len();
      if let Some(e) = content[s..].find(close) {
        content[s..s + e].trim().to_string()
      } else {
        "0.0.0".to_string()
      }
    } else {
      "0.0.0".to_string()
    }
  };
  Project {
    name,
    project_type: "DotNetCsproj".to_string(),
    version,
    path: parent.to_string(),
    dependencies_count: deps_count,
    children: Vec::new(),
  }
}

/// `.sln`: high-level solution marker; deps_count = 0 (solution
/// aggregates projects discovered separately).
fn parse_sln(file_name: &str, parent: &str) -> Project {
  Project {
    name: file_name.trim_end_matches(".sln").to_string(),
    project_type: "DotNetSln".to_string(),
    version: "0.0.0".to_string(),
    path: parent.to_string(),
    dependencies_count: 0,
    children: Vec::new(),
  }
}

// ─────────────── §2.1 net-new project-marker parsers ──────────────

/// Ant `build.xml` — pull `<project name="…" basedir="…">`.
fn parse_ant(path: &Path, parent: &str) -> Project {
  let content = fs::read_to_string(path).unwrap_or_default();
  let extract_attr = |attr: &str| -> Option<String> {
    let needle = format!("{}=\"", attr);
    let i = content.find(&needle)? + needle.len();
    let close = content[i..].find('"')? + i;
    Some(content[i..close].trim().to_string())
  };
  Project {
    name: extract_attr("name").unwrap_or_else(|| "unnamed".to_string()),
    project_type: "Ant".to_string(),
    version: "0.0.0".to_string(),
    path: parent.to_string(),
    dependencies_count: content.matches("<dependency").count(),
    children: Vec::new(),
  }
}

fn parse_make(_path: &Path, parent: &str) -> Project {
  Project {
    name: parent.rsplit('/').next().unwrap_or("unnamed").to_string(),
    project_type: "Make".to_string(),
    version: "0.0.0".to_string(),
    path: parent.to_string(),
    dependencies_count: 0,
    children: Vec::new(),
  }
}

/// CMake `CMakeLists.txt` — lift `project(<NAME> ...)` and count
/// `add_executable` / `add_library` targets as deps.
fn parse_cmake(path: &Path, parent: &str) -> Project {
  let content = fs::read_to_string(path).unwrap_or_default();
  let name = content
    .lines()
    .find_map(|l| {
      let t = l.trim_start();
      if let Some(rest) = t
        .strip_prefix("project(")
        .or_else(|| t.strip_prefix("project ("))
      {
        let n = rest
          .split(|c: char| c.is_whitespace() || c == ')' || c == ',')
          .next()
          .unwrap_or("")
          .trim()
          .trim_matches('"');
        if !n.is_empty() {
          return Some(n.to_string());
        }
      }
      None
    })
    .unwrap_or_else(|| parent.rsplit('/').next().unwrap_or("unnamed").to_string());
  let deps_count =
    content.matches("add_executable").count() + content.matches("add_library").count();
  Project {
    name,
    project_type: "CMake".to_string(),
    version: "0.0.0".to_string(),
    path: parent.to_string(),
    dependencies_count: deps_count,
    children: Vec::new(),
  }
}

/// Python `requirements.txt` — count non-blank, non-comment lines.
fn parse_requirements(path: &Path, parent: &str) -> Project {
  let content = fs::read_to_string(path).unwrap_or_default();
  let deps = content
    .lines()
    .filter(|l| {
      let t = l.trim();
      !t.is_empty() && !t.starts_with('#') && !t.starts_with("-r")
    })
    .count();
  Project {
    name: parent.rsplit('/').next().unwrap_or("unnamed").to_string(),
    project_type: "Requirements".to_string(),
    version: "0.0.0".to_string(),
    path: parent.to_string(),
    dependencies_count: deps,
    children: Vec::new(),
  }
}

#[derive(Deserialize)]
struct ComposerJson {
  name: Option<String>,
  version: Option<String>,
  require: Option<HashMap<String, String>>,
  #[serde(rename = "require-dev")]
  require_dev: Option<HashMap<String, String>>,
}

fn parse_composer(path: &Path, parent: &str) -> Option<Project> {
  let content = fs::read_to_string(path).ok()?;
  let cj: ComposerJson = serde_json::from_str(&content).ok()?;
  Some(Project {
    name: cj.name.unwrap_or_else(|| "unnamed".to_string()),
    project_type: "Composer".to_string(),
    version: cj.version.unwrap_or_else(|| "0.0.0".to_string()),
    path: parent.to_string(),
    dependencies_count: cj.require.as_ref().map(|m| m.len()).unwrap_or(0)
      + cj.require_dev.as_ref().map(|m| m.len()).unwrap_or(0),
    children: Vec::new(),
  })
}

/// Bazel `BUILD.bazel` (or bare `BUILD`) — count target rules.
fn parse_bazel(path: &Path, parent: &str) -> Project {
  let content = fs::read_to_string(path).unwrap_or_default();
  // Common rule kinds; not exhaustive but representative.
  let deps = [
    "cc_library(",
    "cc_binary(",
    "rust_library(",
    "rust_binary(",
    "py_library(",
    "py_binary(",
    "java_library(",
    "java_binary(",
    "go_library(",
    "go_binary(",
    "test_suite(",
  ]
  .iter()
  .map(|k| content.matches(k).count())
  .sum();
  Project {
    name: parent.rsplit('/').next().unwrap_or("unnamed").to_string(),
    project_type: "Bazel".to_string(),
    version: "0.0.0".to_string(),
    path: parent.to_string(),
    dependencies_count: deps,
    children: Vec::new(),
  }
}

#[derive(Deserialize)]
struct HelmChart {
  name: Option<String>,
  version: Option<String>,
  dependencies: Option<Vec<HelmDep>>,
}
#[derive(Deserialize)]
struct HelmDep {
  #[allow(dead_code)]
  name: Option<String>,
}

fn parse_helm(path: &Path, parent: &str) -> Project {
  let content = fs::read_to_string(path).unwrap_or_default();
  let chart: Option<HelmChart> = serde_yaml::from_str(&content).ok();
  let chart = chart.unwrap_or(HelmChart {
    name: None,
    version: None,
    dependencies: None,
  });
  Project {
    name: chart.name.unwrap_or_else(|| "unnamed".to_string()),
    project_type: "Helm".to_string(),
    version: chart.version.unwrap_or_else(|| "0.0.0".to_string()),
    path: parent.to_string(),
    dependencies_count: chart.dependencies.as_ref().map(|d| d.len()).unwrap_or(0),
    children: Vec::new(),
  }
}

fn parse_ansible(file_name: &str, parent: &str) -> Project {
  Project {
    name: parent.rsplit('/').next().unwrap_or("unnamed").to_string(),
    project_type: format!("Ansible/{}", file_name.trim_end_matches(".yml")),
    version: "0.0.0".to_string(),
    path: parent.to_string(),
    dependencies_count: 0,
    children: Vec::new(),
  }
}

/// SwiftPM `Package.swift` — lift `name: "…"` and count `.product`
/// / `.target` lines.
fn parse_swiftpm(path: &Path, parent: &str) -> Project {
  let content = fs::read_to_string(path).unwrap_or_default();
  let name = content
    .find("name:")
    .and_then(|i| {
      let rest = &content[i + 5..];
      let opened = rest.chars().find(|&c| c == '"' || c == '\'')?;
      let after = rest.find(opened)? + 1;
      let close = rest[after..].find(opened)? + after;
      Some(rest[after..close].trim().to_string())
    })
    .unwrap_or_else(|| parent.rsplit('/').next().unwrap_or("unnamed").to_string());
  let deps = content.matches(".package(").count() + content.matches(".product(").count();
  Project {
    name,
    project_type: "SwiftPM".to_string(),
    version: "0.0.0".to_string(),
    path: parent.to_string(),
    dependencies_count: deps,
    children: Vec::new(),
  }
}

fn parse_nix(file_name: &str, _path: &Path, parent: &str) -> Project {
  Project {
    name: parent.rsplit('/').next().unwrap_or("unnamed").to_string(),
    project_type: format!("Nix/{}", file_name.trim_end_matches(".nix")),
    version: "0.0.0".to_string(),
    path: parent.to_string(),
    dependencies_count: 0,
    children: Vec::new(),
  }
}

/// sbt `build.sbt` — lift `name := "…"` and `version := "…"`.
fn parse_sbt(path: &Path, parent: &str) -> Project {
  let content = fs::read_to_string(path).unwrap_or_default();
  let extract = |key: &str| -> Option<String> {
    let needle = format!("{} := \"", key);
    let i = content.find(&needle)? + needle.len();
    let close = content[i..].find('"')? + i;
    Some(content[i..close].trim().to_string())
  };
  Project {
    name: extract("name")
      .unwrap_or_else(|| parent.rsplit('/').next().unwrap_or("unnamed").to_string()),
    project_type: "Sbt".to_string(),
    version: extract("version").unwrap_or_else(|| "0.0.0".to_string()),
    path: parent.to_string(),
    dependencies_count: content.matches("libraryDependencies").count(),
    children: Vec::new(),
  }
}

/// Ruby `*.gemspec` — line scan for `s.name`, `s.version`,
/// `add_dependency`/`add_runtime_dependency`.
fn parse_gemspec(path: &Path, file_name: &str, parent: &str) -> Project {
  let content = fs::read_to_string(path).unwrap_or_default();
  let extract = |attr: &str| -> Option<String> {
    let needle = format!(".{} =", attr);
    let i = content.find(&needle)? + needle.len();
    let rest = &content[i..];
    let opened = rest.chars().find(|&c| c == '"' || c == '\'')?;
    let after = rest.find(opened)? + 1;
    let close = rest[after..].find(opened)? + after;
    Some(rest[after..close].trim().to_string())
  };
  Project {
    name: extract("name").unwrap_or_else(|| file_name.trim_end_matches(".gemspec").to_string()),
    project_type: "Gemspec".to_string(),
    version: extract("version").unwrap_or_else(|| "0.0.0".to_string()),
    path: parent.to_string(),
    dependencies_count: content.matches("add_dependency").count()
      + content.matches("add_runtime_dependency").count()
      + content.matches("add_development_dependency").count(),
    children: Vec::new(),
  }
}

/// Lua `*.rockspec` — pull `package = "…"` / `version = "…"`.
/// Static-web `index.html` — name from parent dir.
fn parse_index_html(parent: &str) -> Project {
  Project {
    name: parent
      .rsplit('/')
      .next()
      .filter(|s| !s.is_empty() && *s != ".")
      .unwrap_or("web")
      .to_string(),
    project_type: "Web/Static".to_string(),
    version: "0.0.0".to_string(),
    path: parent.to_string(),
    dependencies_count: 0,
    children: Vec::new(),
  }
}

/// Lua/Neovim `init.lua` — no metadata format, name from parent dir.
fn parse_initlua(parent: &str) -> Project {
  Project {
    name: parent
      .rsplit('/')
      .next()
      .filter(|s| !s.is_empty() && *s != ".")
      .unwrap_or("init")
      .to_string(),
    project_type: "Lua/Neovim".to_string(),
    version: "0.0.0".to_string(),
    path: parent.to_string(),
    dependencies_count: 0,
    children: Vec::new(),
  }
}

/// Dart / Flutter `pubspec.yaml` — top-level `name:` and `version:`.
fn parse_pubspec(path: &Path, parent: &str) -> Project {
  let content = fs::read_to_string(path).unwrap_or_default();
  let extract = |key: &str| -> Option<String> {
    for line in content.lines() {
      if let Some(rest) = line.strip_prefix(key) {
        if let Some(v) = rest.strip_prefix(':') {
          return Some(v.trim().trim_matches('"').trim_matches('\'').to_string());
        }
      }
    }
    None
  };
  Project {
    name: extract("name").unwrap_or_else(|| "unnamed".to_string()),
    project_type: "Dart/Pub".to_string(),
    version: extract("version").unwrap_or_else(|| "0.0.0".to_string()),
    path: parent.to_string(),
    dependencies_count: content.matches("\n  ").count().max(0),
    children: Vec::new(),
  }
}

/// Elixir `mix.exs` — defmodule … do; the project name lives in
/// `app: :name` and the version in `version: "..."`.
fn parse_mix(path: &Path, parent: &str) -> Project {
  let content = fs::read_to_string(path).unwrap_or_default();
  let extract_atom = |key: &str| -> Option<String> {
    let needle = format!("{}: :", key);
    let i = content.find(&needle)? + needle.len();
    let end = content[i..]
      .find(|c: char| !c.is_alphanumeric() && c != '_')
      .map(|n| n + i)
      .unwrap_or(content.len());
    Some(content[i..end].to_string())
  };
  let extract_str = |key: &str| -> Option<String> {
    let needle = format!("{}: \"", key);
    let i = content.find(&needle)? + needle.len();
    let close = content[i..].find('"')? + i;
    Some(content[i..close].to_string())
  };
  Project {
    name: extract_atom("app").unwrap_or_else(|| "unnamed".to_string()),
    project_type: "Elixir/Mix".to_string(),
    version: extract_str("version").unwrap_or_else(|| "0.0.0".to_string()),
    path: parent.to_string(),
    dependencies_count: content.matches(":hex").count() + content.matches("{:").count(),
    children: Vec::new(),
  }
}

/// Terraform `versions.tf` — the conventional file declaring the
/// `terraform { required_version = ... }` block. Modules without
/// versions.tf aren't picked up; documented in plan_corrections.md.
fn parse_terraform(path: &Path, parent: &str) -> Project {
  let content = fs::read_to_string(path).unwrap_or_default();
  let extract = |key: &str| -> Option<String> {
    let needle = format!("{} = \"", key);
    let i = content.find(&needle)? + needle.len();
    let close = content[i..].find('"')? + i;
    Some(content[i..close].to_string())
  };
  let module_name = std::path::Path::new(parent)
    .file_name()
    .and_then(|n| n.to_str())
    .unwrap_or("terraform-module")
    .to_string();
  Project {
    name: module_name,
    project_type: "Terraform".to_string(),
    version: extract("required_version").unwrap_or_else(|| "0.0.0".to_string()),
    path: parent.to_string(),
    dependencies_count: content.matches("required_providers").count(),
    children: Vec::new(),
  }
}

fn parse_rockspec(path: &Path, file_name: &str, parent: &str) -> Project {
  let content = fs::read_to_string(path).unwrap_or_default();
  let extract = |key: &str| -> Option<String> {
    let needle = format!("{} = \"", key);
    let i = content.find(&needle)? + needle.len();
    let close = content[i..].find('"')? + i;
    Some(content[i..close].trim().to_string())
  };
  Project {
    name: extract("package").unwrap_or_else(|| {
      file_name
        .trim_end_matches(".rockspec")
        .split('-')
        .next()
        .unwrap_or("unnamed")
        .to_string()
    }),
    project_type: "Rockspec".to_string(),
    version: extract("version").unwrap_or_else(|| "0.0.0".to_string()),
    path: parent.to_string(),
    dependencies_count: content.matches("\"luarocks").count(),
    children: Vec::new(),
  }
}

// ────────────────────────────── find ──────────────────────────────

fn regex_parse_diagnostic(pattern: &str, err: &regex::Error) -> Diagnostic {
  let message = match err {
    regex::Error::Syntax(text) => {
      format!("regex pattern failed to parse:\n{text}")
    }
    regex::Error::CompiledTooBig(limit) => {
      format!("regex pattern compiled too large; size limit is {limit} bytes")
    }
    _ => format!("regex pattern failed to parse: {err}"),
  };
  Diagnostic::new("REGEX_PARSE_FAILED", message).with_hint(regex_parse_hint(pattern))
}

fn regex_parse_hint(pattern: &str) -> &'static str {
  if pattern.contains(r"\\(") || pattern.contains(r"\\)") {
    "Rust regex uses `\\(` or `\\)` for literal parentheses; this pattern contains a double backslash before a parenthesis, so remove one backslash if you meant a literal parenthesis"
  } else {
    "use Rust regex syntax; escape literal metacharacters with one backslash, for example `\\(` for a literal `(`, and use bare `|` for alternation"
  }
}

pub fn find(pattern: &str, root: &Path, opts: &WalkOpts) -> Result<FindResult> {
  find_many(pattern, &[root.to_path_buf()], opts)
}

pub fn find_many(pattern: &str, roots: &[PathBuf], opts: &WalkOpts) -> Result<FindResult> {
  find_many_with_age(pattern, roots, opts, None)
}

pub fn find_many_with_age(
  pattern: &str,
  roots: &[PathBuf],
  opts: &WalkOpts,
  age: Option<std::time::Duration>,
) -> Result<FindResult> {
  find_many_with_predicates(pattern, roots, opts, age, None, None)
}

pub fn find_many_with_predicates(
  pattern: &str,
  roots: &[PathBuf],
  opts: &WalkOpts,
  age: Option<std::time::Duration>,
  filter: Option<&lmprobe_types::schema::FilePredicateInput>,
  prune: Option<&lmprobe_types::schema::FilePredicateInput>,
) -> Result<FindResult> {
  use crate::file_predicate::FilePredicate;
  let compile = |input: Option<&lmprobe_types::schema::FilePredicateInput>| {
    input
      .map(FilePredicate::compile)
      .transpose()
      .map(|value| value.map(Arc::new))
  };
  let predicates = compile(filter).and_then(|filter| compile(prune).map(|prune| (filter, prune)));
  let (filter, prune) = match predicates {
    Ok(predicates) if !(opts.quick_project_roots && predicates.1.is_some()) => predicates,
    result => {
      let message = match result {
        Err(message) => message,
        Ok(_) => "prune cannot combine with quick-projects; each controls traversal roots".into(),
      };
      let mut env = FindResult::new(Vec::new());
      env.exit_status = ExitStatus::Error;
      env.push_warning(Diagnostic::new("FILE_PREDICATE_INVALID", message));
      return Ok(env);
    }
  };
  let re = match Regex::new(pattern) {
    Ok(re) => re,
    Err(err) => {
      let mut env = FindResult::new(Vec::new());
      env.exit_status = ExitStatus::Error;
      env.push_warning(regex_parse_diagnostic(pattern, &err));
      return Ok(env);
    }
  };
  let roots = narrow_quick_project_roots(roots, opts)?;
  let mut paths = Vec::new();
  let mut warnings = Vec::new();
  for root in &roots {
    let (found, diagnostics) = find_paths(&re, root, opts, filter.clone(), prune.clone())?;
    paths.extend(found);
    warnings.extend(diagnostics);
  }
  rank_find_paths(pattern, &mut paths);
  if age.is_none() {
    if let Some(limit) = opts.result_limit {
      paths.truncate(limit);
    }
  }
  let now = std::time::SystemTime::now();
  let mut files = Vec::new();
  paths.retain(
    |path| match crate::file_actions::snapshot(Path::new(path)) {
      Ok(file) => {
        if age.is_some() && file.modified_at.is_none() {
          warnings.push(Diagnostic::new(
            "FILE_METADATA_FAILED",
            format!("{path}: modification time is unavailable"),
          ));
          return false;
        }
        if age.is_some_and(|age| !crate::file_actions::older_than(&file, age, now)) {
          return false;
        }
        files.push(file);
        true
      }
      Err(error) => {
        warnings.push(Diagnostic::new(
          "FILE_METADATA_FAILED",
          format!("{path}: {error}"),
        ));
        age.is_none()
      }
    },
  );
  if let Some(limit) = opts.result_limit {
    paths.truncate(limit);
    let selected: HashSet<_> = paths.iter().map(String::as_str).collect();
    files.retain(|file| selected.contains(file.path.as_str()));
  }
  let mut env = FindResult::new(paths);
  env.files = files;
  env.partial = warnings
    .iter()
    .any(|warning| matches!(warning.code.as_str(), "WALK_ERROR" | "FILE_METADATA_FAILED"));
  env.warnings = warnings;
  if let Some(d) = posix_pipe_alternation_warning(pattern, env.paths.is_empty()) {
    env.push_warning(d);
  }
  if let Some(d) = gitignore_excluded_warning(
    roots
      .iter()
      .map(|root| count_gitignore_excluded_with_prune(root, opts, prune.as_ref()))
      .sum(),
  ) {
    env.push_warning(d);
  }
  Ok(env)
}

fn narrow_quick_project_roots(roots: &[PathBuf], opts: &WalkOpts) -> Result<Vec<PathBuf>> {
  if !opts.quick_project_roots {
    return Ok(roots.to_vec());
  }

  let mut candidates = Vec::new();
  for root in roots {
    if root.is_file() {
      candidates.push(root.clone());
      continue;
    }
    if root.is_dir() {
      let before = candidates.len();
      collect_quick_project_roots(root, opts, &mut candidates)?;
      if candidates.len() == before {
        candidates.push(root.clone());
      }
      continue;
    }
    candidates.push(root.clone());
  }

  if candidates.is_empty() {
    return Ok(roots.to_vec());
  }
  Ok(dedup_project_roots(candidates))
}

fn collect_quick_project_roots(
  root: &Path,
  opts: &WalkOpts,
  candidates: &mut Vec<PathBuf>,
) -> Result<()> {
  let marker_candidates: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));
  let mut perl_counts: HashMap<PathBuf, usize> = HashMap::new();
  let mut wb = WalkBuilder::new(root);
  apply_walk_opts(&mut wb, opts, root);
  wb.max_depth(Some(QUICK_PROJECT_DISCOVERY_MAX_DEPTH));
  {
    let marker_candidates = marker_candidates.clone();
    wb.filter_entry(move |entry| {
      let path = entry.path();
      if !path.is_dir() {
        return true;
      }
      if quick_project_marker_dir(path) {
        marker_candidates.lock().unwrap().push(path.to_path_buf());
        return false;
      }
      true
    });
  }
  for entry in wb.build() {
    let entry = entry.context("quick-project prescan failed; selection is incomplete")?;
    let path = entry.path();
    if !path.is_file() {
      continue;
    }
    if is_perl_path(path) {
      if let Some(parent) = path.parent() {
        *perl_counts.entry(parent.to_path_buf()).or_default() += 1;
      }
    }
  }

  for (dir, count) in perl_counts {
    if count >= QUICK_PERL_DENSE_FILE_THRESHOLD {
      candidates.push(dir);
    }
  }
  candidates.extend(std::mem::take(&mut *marker_candidates.lock().unwrap()));
  Ok(())
}

fn quick_project_marker_dir(path: &Path) -> bool {
  QUICK_PROJECT_MARKERS
    .iter()
    .any(|marker| path.join(marker).is_file())
}

fn is_perl_path(path: &Path) -> bool {
  path
    .extension()
    .and_then(|ext| ext.to_str())
    .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "pl" | "pm"))
}

fn dedup_project_roots(mut roots: Vec<PathBuf>) -> Vec<PathBuf> {
  roots.sort_by(|a, b| {
    path_sort_key(a)
      .cmp(&path_sort_key(b))
      .then_with(|| a.components().count().cmp(&b.components().count()))
  });
  roots.dedup();

  let mut deduped: Vec<PathBuf> = Vec::new();
  for root in roots {
    if deduped
      .iter()
      .any(|ancestor| root != *ancestor && root.starts_with(ancestor))
    {
      continue;
    }
    deduped.retain(|existing| !(existing != &root && existing.starts_with(&root)));
    deduped.push(root);
  }
  deduped
}

fn path_sort_key(path: &Path) -> String {
  path.to_string_lossy().replace('\\', "/")
}

fn rank_find_paths(pattern: &str, paths: &mut [String]) {
  let pattern_tokens = find_rank_tokens(pattern);
  paths.sort_by_cached_key(|path| (Reverse(find_path_rank(&pattern_tokens, path)), path.clone()));
}

fn find_rank_tokens(pattern: &str) -> Vec<String> {
  pattern
    .split(|c: char| !c.is_ascii_alphanumeric())
    .filter(|token| token.len() >= 2)
    .map(|token| token.to_ascii_lowercase())
    .collect()
}

fn find_path_rank(pattern_tokens: &[String], path: &str) -> i32 {
  let components: Vec<String> = path
    .split(['/', '\\'])
    .filter(|component| !component.is_empty())
    .map(|component| component.to_ascii_lowercase())
    .collect();
  let path_tokens: Vec<String> = components
    .iter()
    .flat_map(|component| {
      component
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_string())
    })
    .collect();
  let mut score = 0;

  for token in pattern_tokens {
    if path_tokens.iter().any(|path_token| path_token == token) {
      score += 12;
    } else if components.iter().any(|component| component.contains(token)) {
      score += 3;
    }
  }

  if components
    .iter()
    .any(|component| is_source_component(component))
  {
    score += 20;
  }
  if components
    .windows(2)
    .any(|pair| pair[0] == "src" && pair[1] == "main")
  {
    score += 20;
  }

  if !pattern_tokens
    .iter()
    .any(|token| is_low_signal_component(token))
  {
    for component in &components {
      if is_low_signal_component(component) {
        score -= 40;
      }
    }
  }

  score
}

fn is_source_component(component: &str) -> bool {
  matches!(
    component,
    "src" | "source" | "sources" | "lib" | "libs" | "pkg" | "packages" | "app" | "core"
  )
}

fn is_low_signal_component(component: &str) -> bool {
  matches!(
    component,
    "test"
      | "tests"
      | "spec"
      | "specs"
      | "doc"
      | "docs"
      | "documentation"
      | "fixture"
      | "fixtures"
      | "sample"
      | "samples"
      | "example"
      | "examples"
      | "__tests__"
  )
}

fn find_paths(
  re: &Regex,
  root: &Path,
  opts: &WalkOpts,
  filter: Option<Arc<crate::file_predicate::FilePredicate>>,
  prune: Option<Arc<crate::file_predicate::FilePredicate>>,
) -> Result<(Vec<String>, Vec<Diagnostic>)> {
  let (tx, rx) = mpsc::channel();
  let warnings = Arc::new(Mutex::new(Vec::new()));
  let mut wb = WalkBuilder::new(root);
  apply_walk_opts(&mut wb, opts, root);
  let walker = wb.build_parallel();
  walker.run(|| {
    let tx = tx.clone();
    let re = re.clone();
    let warnings = warnings.clone();
    let filter = filter.clone();
    let prune = prune.clone();
    Box::new(move |result| {
      match result {
        Ok(entry) => {
          let kind = match entry.file_type() {
            Some(kind) => kind,
            None => return ignore::WalkState::Continue,
          };
          let stop = if kind.is_dir() {
            match prune
              .as_ref()
              .map(|rule| rule.matches(entry.path(), kind))
              .transpose()
            {
              Ok(stop) => stop.unwrap_or(false),
              Err(error) => {
                warnings.lock().unwrap().push(Diagnostic::new(
                  "FILE_METADATA_FAILED",
                  format!("{}: {error}", entry.path().display()),
                ));
                return ignore::WalkState::Skip;
              }
            }
          } else {
            false
          };
          let Some(p_str) = entry.path().to_str() else {
            warnings.lock().unwrap().push(Diagnostic::new(
              "FILE_METADATA_FAILED",
              "file path is not valid UTF-8",
            ));
            return if stop {
              ignore::WalkState::Skip
            } else {
              ignore::WalkState::Continue
            };
          };
          if re.is_match(p_str) {
            match filter
              .as_ref()
              .map(|rule| rule.matches(entry.path(), kind))
              .transpose()
            {
              Ok(matched) if matched.unwrap_or(true) => {
                let _ = tx.send(p_str.to_string());
              }
              Ok(_) => {}
              Err(error) => warnings.lock().unwrap().push(Diagnostic::new(
                "FILE_METADATA_FAILED",
                format!("{p_str}: {error}"),
              )),
            }
          }
          if stop {
            return ignore::WalkState::Skip;
          }
        }
        Err(error) => warnings
          .lock()
          .unwrap()
          .push(Diagnostic::new("WALK_ERROR", error.to_string())),
      }
      ignore::WalkState::Continue
    })
  });
  drop(tx);
  let warnings = std::mem::take(&mut *warnings.lock().unwrap());
  Ok((rx.into_iter().collect(), warnings))
}

/// Heuristic for the `\|` foot-gun: POSIX BRE habits make users write
/// `\|` for alternation, but lmprobe uses Rust regex flavor where `|`
/// is alternation and `\|` is a literal pipe character. When the
/// pattern contains `\|` AND the result has zero hits, emit a
/// structured warning. Non-empty results assume the user knows what
/// they're doing (legitimate literal-pipe matches stay quiet).
///
/// QA Phase 6A.1 / battle-test cross-cutting Pattern D.
fn posix_pipe_alternation_warning(pattern: &str, no_hits: bool) -> Option<Diagnostic> {
  if !no_hits {
    return None;
  }
  if !pattern.contains(r"\|") {
    return None;
  }
  Some(
        Diagnostic::new(
            "REGEX_LITERAL_PIPE_NO_MATCH",
            "query contains `\\|` (literal pipe) and returned zero matches; Rust regex uses bare `|` for alternation, while `\\|` matches a literal pipe character",
        )
        .with_hint("try replacing `\\|` with `|` if you want alternation"),
    )
}

// ───────────────────── Python slice candidates ────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Default)]
struct PythonScope {
  class_name: Option<String>,
  function_name: Option<String>,
}

#[derive(Clone, Debug)]
struct PythonScopeFrame {
  indent: usize,
  kind: PythonScopeKind,
  name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PythonScopeKind {
  Class,
  Function,
}

#[derive(Clone, Debug)]
struct SliceAssignment {
  line_idx: usize,
  scope: PythonScope,
  base: String,
  derived: String,
  slice: String,
}

pub fn py_slice_candidates(
  roots: &[PathBuf],
  opts: &WalkOpts,
  include_trace_plan: bool,
) -> Result<PySliceCandidatesResult> {
  let mut candidates = Vec::new();
  let mut diagnostics = Vec::new();
  for root in roots {
    let files = python_candidate_files(root, opts, &mut diagnostics)?;
    for file in files {
      if let Ok(content) = fs::read_to_string(&file) {
        candidates.extend(py_slice_candidates_in_file(
          &file,
          &content,
          include_trace_plan,
        )?);
      }
    }
  }
  let mut env = PySliceCandidatesResult::new(candidates);
  for diagnostic in diagnostics {
    env.push_warning(diagnostic);
  }
  Ok(env)
}

fn python_candidate_files(
  root: &Path,
  opts: &WalkOpts,
  diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<PathBuf>> {
  let mut files = Vec::new();
  if root.is_file() {
    let lang = opts
      .lang_filter
      .or_else(|| SgLang::from_path(root))
      .or_else(|| sniff_shebang(root));
    if lang == Some(SgLang::Python) {
      files.push(root.to_path_buf());
    }
    return Ok(files);
  }

  let mut wb = WalkBuilder::new(root);
  apply_walk_opts(&mut wb, opts, root);
  for entry in wb.build() {
    let Ok(entry) = entry else {
      continue;
    };
    match walk_pre_check(&entry, opts) {
      PreCheck::Process(path, lang) if lang == SgLang::Python => files.push(path),
      PreCheck::SkipWithDiag(diagnostic) => diagnostics.push(diagnostic),
      PreCheck::Process(_, _) | PreCheck::Skip => {}
    }
  }
  Ok(files)
}

fn py_slice_candidates_in_file(
  path: &Path,
  content: &str,
  include_trace_plan: bool,
) -> Result<Vec<PySliceCandidate>> {
  let assignment_re =
    Regex::new(r"^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*([A-Za-z_][A-Za-z0-9_]*)\s*\[([^\]\n]+)\]")
      .map_err(|e| anyhow!("internal slice assignment regex failed: {}", e))?;
  let lines = content.lines().collect::<Vec<_>>();
  let scopes = python_line_scopes(&lines);
  let mut assignments = Vec::new();
  for (line_idx, line) in lines.iter().enumerate() {
    let Some(captures) = assignment_re.captures(line) else {
      continue;
    };
    let derived = captures.get(1).map(|m| m.as_str()).unwrap_or("");
    let base = captures.get(2).map(|m| m.as_str()).unwrap_or("");
    if !derived.starts_with(&format!("{base}_")) {
      continue;
    }
    assignments.push(SliceAssignment {
      line_idx,
      scope: scopes.get(line_idx).cloned().unwrap_or_default(),
      base: base.to_string(),
      derived: derived.to_string(),
      slice: captures
        .get(3)
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_default(),
    });
  }

  let mut candidates = Vec::new();
  for assignment in assignments {
    let related_re = Regex::new(&format!(
      r"\b{}_[A-Za-z0-9_]*\b",
      regex::escape(&assignment.base)
    ))
    .map_err(|e| anyhow!("internal related-variable regex failed: {}", e))?;
    let end = (assignment.line_idx + 41).min(lines.len());
    for call_idx in (assignment.line_idx + 1)..end {
      if scopes.get(call_idx) != Some(&assignment.scope) {
        continue;
      }
      let line = lines[call_idx];
      if !line.contains(&assignment.derived) {
        continue;
      }
      let Some(related) = related_re.find_iter(line).find_map(|m| {
        let var = m.as_str();
        if var == assignment.derived || !is_unsliced_name_at(line, m.end()) {
          None
        } else {
          Some(var.to_string())
        }
      }) else {
        continue;
      };
      if !call_arguments_contain_pair(line, &assignment.derived, &related) {
        continue;
      }
      let mut candidate = PySliceCandidate {
        candidate_id: None,
        file_path: path.to_string_lossy().into_owned(),
        enclosing_class: assignment.scope.class_name.clone(),
        enclosing_function: assignment.scope.function_name.clone(),
        sliced_base_variable: assignment.base.clone(),
        derived_sliced_variable: assignment.derived.clone(),
        slice_expression: format!("{}[{}]", assignment.base, assignment.slice),
        related_unsliced_variable: related,
        call_expression_line: line.trim().to_string(),
        slice_line_number: assignment.line_idx + 1,
        call_line_number: call_idx + 1,
        slice_source_line: lines[assignment.line_idx].trim().to_string(),
        call_source_line: line.trim().to_string(),
        confidence: "low".to_string(),
        trace_plan: None,
      };
      if include_trace_plan {
        attach_py_slice_trace_plan(&mut candidate, &lines);
      }
      candidates.push(candidate);
      break;
    }
  }
  Ok(candidates)
}

fn attach_py_slice_trace_plan(candidate: &mut PySliceCandidate, lines: &[&str]) {
  let candidate_id = py_slice_candidate_id(candidate);
  let statement_pattern = python_trace_statement_pattern(&candidate.call_source_line);
  let normalized_pattern = normalize_python_statement_pattern(&statement_pattern);
  let structurally_equivalent_statement_count = lines
    .iter()
    .filter(|line| normalize_python_statement_pattern(line) == normalized_pattern)
    .count();
  let mut warnings = Vec::new();
  if structurally_equivalent_statement_count > 1 {
    warnings.push(format!(
      "statement pattern appears {structurally_equivalent_statement_count} times in this file after Python whitespace/comment normalization; restrict the trace path or inspect matches before executing"
    ));
  }
  candidate.candidate_id = Some(candidate_id);
  candidate.trace_plan = Some(PySliceTracePlan {
    file_path: candidate.file_path.clone(),
    statement_pattern,
    watch_variables: unique_trace_watch_variables(candidate),
    suggested_line_number: candidate.call_line_number,
    enclosing_function: candidate.enclosing_function.clone(),
    warnings,
  });
}

fn python_trace_statement_pattern(line: &str) -> String {
  strip_python_trailing_comment(line).trim().to_string()
}

fn normalize_python_statement_pattern(line: &str) -> String {
  let without_comment = strip_python_trailing_comment(line);
  let mut normalized = String::new();
  let mut quote: Option<char> = None;
  let mut escaped = false;
  for ch in without_comment.trim().chars() {
    if let Some(q) = quote {
      normalized.push(ch);
      if escaped {
        escaped = false;
      } else if ch == '\\' {
        escaped = true;
      } else if ch == q {
        quote = None;
      }
      continue;
    }
    match ch {
      '"' | '\'' => {
        quote = Some(ch);
        normalized.push(ch);
      }
      c if c.is_whitespace() => {}
      c => normalized.push(c),
    }
  }
  normalized
}

fn strip_python_trailing_comment(line: &str) -> &str {
  let mut quote: Option<char> = None;
  let mut escaped = false;
  for (idx, ch) in line.char_indices() {
    if let Some(q) = quote {
      if escaped {
        escaped = false;
      } else if ch == '\\' {
        escaped = true;
      } else if ch == q {
        quote = None;
      }
      continue;
    }
    match ch {
      '"' | '\'' => quote = Some(ch),
      '#' => return line[..idx].trim_end(),
      _ => {}
    }
  }
  line
}

fn unique_trace_watch_variables(candidate: &PySliceCandidate) -> Vec<String> {
  let mut vars = Vec::new();
  for var in [
    &candidate.sliced_base_variable,
    &candidate.derived_sliced_variable,
    &candidate.related_unsliced_variable,
  ] {
    if !vars.iter().any(|existing| existing == var) {
      vars.push(var.clone());
    }
  }
  vars
}

fn py_slice_candidate_id(candidate: &PySliceCandidate) -> String {
  let mut hash = 0xcbf29ce484222325u64;
  for part in [
    candidate.file_path.as_str(),
    &candidate.slice_line_number.to_string(),
    &candidate.call_line_number.to_string(),
    candidate.derived_sliced_variable.as_str(),
    candidate.related_unsliced_variable.as_str(),
    candidate.call_source_line.as_str(),
  ] {
    for byte in part.as_bytes() {
      hash ^= u64::from(*byte);
      hash = hash.wrapping_mul(0x100000001b3);
    }
    hash ^= 0xff;
    hash = hash.wrapping_mul(0x100000001b3);
  }
  format!("pyslice-{hash:016x}")
}

fn python_line_scopes(lines: &[&str]) -> Vec<PythonScope> {
  let mut frames: Vec<PythonScopeFrame> = Vec::new();
  let mut scopes = Vec::with_capacity(lines.len());
  for line in lines {
    let trimmed = line.trim_start();
    let indent = python_indent(line);
    if !trimmed.is_empty() && !trimmed.starts_with('#') {
      while frames.last().is_some_and(|frame| indent <= frame.indent) {
        frames.pop();
      }
    }
    scopes.push(scope_from_frames(&frames));
    if let Some(name) = python_decl_name(trimmed, "class ") {
      frames.push(PythonScopeFrame {
        indent,
        kind: PythonScopeKind::Class,
        name,
      });
    } else if let Some(name) = python_decl_name(trimmed, "def ") {
      frames.push(PythonScopeFrame {
        indent,
        kind: PythonScopeKind::Function,
        name,
      });
    }
  }
  scopes
}

fn scope_from_frames(frames: &[PythonScopeFrame]) -> PythonScope {
  PythonScope {
    class_name: frames
      .iter()
      .rev()
      .find(|frame| frame.kind == PythonScopeKind::Class)
      .map(|frame| frame.name.clone()),
    function_name: frames
      .iter()
      .rev()
      .find(|frame| frame.kind == PythonScopeKind::Function)
      .map(|frame| frame.name.clone()),
  }
}

fn python_decl_name(trimmed: &str, prefix: &str) -> Option<String> {
  let rest = trimmed.strip_prefix(prefix)?;
  let name = rest
    .split(|c: char| c == '(' || c == ':' || c.is_whitespace())
    .next()
    .unwrap_or("");
  if name.is_empty() {
    None
  } else {
    Some(name.to_string())
  }
}

fn python_indent(line: &str) -> usize {
  line
    .chars()
    .take_while(|c| *c == ' ' || *c == '\t')
    .map(|c| if c == '\t' { 4 } else { 1 })
    .sum()
}

fn call_arguments_contain_pair(line: &str, left: &str, right: &str) -> bool {
  let mut search_start = 0;
  while let Some(open_rel) = line[search_start..].find('(') {
    let open_idx = search_start + open_rel;
    if !has_callable_before(line, open_idx) {
      search_start = open_idx + 1;
      continue;
    }
    let Some(close_idx) = matching_close_paren(line, open_idx) else {
      return false;
    };
    let args = &line[open_idx + 1..close_idx];
    if contains_python_name(args, left) && contains_python_name(args, right) {
      return true;
    }
    search_start = close_idx + 1;
  }
  false
}

fn has_callable_before(line: &str, open_idx: usize) -> bool {
  line[..open_idx]
    .chars()
    .rev()
    .find(|c| !c.is_whitespace())
    .is_some_and(|c| c == '_' || c == '.' || c.is_ascii_alphanumeric())
}

fn matching_close_paren(line: &str, open_idx: usize) -> Option<usize> {
  let mut depth = 0usize;
  for (idx, ch) in line.char_indices().skip_while(|(idx, _)| *idx < open_idx) {
    if ch == '(' {
      depth += 1;
    } else if ch == ')' {
      depth = depth.saturating_sub(1);
      if depth == 0 {
        return Some(idx);
      }
    }
  }
  None
}

fn contains_python_name(text: &str, name: &str) -> bool {
  let Ok(re) = Regex::new(&format!(r"\b{}\b", regex::escape(name))) else {
    return false;
  };
  re.is_match(text)
}

fn is_unsliced_name_at(line: &str, end: usize) -> bool {
  line
    .get(end..)
    .and_then(|tail| tail.chars().find(|c| !c.is_whitespace()))
    != Some('[')
}

// ───────────────────── Python callsite arguments ──────────────────

pub fn callsites_args(
  callee: &str,
  root: &Path,
  around: Option<usize>,
  opts: &WalkOpts,
) -> Result<CallsitesResult> {
  let mut diagnostics = Vec::new();
  let files = python_candidate_files(root, opts, &mut diagnostics)?;
  let mut callsites = Vec::new();
  for file in files {
    if let Ok(content) = fs::read_to_string(&file) {
      callsites.extend(callsites_args_in_file(&file, &content, callee, around)?);
    }
  }
  let mut env = CallsitesResult::new(callsites);
  for diagnostic in diagnostics {
    env.push_warning(diagnostic);
  }
  Ok(env)
}

pub fn callsites_filtered(
  callee: Option<&str>,
  root: &Path,
  language: Option<CallsiteLanguage>,
  filters: &[CallsiteFilterInput],
  around: Option<usize>,
  opts: &WalkOpts,
) -> Result<CallsitesResult> {
  let mut diagnostics = Vec::new();
  let files = callsite_candidate_files(root, opts, language, &mut diagnostics)?;
  let mut callsites = Vec::new();
  for file in files {
    if let Ok(content) = fs::read_to_string(&file) {
      callsites.extend(callsites_filtered_in_file(
        &file, &content, callee, language, filters, around,
      )?);
    }
  }
  let mut env = CallsitesResult::new(callsites);
  for diagnostic in diagnostics {
    env.push_warning(diagnostic);
  }
  Ok(env)
}

pub struct SourceReadsInput<'a> {
  pub root: &'a Path,
  pub language: SourceReadLanguage,
  pub filters: &'a [SourceReadFilterInput],
  pub redact_arg_values: bool,
  pub opts: &'a WalkOpts,
}

pub fn source_reads(input: SourceReadsInput<'_>) -> Result<SourceReadsResult> {
  let db = lmprobe_secrets::load_default_source_db()
    .map_err(|err| anyhow!("failed to load source DB: {err}"))?;
  let prefilter = lmprobe_secrets::compile_source_prefilter(&db)
    .map_err(|err| anyhow!("failed to compile source DB prefilter: {err}"))?;
  let mut diagnostics = Vec::new();
  let files = callsite_candidate_files(
    input.root,
    input.opts,
    Some(source_language_to_callsite_language(input.language)),
    &mut diagnostics,
  )?;
  let mut hits = Vec::new();
  for file in files {
    let Ok(content) = fs::read_to_string(&file) else {
      continue;
    };
    if !prefilter.is_match(&content) {
      continue;
    }
    hits.extend(source_reads_in_file(
      &file,
      &content,
      &db,
      input.language,
      input.filters,
    ));
  }
  let arg_values_redacted = input.redact_arg_values && redact_source_read_values(&mut hits);
  let mut env = SourceReadsResult::new(hits);
  env.arg_values_redacted = arg_values_redacted;
  for diagnostic in diagnostics {
    env.push_warning(diagnostic);
  }
  Ok(env)
}

fn source_reads_in_file(
  path: &Path,
  content: &str,
  db: &lmprobe_secrets::SourceDb,
  language: SourceReadLanguage,
  filters: &[SourceReadFilterInput],
) -> Vec<SourceReadHit> {
  let language = source_read_language_to_provider_language(language);
  lmprobe_secrets::source_matchers::source_reads_in_content(path, content, db, language, filters)
}

fn redact_source_read_values(hits: &mut [SourceReadHit]) -> bool {
  let mut redacted = false;
  for hit in hits {
    if hit.env_var_name.is_some() {
      continue;
    }
    let should_redact = hit.key_name.as_deref().is_some_and(sensitive_source_value)
      || sensitive_source_value(&hit.read_text);
    if should_redact {
      hit.read_text = "REDACTED".into();
      if let Some(key_name) = hit.key_name.as_mut() {
        if sensitive_source_value(key_name) {
          *key_name = "REDACTED".into();
        }
      }
      redacted = true;
    }
  }
  redacted
}

fn sensitive_source_value(value: &str) -> bool {
  let trimmed = lmprobe_secrets::source_matchers::strip_literal(value);
  let lower = trimmed.to_ascii_lowercase();
  let credential_words = [
    "secret",
    "token",
    "password",
    "key",
    "auth",
    "credential",
    "apikey",
  ];
  if credential_words
    .iter()
    .any(|word| lower.contains(word) || lower.contains(&word.replace('_', "")))
  {
    return true;
  }
  if trimmed.len() >= 10 && shannon_entropy(&trimmed) >= 3.5 {
    return true;
  }
  ["AKIA", "ASIA", "ghp_", "glpat-", "sk_live_", "xoxb-", "SG."]
    .iter()
    .any(|prefix| trimmed.contains(prefix))
}

fn shannon_entropy(value: &str) -> f64 {
  if value.is_empty() {
    return 0.0;
  }
  let mut counts = HashMap::new();
  for byte in value.bytes() {
    *counts.entry(byte).or_insert(0usize) += 1;
  }
  let len = value.len() as f64;
  counts
    .values()
    .map(|count| {
      let p = *count as f64 / len;
      -p * p.log2()
    })
    .sum()
}

fn source_language_to_callsite_language(language: SourceReadLanguage) -> CallsiteLanguage {
  match language {
    SourceReadLanguage::Python => CallsiteLanguage::Python,
    SourceReadLanguage::Javascript => CallsiteLanguage::Javascript,
    SourceReadLanguage::Typescript => CallsiteLanguage::Typescript,
    SourceReadLanguage::Go => CallsiteLanguage::Go,
    SourceReadLanguage::Java => CallsiteLanguage::Java,
  }
}

fn source_read_language_to_provider_language(
  language: SourceReadLanguage,
) -> lmprobe_secrets::ProviderLanguageId {
  match language {
    SourceReadLanguage::Python => lmprobe_secrets::ProviderLanguageId::Python,
    SourceReadLanguage::Javascript => lmprobe_secrets::ProviderLanguageId::JavaScript,
    SourceReadLanguage::Typescript => lmprobe_secrets::ProviderLanguageId::TypeScript,
    SourceReadLanguage::Go => lmprobe_secrets::ProviderLanguageId::Go,
    SourceReadLanguage::Java => lmprobe_secrets::ProviderLanguageId::Java,
  }
}

fn callsite_candidate_files(
  root: &Path,
  opts: &WalkOpts,
  language: Option<CallsiteLanguage>,
  diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<PathBuf>> {
  let mut files = Vec::new();
  if root.is_file() {
    let lang = opts
      .lang_filter
      .or_else(|| SgLang::from_path(root))
      .or_else(|| sniff_shebang(root));
    if language.is_none_or(|language| callsite_language_matches(language, lang)) {
      files.push(root.to_path_buf());
    }
    return Ok(files);
  }

  let mut wb = WalkBuilder::new(root);
  apply_walk_opts(&mut wb, opts, root);
  for entry in wb.build() {
    let Ok(entry) = entry else {
      continue;
    };
    match walk_pre_check(&entry, opts) {
      PreCheck::Process(path, lang)
        if language.is_none_or(|language| callsite_language_matches(language, Some(lang))) =>
      {
        files.push(path)
      }
      PreCheck::SkipWithDiag(diagnostic) => diagnostics.push(diagnostic),
      PreCheck::Process(_, _) | PreCheck::Skip => {}
    }
  }
  Ok(files)
}

fn callsite_language_matches(language: CallsiteLanguage, lang: Option<SgLang>) -> bool {
  matches!(
    (language, lang),
    (CallsiteLanguage::Python, Some(SgLang::Python))
      | (CallsiteLanguage::Javascript, Some(SgLang::JavaScript))
      | (CallsiteLanguage::Typescript, Some(SgLang::TypeScript))
      | (CallsiteLanguage::Typescript, Some(SgLang::Tsx))
      | (CallsiteLanguage::Go, Some(SgLang::Go))
      | (CallsiteLanguage::Java, Some(SgLang::Java))
  )
}

fn callsites_filtered_in_file(
  path: &Path,
  content: &str,
  callee: Option<&str>,
  language: Option<CallsiteLanguage>,
  filters: &[CallsiteFilterInput],
  around: Option<usize>,
) -> Result<Vec<CallsiteArgsHit>> {
  let Some(lang) = language
    .and_then(callsite_support_lang)
    .or_else(|| SgLang::from_path(path))
  else {
    return Ok(Vec::new());
  };
  let grep = lang.ast_grep(content);
  let root = grep.root();
  let import_evidence = import_evidence_for_filters(&root, filters);
  let mut hits = Vec::new();
  for node in root.dfs().filter(|node| node.is_named()) {
    let mut line_hits = callsite_hits_from_node(&node, callee, filters);
    for hit in &mut line_hits {
      hit.file_path = path.to_string_lossy().into_owned();
      let line_idx = node.start_pos().line();
      hit.line_number = line_idx + 1;
      hit.import_evidence = import_evidence.clone();
      hit.source_context = around
        .map(|around| line_context_from_content(content, line_idx, around))
        .unwrap_or_default();
    }
    hits.extend(line_hits.into_iter().filter(|hit| {
      filters.is_empty()
        || filters
          .iter()
          .any(|filter| callsite_filter_matches(hit, filter))
    }));
  }
  Ok(hits)
}

type SensorNode<'a> = ast_grep_core::Node<'a, ast_grep_core::tree_sitter::StrDoc<SgLang>>;

fn callsite_support_lang(language: CallsiteLanguage) -> Option<SgLang> {
  match language {
    CallsiteLanguage::Python => Some(SgLang::Python),
    CallsiteLanguage::Javascript => Some(SgLang::JavaScript),
    CallsiteLanguage::Typescript => Some(SgLang::TypeScript),
    CallsiteLanguage::Go => Some(SgLang::Go),
    CallsiteLanguage::Java => Some(SgLang::Java),
  }
}

fn callsite_hits_from_node(
  node: &SensorNode<'_>,
  callee: Option<&str>,
  filters: &[CallsiteFilterInput],
) -> Vec<CallsiteArgsHit> {
  let mut hits = Vec::new();
  if let Some(callee) = callee {
    if let Some(hit) = callsite_hit_from_call_node(node, callee) {
      hits.push(hit);
    }
  }
  for filter in filters {
    if let Some(targets) = filter.assignment_targets.as_ref() {
      for target in targets {
        if let Some(hit) = callsite_hit_from_assignment_node(node, target) {
          hits.push(hit);
        }
      }
    }
    if let Some(fields) = filter.struct_fields.as_ref() {
      if let Some(hit) = callsite_hit_from_struct_node(node, fields) {
        hits.push(hit);
      }
    }
  }
  hits
}

fn callsite_hit_from_call_node(
  node: &SensorNode<'_>,
  configured_callee: &str,
) -> Option<CallsiteArgsHit> {
  let (callee_text, arguments, kind) = call_parts_from_node(node)?;
  if !callee_matches(&callee_text, configured_callee) {
    return None;
  }
  let args = argument_text(&arguments);
  let (positional_args, keyword_args) = split_call_arguments(&args);
  let mut structured_args = callsite_arguments(&positional_args, &keyword_args);
  structured_args.extend(object_arguments_from_node(&arguments));
  Some(CallsiteArgsHit {
    file_path: String::new(),
    line_number: 0,
    callee_text,
    positional_args,
    keyword_args,
    sink_kind: kind,
    import_evidence: Vec::new(),
    structured_args,
    source_context: Vec::new(),
  })
}

fn call_parts_from_node<'a>(
  node: &SensorNode<'a>,
) -> Option<(String, SensorNode<'a>, CallsiteKind)> {
  let kind = match node.kind().as_ref() {
    "call" | "call_expression" | "method_invocation" => CallsiteKind::Call,
    "new_expression" | "object_creation_expression" => CallsiteKind::New,
    _ => return None,
  };
  let arguments = node.field("arguments").or_else(|| {
    node
      .children()
      .find(|child| child.is_named() && child.kind().as_ref().contains("argument"))
  })?;
  let callee = if let Some(function) = node.field("function") {
    node_text_compact(&function)
  } else if let Some(constructor) = node.field("constructor") {
    node_text_compact(&constructor)
  } else if let Some(type_node) = node.field("type") {
    node_text_compact(&type_node)
  } else if let (Some(object), Some(name)) = (node.field("object"), node.field("name")) {
    format!(
      "{}.{}",
      node_text_compact(&object),
      node_text_compact(&name)
    )
  } else if let Some(name) = node.field("name") {
    node_text_compact(&name)
  } else {
    return None;
  };
  Some((callee, arguments, kind))
}

fn callee_matches(actual: &str, configured: &str) -> bool {
  let actual = compact_text(actual);
  let configured = compact_text(configured);
  if actual == configured {
    return true;
  }
  !configured.contains('.') && actual.rsplit('.').next() == Some(configured.as_str())
}

fn callsite_hit_from_assignment_node(
  node: &SensorNode<'_>,
  target: &str,
) -> Option<CallsiteArgsHit> {
  if !matches!(
    node.kind().as_ref(),
    "assignment" | "assignment_expression" | "assignment_statement"
  ) {
    return None;
  }
  let lhs = node
    .field("left")
    .or_else(|| node.field("lhs"))
    .or_else(|| node.field("target"))
    .or_else(|| node.field("name"))?;
  let rhs = node
    .field("right")
    .or_else(|| node.field("rhs"))
    .or_else(|| node.field("value"))?;
  if compact_text(&lhs.text()) != compact_text(target) {
    return None;
  }
  Some(CallsiteArgsHit {
    file_path: String::new(),
    line_number: 0,
    callee_text: target.to_string(),
    positional_args: Vec::new(),
    keyword_args: Vec::new(),
    sink_kind: CallsiteKind::Assignment,
    import_evidence: Vec::new(),
    structured_args: vec![CallsiteArgument {
      position: None,
      name: Some(target.to_string()),
      text: rhs.text().trim().trim_end_matches(';').to_string(),
      role: CallsiteRole::AssignmentValue,
    }],
    source_context: Vec::new(),
  })
}

fn callsite_hit_from_struct_node(
  node: &SensorNode<'_>,
  fields: &[String],
) -> Option<CallsiteArgsHit> {
  if !matches!(
    node.kind().as_ref(),
    "composite_literal" | "literal_value" | "field_declaration_list" | "keyed_element"
  ) {
    return None;
  }
  let args = struct_field_arguments(node, fields);
  if args.is_empty() {
    return None;
  }
  Some(CallsiteArgsHit {
    file_path: String::new(),
    line_number: 0,
    callee_text: String::new(),
    positional_args: Vec::new(),
    keyword_args: Vec::new(),
    sink_kind: CallsiteKind::Struct,
    import_evidence: Vec::new(),
    structured_args: args,
    source_context: Vec::new(),
  })
}

fn struct_field_arguments(node: &SensorNode<'_>, fields: &[String]) -> Vec<CallsiteArgument> {
  let mut args = Vec::new();
  for descendant in node.dfs().filter(|descendant| descendant.is_named()) {
    let Some((name, value)) = field_name_and_value(&descendant) else {
      continue;
    };
    if fields.iter().any(|field| field == &name) {
      args.push(CallsiteArgument {
        position: None,
        name: Some(name),
        text: value,
        role: CallsiteRole::StructField,
      });
    }
  }
  args
}

fn object_arguments_from_node(arguments: &SensorNode<'_>) -> Vec<CallsiteArgument> {
  let mut args = Vec::new();
  for child in arguments.children().filter(|child| child.is_named()) {
    collect_object_arguments(&child, None, &mut args);
  }
  args
}

fn collect_object_arguments(
  node: &SensorNode<'_>,
  prefix: Option<String>,
  args: &mut Vec<CallsiteArgument>,
) {
  let Some((key, value)) = field_name_and_value(node) else {
    for child in node.children().filter(|child| child.is_named()) {
      collect_object_arguments(&child, prefix.clone(), args);
    }
    return;
  };
  let path = prefix
    .as_ref()
    .map(|prefix| format!("{prefix}.{key}"))
    .unwrap_or_else(|| key.clone());
  args.push(CallsiteArgument {
    position: None,
    name: Some(key.clone()),
    text: value.clone(),
    role: CallsiteRole::ObjectKey,
  });
  if path != key {
    args.push(CallsiteArgument {
      position: None,
      name: Some(path.clone()),
      text: value.clone(),
      role: CallsiteRole::ObjectPath,
    });
  }
  if let Some(value_node) = node.field("value").or_else(|| node.field("right")) {
    for child in value_node.children().filter(|child| child.is_named()) {
      collect_object_arguments(&child, Some(path.clone()), args);
    }
  }
}

fn field_name_and_value(node: &SensorNode<'_>) -> Option<(String, String)> {
  let key = node
    .field("key")
    .or_else(|| node.field("name"))
    .or_else(|| node.field("field"))
    .or_else(|| node.field("left"))?;
  let value = node
    .field("value")
    .or_else(|| node.field("right"))
    .or_else(|| node.field("body"))?;
  Some((
    strip_literal_text(&key.text()),
    value.text().trim().to_string(),
  ))
}

fn callsite_arguments(
  positional_args: &[String],
  keyword_args: &[CallsiteKeywordArg],
) -> Vec<CallsiteArgument> {
  let mut args = Vec::new();
  args.extend(
    positional_args
      .iter()
      .enumerate()
      .map(|(position, text)| CallsiteArgument {
        position: Some(position),
        name: None,
        text: text.clone(),
        role: CallsiteRole::Positional,
      }),
  );
  args.extend(keyword_args.iter().map(|arg| CallsiteArgument {
    position: None,
    name: Some(arg.name.clone()),
    text: arg.value.clone(),
    role: CallsiteRole::Keyword,
  }));
  args
}

fn callsite_filter_matches(hit: &CallsiteArgsHit, filter: &CallsiteFilterInput) -> bool {
  filter_import_hints_match(filter, &hit.import_evidence)
    && filter
      .arg_names
      .as_ref()
      .is_none_or(|names| has_arg_name(hit, names))
    && filter
      .positional_indices
      .as_ref()
      .is_none_or(|positions| has_position(hit, positions))
    && filter
      .struct_fields
      .as_ref()
      .is_none_or(|fields| has_role_name(hit, CallsiteRole::StructField, fields))
    && filter
      .assignment_targets
      .as_ref()
      .is_none_or(|targets| has_role_name(hit, CallsiteRole::AssignmentValue, targets))
    && filter
      .object_keys
      .as_ref()
      .is_none_or(|keys| has_role_name(hit, CallsiteRole::ObjectKey, keys))
    && filter
      .object_paths
      .as_ref()
      .is_none_or(|paths| has_role_name(hit, CallsiteRole::ObjectPath, paths))
}

fn filter_import_hints_match(filter: &CallsiteFilterInput, import_evidence: &[String]) -> bool {
  filter.import_hints.as_ref().is_none_or(|hints| {
    hints
      .iter()
      .any(|hint| import_evidence.iter().any(|evidence| evidence == hint))
  })
}

fn import_evidence_for_filters(
  root: &SensorNode<'_>,
  filters: &[CallsiteFilterInput],
) -> Vec<String> {
  let mut evidence = Vec::new();
  for hint in filters
    .iter()
    .filter_map(|filter| filter.import_hints.as_ref())
    .flatten()
  {
    if !evidence.contains(hint)
      && root
        .dfs()
        .filter(|node| node.is_named())
        .any(|node| node.kind().as_ref() != "comment" && import_hint_matches(&node.text(), hint))
    {
      evidence.push(hint.clone());
    }
  }
  evidence
}

fn import_hint_matches(text: &str, hint: &str) -> bool {
  text
    .split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/' | '.' | '@')))
    .filter(|token| !token.is_empty())
    .any(|token| {
      token == hint
        || token.starts_with(hint)
        || token.ends_with(&format!("/{hint}"))
        || token.ends_with(&format!(".{hint}"))
    })
}

fn has_arg_name(hit: &CallsiteArgsHit, names: &[String]) -> bool {
  hit.structured_args.iter().any(|arg| {
    arg.role == CallsiteRole::Keyword
      && arg
        .name
        .as_ref()
        .is_some_and(|name| names.iter().any(|candidate| candidate == name))
  })
}

fn has_position(hit: &CallsiteArgsHit, positions: &[usize]) -> bool {
  hit.structured_args.iter().any(|arg| {
    arg.role == CallsiteRole::Positional
      && arg
        .position
        .is_some_and(|position| positions.contains(&position))
  })
}

fn has_role_name(hit: &CallsiteArgsHit, role: CallsiteRole, names: &[String]) -> bool {
  hit.structured_args.iter().any(|arg| {
    arg.role == role
      && arg
        .name
        .as_ref()
        .is_some_and(|name| names.iter().any(|candidate| candidate == name))
  })
}

fn callsites_args_in_file(
  path: &Path,
  content: &str,
  callee: &str,
  around: Option<usize>,
) -> Result<Vec<CallsiteArgsHit>> {
  let mut hits = Vec::new();
  let grep = SgLang::Python.ast_grep(content);
  let root = grep.root();
  for node in root.dfs() {
    if !node.is_named() {
      continue;
    }
    if let Some(mut hit) = callsite_hit_from_call_node(&node, callee) {
      let line_idx = node.start_pos().line();
      hit.file_path = path.to_string_lossy().into_owned();
      hit.line_number = line_idx + 1;
      hit.source_context = around
        .map(|around| line_context_from_content(content, line_idx, around))
        .unwrap_or_default();
      hits.push(hit);
    }
  }
  Ok(hits)
}

fn split_call_arguments(args: &str) -> (Vec<String>, Vec<CallsiteKeywordArg>) {
  let mut positional = Vec::new();
  let mut keywords = Vec::new();
  for arg in split_top_level_commas(args) {
    let arg = arg.trim();
    if arg.is_empty() {
      continue;
    }
    if let Some((name, value)) = split_keyword_argument(arg) {
      keywords.push(CallsiteKeywordArg { name, value });
    } else {
      positional.push(arg.to_string());
    }
  }
  (positional, keywords)
}

fn split_keyword_argument(arg: &str) -> Option<(String, String)> {
  let eq_idx = top_level_equals(arg)?;
  let name = arg[..eq_idx].trim();
  if !is_simple_python_name(name) {
    return None;
  }
  Some((name.to_string(), arg[eq_idx + 1..].trim().to_string()))
}

fn split_top_level_commas(args: &str) -> Vec<String> {
  let mut parts = Vec::new();
  let mut start = 0usize;
  let mut paren = 0usize;
  let mut bracket = 0usize;
  let mut brace = 0usize;
  let mut quote: Option<char> = None;
  let mut escaped = false;
  for (idx, ch) in args.char_indices() {
    if let Some(q) = quote {
      if escaped {
        escaped = false;
      } else if ch == '\\' {
        escaped = true;
      } else if ch == q {
        quote = None;
      }
      continue;
    }
    match ch {
      '"' | '\'' => quote = Some(ch),
      '(' => paren += 1,
      ')' => paren = paren.saturating_sub(1),
      '[' => bracket += 1,
      ']' => bracket = bracket.saturating_sub(1),
      '{' => brace += 1,
      '}' => brace = brace.saturating_sub(1),
      ',' if paren == 0 && bracket == 0 && brace == 0 => {
        parts.push(args[start..idx].to_string());
        start = idx + 1;
      }
      _ => {}
    }
  }
  parts.push(args[start..].to_string());
  parts
}

fn top_level_equals(arg: &str) -> Option<usize> {
  let mut paren = 0usize;
  let mut bracket = 0usize;
  let mut brace = 0usize;
  let mut quote: Option<char> = None;
  let mut escaped = false;
  for (idx, ch) in arg.char_indices() {
    if let Some(q) = quote {
      if escaped {
        escaped = false;
      } else if ch == '\\' {
        escaped = true;
      } else if ch == q {
        quote = None;
      }
      continue;
    }
    match ch {
      '"' | '\'' => quote = Some(ch),
      '(' => paren += 1,
      ')' => paren = paren.saturating_sub(1),
      '[' => bracket += 1,
      ']' => bracket = bracket.saturating_sub(1),
      '{' => brace += 1,
      '}' => brace = brace.saturating_sub(1),
      '=' if paren == 0 && bracket == 0 && brace == 0 && is_keyword_assignment_equal(arg, idx) => {
        return Some(idx)
      }
      _ => {}
    }
  }
  None
}

fn is_keyword_assignment_equal(arg: &str, idx: usize) -> bool {
  let prev = arg[..idx].chars().rev().find(|c| !c.is_whitespace());
  let next = arg[idx + 1..].chars().find(|c| !c.is_whitespace());
  !matches!(prev, Some('=' | '!' | '<' | '>')) && next != Some('=')
}

fn is_simple_python_name(name: &str) -> bool {
  let mut chars = name.chars();
  let Some(first) = chars.next() else {
    return false;
  };
  (first == '_' || first.is_ascii_alphabetic())
    && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn argument_text(arguments: &SensorNode<'_>) -> String {
  let text = arguments.text();
  trim_balanced_delimiters(text.trim()).to_string()
}

fn trim_balanced_delimiters(text: &str) -> &str {
  if text.len() >= 2 {
    let bytes = text.as_bytes();
    if matches!(
      (bytes[0], bytes[text.len() - 1]),
      (b'(', b')') | (b'[', b']') | (b'{', b'}')
    ) {
      return &text[1..text.len() - 1];
    }
  }
  text
}

fn node_text_compact(node: &SensorNode<'_>) -> String {
  compact_text(&node.text())
}

fn compact_text(text: &str) -> String {
  text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn strip_literal_text(text: &str) -> String {
  text
    .trim()
    .trim_matches('"')
    .trim_matches('\'')
    .trim_matches('`')
    .to_string()
}

fn line_context_from_content(content: &str, line_idx: usize, around: usize) -> Vec<LineContext> {
  let lines = content.split('\n').collect::<Vec<_>>();
  line_context_around(&lines, line_idx, around)
}

fn line_context_around(lines: &[&str], line_idx: usize, around: usize) -> Vec<LineContext> {
  let start = line_idx.saturating_sub(around);
  let end = (line_idx + around + 1).min(lines.len());
  (start..end)
    .filter_map(|idx| {
      lines.get(idx).map(|line| LineContext {
        line_number: idx + 1,
        line_content: (*line).to_string(),
      })
    })
    .collect()
}

// ────────────────────────────── grep ──────────────────────────────

pub fn grep(pattern: &str, root: &Path, opts: &WalkOpts) -> Result<GrepResultEnvelope> {
  grep_many(pattern, &[root.to_path_buf()], opts)
}

pub fn grep_many(pattern: &str, roots: &[PathBuf], opts: &WalkOpts) -> Result<GrepResultEnvelope> {
  let re = match Regex::new(pattern) {
    Ok(re) => re,
    Err(err) => {
      let mut env = GrepResultEnvelope::new(Vec::new());
      env.exit_status = ExitStatus::Error;
      env.push_warning(regex_parse_diagnostic(pattern, &err));
      return Ok(env);
    }
  };
  let roots = narrow_quick_project_roots(roots, opts)?;
  let mut hits = Vec::new();
  let mut diags = Vec::new();
  let mut scan_stats = ScanStats::default();
  for root in &roots {
    let mut root_opts = opts.clone();
    if let Some(limit) = opts.result_limit {
      if hits.len() >= limit {
        break;
      }
      root_opts.result_limit = Some(limit - hits.len());
    }
    let (root_hits, root_diags, root_stats) = grep_text_hits(&re, root, &root_opts)?;
    hits.extend(root_hits);
    diags.extend(root_diags);
    scan_stats.files_considered += root_stats.files_considered;
    scan_stats.files_scanned += root_stats.files_scanned;
    scan_stats.files_skipped += root_stats.files_skipped;
  }
  if let Some(limit) = opts.result_limit {
    hits.truncate(limit);
  }
  let no_hits = hits.is_empty();
  let partial = diags.iter().any(is_coverage_partial_diagnostic);
  let gitignore_excluded = count_gitignore_excluded_total(&roots, opts);
  let mut env = GrepResultEnvelope::new(hits);
  if partial {
    env.partial = true;
  }
  if no_hits {
    // Dogfood finding (2026-09): `files_skipped: 0` on a `no_match`
    // response read as "nothing was excluded" when `.gitignore` had in
    // fact hidden the matching file — a false reassurance, not just a
    // missing warning. `files_ignored` is the honest counter for that
    // population; `files_skipped` keeps its narrower, already-accurate
    // meaning (considered-then-skipped, e.g. `FILE_TOO_LARGE`).
    scan_stats.files_ignored = Some(gitignore_excluded);
    env.scan_stats = Some(scan_stats);
  }
  for d in diags {
    env.push_warning(d);
  }
  if let Some(d) = posix_pipe_alternation_warning(pattern, no_hits) {
    env.push_warning(d);
  }
  if let Some(d) = gitignore_excluded_warning(gitignore_excluded) {
    env.push_warning(d);
  }
  Ok(env)
}

fn is_coverage_partial_diagnostic(diagnostic: &Diagnostic) -> bool {
  matches!(
    diagnostic.code.as_str(),
    "FILE_TOO_LARGE" | "FILE_READ_FAILED" | "WALK_ENTRY_ERROR"
  )
}

pub fn attach_grep_parents(hits: &mut [GrepHit], opts: &WalkOpts) {
  attach_grep_structural_expansions(hits, opts, true, false);
}

pub fn attach_grep_bodies(hits: &mut [GrepHit], opts: &WalkOpts) {
  attach_grep_structural_expansions(hits, opts, false, true);
}

pub fn attach_grep_roles(hits: &mut [GrepHit], pattern: &str, opts: &WalkOpts) {
  if hits.is_empty() {
    return;
  }
  let Ok(re) = Regex::new(pattern) else {
    return;
  };
  attach_grep_roles_with_regex(hits, &re, opts);
}

pub fn attach_grep_roles_for_literal(hits: &mut [GrepHit], literal: &str, opts: &WalkOpts) {
  if hits.is_empty() {
    return;
  }
  let Ok(re) = Regex::new(&regex::escape(literal)) else {
    return;
  };
  attach_grep_roles_with_regex(hits, &re, opts);
}

fn attach_grep_roles_with_regex(hits: &mut [GrepHit], re: &Regex, opts: &WalkOpts) {
  use std::collections::BTreeMap;

  let mut by_file: BTreeMap<String, Vec<usize>> = BTreeMap::new();
  for (idx, hit) in hits.iter().enumerate() {
    by_file.entry(hit.file_path.clone()).or_default().push(idx);
  }

  for (file_path, hit_indices) in by_file {
    let path = Path::new(&file_path);
    let lang = opts
      .lang_filter
      .or_else(|| SgLang::from_path(path))
      .or_else(|| sniff_shebang(path));
    let Ok(content) = fs::read_to_string(path) else {
      for idx in hit_indices {
        hits[idx].role = Some("unknown".to_string());
      }
      continue;
    };
    let lines = content.lines().collect::<Vec<_>>();
    for idx in hit_indices {
      let Some(line) = lines.get(hits[idx].line_number.saturating_sub(1)) else {
        hits[idx].role = Some("unknown".to_string());
        continue;
      };
      let role = if lang == Some(SgLang::Python) {
        classify_python_line_role(line, re)
      } else {
        "unknown"
      };
      hits[idx].role = Some(role.to_string());
    }
  }
}

fn classify_python_line_role(line: &str, re: &Regex) -> &'static str {
  let mut role: Option<&'static str> = None;
  for m in re.find_iter(line) {
    let next_role = classify_python_anchor_role(line, m.start(), m.end());
    if role.is_some_and(|role| role != next_role) {
      return "unknown";
    }
    role = Some(next_role);
  }
  role.unwrap_or("unknown")
}

fn classify_python_anchor_role(line: &str, start: usize, end: usize) -> &'static str {
  let trimmed = line.trim_start();
  let before = line.get(..start).unwrap_or("");
  let after = line.get(end..).unwrap_or("");

  if starts_with_any(trimmed, &["import ", "from "]) {
    return "import";
  }
  if starts_with_any(trimmed, &["def ", "class "]) {
    return "definition";
  }
  if next_nonspace(after) == Some(':') && before.rfind('{') > before.rfind('}') {
    return "dict_key";
  }
  if previous_nonspace(before) == Some('.') || next_nonspace(after) == Some('.') {
    return "attribute_access";
  }
  if previous_nonspace(before) == Some('[') || next_nonspace(after) == Some('[') {
    return "index_access";
  }
  if starts_with_any(trimmed, &["return ", "yield "]) {
    return "return";
  }
  if starts_with_any(trimmed, &["if ", "elif ", "while ", "assert "]) {
    return "condition";
  }
  if next_nonspace(after) == Some('(') {
    return "call";
  }
  if is_assignment_lhs(line, start) {
    return "assignment";
  }
  if is_inside_call_arguments(before, after) {
    return "argument";
  }
  if line.get(start..end).is_some_and(is_role_readable_anchor) {
    return "read";
  }
  "unknown"
}

fn starts_with_any(value: &str, prefixes: &[&str]) -> bool {
  prefixes.iter().any(|prefix| value.starts_with(prefix))
}

fn previous_nonspace(value: &str) -> Option<char> {
  value.chars().rev().find(|c| !c.is_whitespace())
}

fn next_nonspace(value: &str) -> Option<char> {
  value.chars().find(|c| !c.is_whitespace())
}

fn is_assignment_lhs(line: &str, start: usize) -> bool {
  let Some(eq_idx) = line.find('=') else {
    return false;
  };
  if line.get(eq_idx..=eq_idx).is_some_and(|s| s == "=") {
    let prev = line[..eq_idx].chars().rev().find(|c| !c.is_whitespace());
    let next = line[eq_idx + 1..].chars().find(|c| !c.is_whitespace());
    if matches!(prev, Some('=' | '!' | '<' | '>')) || next == Some('=') {
      return false;
    }
  }
  start < eq_idx
}

fn is_inside_call_arguments(before: &str, after: &str) -> bool {
  before.rfind('(').is_some_and(|open| {
    let after_open = &before[open + 1..];
    !after_open.contains(')') && after.contains(')')
  })
}

fn is_role_readable_anchor(value: &str) -> bool {
  let value = value.trim_matches(|c: char| c == '"' || c == '\'' || c.is_whitespace());
  let mut chars = value.chars();
  let Some(first) = chars.next() else {
    return false;
  };
  (first == '_' || first.is_ascii_alphabetic())
    && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

pub fn attach_grep_structural_expansions(
  hits: &mut [GrepHit],
  opts: &WalkOpts,
  include_parents: bool,
  include_body: bool,
) {
  if !include_parents && !include_body {
    return;
  }
  use std::collections::BTreeMap;

  let mut by_file: BTreeMap<String, Vec<usize>> = BTreeMap::new();
  for (idx, hit) in hits.iter().enumerate() {
    by_file.entry(hit.file_path.clone()).or_default().push(idx);
  }

  for (file_path, hit_indices) in by_file {
    let path = Path::new(&file_path);
    let Some(lang) = opts
      .lang_filter
      .or_else(|| SgLang::from_path(path))
      .or_else(|| sniff_shebang(path))
    else {
      continue;
    };
    let Ok(content) = fs::read_to_string(path) else {
      continue;
    };
    let line_numbers = hit_indices
      .iter()
      .map(|idx| hits[*idx].line_number)
      .collect::<Vec<_>>();
    let expansions = grep_structural_expansions(&content, lang, &line_numbers);
    for (idx, expansion) in hit_indices.into_iter().zip(expansions) {
      if include_parents {
        hits[idx].parents = expansion.parents;
      }
      if include_body {
        hits[idx].body = expansion.body;
      }
    }
  }
}

/// For each requested source line, detect whether that line is inside
/// any AST node matching one of the provided `kinds`.
pub fn grep_hits_inside_kinds(
  path: &Path,
  opts: &WalkOpts,
  line_numbers: &[usize],
  kinds: &HashSet<String>,
) -> Vec<bool> {
  if line_numbers.is_empty() {
    return Vec::new();
  }
  let Some(lang) = opts
    .lang_filter
    .or_else(|| SgLang::from_path(path))
    .or_else(|| sniff_shebang(path))
  else {
    return vec![false; line_numbers.len()];
  };
  let Ok(content) = fs::read_to_string(path) else {
    return vec![false; line_numbers.len()];
  };
  let line_numbers = line_numbers.to_vec();
  let hits_by_line = grep_hints_by_line(&content, lang, &line_numbers);
  line_numbers
    .iter()
    .map(|line_number| {
      hits_by_line
        .iter()
        .any(|hints| kinds.contains(&hints.0) && hints.1.contains(line_number))
    })
    .collect()
}

fn grep_hints_by_line(
  content: &str,
  lang: SgLang,
  line_numbers: &[usize],
) -> Vec<(String, std::collections::HashSet<usize>)> {
  let grep = lang.ast_grep(content);
  let root = grep.root();
  let mut node_lines = Vec::new();
  let line_set = line_numbers.iter().copied().collect::<HashSet<_>>();

  for node in root.dfs() {
    let kind = node.kind().to_string();
    let line_start = node.start_pos().line() + 1;
    let line_end = node.end_pos().line() + 1;
    let mut covered = HashSet::new();
    for line in line_set.iter().copied() {
      if line_start <= line && line <= line_end {
        let _ = covered.insert(line);
      }
    }
    if !covered.is_empty() {
      node_lines.push((kind, covered));
    }
  }
  node_lines
}

pub fn structural_expansions_for_content(
  content: &str,
  path: &Path,
  opts: &WalkOpts,
  line_numbers: &[usize],
) -> Option<Vec<(Vec<StructuralParent>, Option<StructuralBody>)>> {
  let lang = opts
    .lang_filter
    .or_else(|| SgLang::from_path(path))
    .or_else(|| sniff_shebang(path))?;
  Some(
    grep_structural_expansions(content, lang, line_numbers)
      .into_iter()
      .map(|expansion| (expansion.parents, expansion.body))
      .collect(),
  )
}

struct GrepStructuralExpansion {
  parents: Vec<StructuralParent>,
  body: Option<StructuralBody>,
}

fn grep_structural_expansions(
  content: &str,
  lang: SgLang,
  line_numbers: &[usize],
) -> Vec<GrepStructuralExpansion> {
  let grep = lang.ast_grep(content);
  let root = grep.root();
  let extractor = lmprobe_registry::name_extractor_for(lang);
  let mut parents_by_line = vec![Vec::new(); line_numbers.len()];
  let mut bodies_by_line: Vec<Option<StructuralBody>> = vec![None; line_numbers.len()];

  for node in root.dfs() {
    let kind = node.kind().to_string();
    let is_parent_kind = is_grep_parent_kind(&kind);
    let is_body_kind = is_grep_body_kind(&kind);
    if !is_parent_kind && !is_body_kind {
      continue;
    }
    let line_start = node.start_pos().line() + 1;
    let line_end = node.end_pos().line() + 1;
    let node_match = ast_grep_core::NodeMatch::from(node);
    let name =
      extractor
        .extract(&node_match)
        .and_then(|name| if name.is_empty() { None } else { Some(name) });
    let parent = StructuralParent {
      kind,
      name,
      line_start,
      line_end,
    };
    for (line_idx, line_number) in line_numbers.iter().enumerate() {
      if line_start <= *line_number && *line_number <= line_end {
        if is_parent_kind {
          parents_by_line[line_idx].push(parent.clone());
        }
        if is_body_kind {
          let candidate = StructuralBody {
            kind: parent.kind.clone(),
            name: if is_parent_kind {
              parent.name.clone()
            } else {
              None
            },
            line_start,
            line_end,
            content: node_match.text().to_string(),
          };
          if is_more_specific_body(&candidate, bodies_by_line[line_idx].as_ref()) {
            bodies_by_line[line_idx] = Some(candidate);
          }
        }
      }
    }
  }

  for parents in &mut parents_by_line {
    parents.sort_by(|a, b| {
      a.line_start
        .cmp(&b.line_start)
        .then_with(|| b.line_end.cmp(&a.line_end))
        .then_with(|| a.kind.cmp(&b.kind))
    });
  }
  parents_by_line
    .into_iter()
    .zip(bodies_by_line)
    .map(|(parents, body)| GrepStructuralExpansion { parents, body })
    .collect()
}

fn is_more_specific_body(candidate: &StructuralBody, current: Option<&StructuralBody>) -> bool {
  let Some(current) = current else {
    return true;
  };
  let candidate_span = candidate.line_end.saturating_sub(candidate.line_start);
  let current_span = current.line_end.saturating_sub(current.line_start);
  candidate_span < current_span
    || (candidate_span == current_span && candidate.line_start >= current.line_start)
}

fn grep_text_hits(
  re: &Regex,
  root: &Path,
  opts: &WalkOpts,
) -> Result<(Vec<GrepHit>, Vec<Diagnostic>, ScanStats)> {
  let (tx, rx) = mpsc::channel();
  let diagnostics: Arc<Mutex<Vec<Diagnostic>>> = Arc::new(Mutex::new(Vec::new()));
  let scan_stats = Arc::new(ScanStatsCounters::default());
  let stop = Arc::new(AtomicBool::new(false));
  let matched = Arc::new(AtomicUsize::new(0));
  let root_display = root.display().to_string();
  let mut wb = WalkBuilder::new(root);
  apply_walk_opts(&mut wb, opts, root);
  let walker = wb.build_parallel();
  walker.run(|| {
    let tx = tx.clone();
    let diagnostics = diagnostics.clone();
    let scan_stats = scan_stats.clone();
    let stop = stop.clone();
    let matched = matched.clone();
    let re = re.clone();
    let opts = opts;
    let limit = opts.result_limit;
    let root_display = root_display.clone();
    Box::new(move |result| {
      if stop.load(Ordering::Relaxed) {
        return ignore::WalkState::Quit;
      }
      let entry = match result {
        Ok(entry) => entry,
        Err(err) => {
          diagnostics.lock().unwrap().push(Diagnostic::new(
            "WALK_ENTRY_ERROR",
            format!("failed to walk an entry under {root_display}: {err}"),
          ));
          return ignore::WalkState::Continue;
        }
      };
      // grep accepts every file (no language filter), so we
      // only check size + path-is-file here.
      let path = entry.path();
      if !path.is_file() {
        return ignore::WalkState::Continue;
      }
      scan_stats.considered();
      if let Some(max) = opts.max_filesize {
        if let Ok(meta) = path.metadata() {
          if meta.len() > max {
            scan_stats.skipped();
            diagnostics.lock().unwrap().push(Diagnostic::new(
              "FILE_TOO_LARGE",
              format!(
                "skipped {} ({} bytes > --max-filesize {})",
                path.display(),
                meta.len(),
                max
              ),
            ));
            return ignore::WalkState::Continue;
          }
        }
      }
      match fs::File::open(path) {
        Ok(file) => {
          scan_stats.scanned();
          let reader = BufReader::new(file);
          for (idx, line_res) in reader.lines().enumerate() {
            let line = match line_res {
              Ok(line) => line,
              Err(err) => {
                diagnostics.lock().unwrap().push(Diagnostic::new(
                  "FILE_READ_FAILED",
                  format!(
                    "failed to read line {} from {}: {}",
                    idx + 1,
                    path.display(),
                    err
                  ),
                ));
                break;
              }
            };
            if re.is_match(&line) {
              let _ = tx.send(GrepHit {
                file_path: path.to_str().unwrap_or("").to_string(),
                line_number: idx + 1,
                line_content: line.trim().to_string(),
                ..Default::default()
              });
              if let Some(limit) = limit {
                if matched.fetch_add(1, Ordering::Relaxed) + 1 >= limit {
                  stop.store(true, Ordering::Relaxed);
                  return ignore::WalkState::Quit;
                }
              }
            }
          }
        }
        Err(err) => {
          scan_stats.skipped();
          diagnostics.lock().unwrap().push(Diagnostic::new(
            "FILE_READ_FAILED",
            format!("failed to open {}: {}", path.display(), err),
          ));
        }
      }
      ignore::WalkState::Continue
    })
  });
  drop(tx);
  let hits: Vec<_> = rx.into_iter().collect();
  let diags = std::mem::take(&mut *diagnostics.lock().unwrap());
  Ok((hits, diags, scan_stats.snapshot()))
}

/// §1.3 — structural identifier-node match. For every code-language
/// file (or all, when `--scope all`), parse the AST and emit hits
/// where any named identifier-shaped node has text equal to `name`.
/// `--include-comments` / `--include-docs` opt back into the legacy
/// substring surface area.
pub fn refs(
  name: &str,
  root: &Path,
  scope_code: bool,
  include_comments: bool,
  include_docs: bool,
  opts: &WalkOpts,
) -> Result<RefResult> {
  let (tx, rx) = mpsc::channel();
  let too_large: Arc<Mutex<Vec<Diagnostic>>> = Arc::new(Mutex::new(Vec::new()));
  let name_str = name.to_string();
  let mut wb = WalkBuilder::new(root);
  apply_walk_opts(&mut wb, opts, root);
  let walker = wb.build_parallel();
  walker.run(|| {
    let tx = tx.clone();
    let too_large = too_large.clone();
    let name_str = name_str.clone();
    let opts = opts;
    Box::new(move |result| {
      if let Ok(entry) = result {
        let (path_buf, lang) = match walk_pre_check(&entry, opts) {
          PreCheck::Process(p, l) => (p, l),
          PreCheck::Skip => return ignore::WalkState::Continue,
          PreCheck::SkipWithDiag(d) => {
            too_large.lock().unwrap().push(d);
            return ignore::WalkState::Continue;
          }
        };
        let path = path_buf.as_path();
        {
          {
            if scope_code && !lmprobe_lang_core::langs::is_code_lang(lang) {
              return ignore::WalkState::Continue;
            }
            if let Ok(content) = fs::read_to_string(path) {
              if !content.contains(&name_str) {
                return ignore::WalkState::Continue;
              }
              let grep = lang.ast_grep(content);
              let root_node = grep.root();
              for node in root_node.dfs() {
                if !is_identifier_kind(node.kind().as_ref()) {
                  continue;
                }
                if !include_comments && in_comment_ancestor(&node) {
                  continue;
                }
                if !include_docs && in_doc_ancestor(&node) {
                  continue;
                }
                let txt = node.text();
                if txt == name_str {
                  let _ = tx.send(GrepHit {
                    file_path: path.to_str().unwrap_or("").to_string(),
                    line_number: node.start_pos().line() + 1,
                    line_content: line_for_pos(grep.source(), node.start_pos().line()),
                    ..Default::default()
                  });
                }
              }
            }
          }
        }
      }
      ignore::WalkState::Continue
    })
  });
  drop(tx);
  let mut results: Vec<_> = rx.into_iter().collect();
  results.sort_by(|a, b| {
    a.file_path
      .cmp(&b.file_path)
      .then(a.line_number.cmp(&b.line_number))
  });
  // De-dup multiple identifier hits on the same line.
  results.dedup_by(|a, b| a.file_path == b.file_path && a.line_number == b.line_number);
  let mut env = RefResult::new(results);
  for d in std::mem::take(&mut *too_large.lock().unwrap()) {
    env.push_warning(d);
  }
  Ok(env)
}

pub fn refs_exists(name: &str, root: &Path, opts: &WalkOpts) -> Result<bool> {
  let found = Arc::new(AtomicBool::new(false));
  let name_str = name.to_string();
  let mut wb = WalkBuilder::new(root);
  apply_walk_opts(&mut wb, opts, root);
  let walker = wb.build_parallel();
  walker.run(|| {
    let found = found.clone();
    let name_str = name_str.clone();
    let opts = opts;
    Box::new(move |result| {
      if found.load(Ordering::Relaxed) {
        return ignore::WalkState::Quit;
      }
      if let Ok(entry) = result {
        let (path_buf, lang) = match walk_pre_check(&entry, opts) {
          PreCheck::Process(p, l) => (p, l),
          PreCheck::Skip | PreCheck::SkipWithDiag(_) => return ignore::WalkState::Continue,
        };
        let path = path_buf.as_path();
        if let Ok(content) = fs::read_to_string(path) {
          if !content.contains(&name_str) {
            return ignore::WalkState::Continue;
          }
          let grep = lang.ast_grep(content);
          let root_node = grep.root();
          for node in root_node.dfs() {
            if !is_identifier_kind(node.kind().as_ref()) {
              continue;
            }
            if in_comment_ancestor(&node) || in_doc_ancestor(&node) {
              continue;
            }
            if node.text() == name_str {
              found.store(true, Ordering::Relaxed);
              return ignore::WalkState::Quit;
            }
          }
        }
      }
      ignore::WalkState::Continue
    })
  });
  Ok(found.load(Ordering::Relaxed))
}

fn is_identifier_kind(kind: &str) -> bool {
  // Tree-sitter identifier-flavoured leaf kinds — broadly portable.
  kind == "identifier"
    || kind == "type_identifier"
    || kind == "field_identifier"
    || kind == "simple_identifier"
    || kind == "constant"
    || kind == "name"
    || kind == "shorthand_property_identifier"
    || kind == "property_identifier"
}

fn in_comment_ancestor<'a>(
  node: &ast_grep_core::Node<'a, ast_grep_core::tree_sitter::StrDoc<SgLang>>,
) -> bool {
  let mut cur = node.parent();
  while let Some(n) = cur {
    if n.kind().contains("comment") {
      return true;
    }
    cur = n.parent();
  }
  false
}

fn in_doc_ancestor<'a>(
  node: &ast_grep_core::Node<'a, ast_grep_core::tree_sitter::StrDoc<SgLang>>,
) -> bool {
  let mut cur = node.parent();
  while let Some(n) = cur {
    let k = n.kind();
    if k.contains("docstring") || k.contains("doc_comment") || k == "string" {
      return true;
    }
    cur = n.parent();
  }
  false
}

fn line_for_pos(source: &str, zero_based_line: usize) -> String {
  source
    .lines()
    .nth(zero_based_line)
    .map(|s| s.trim().to_string())
    .unwrap_or_default()
}

// ──────────────────────────── callers ─────────────────────────────

pub fn callers(name: &str, root: &Path, opts: &WalkOpts) -> Result<CallersResult> {
  let (tx, rx) = mpsc::channel();
  let unsupported_langs: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
  let parse_failures: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
  let too_large: Arc<Mutex<Vec<Diagnostic>>> = Arc::new(Mutex::new(Vec::new()));
  // QA TC-080a / Phase 0 §0.7: track whether ANY file with a
  // supported language grammar was actually evaluated during the
  // walk. If hits=0 and no supported lang was reached, the call
  // didn't run anywhere — promote NoMatch → Matched + emit
  // NOT_IMPLEMENTED. If supported langs ran (just found 0), that's
  // a genuine no_match.
  let any_supported_lang_walked: Arc<std::sync::atomic::AtomicBool> =
    Arc::new(std::sync::atomic::AtomicBool::new(false));
  let name_str = name.to_string();
  let mut wb = WalkBuilder::new(root);
  apply_walk_opts(&mut wb, opts, root);
  let walker = wb.build_parallel();
  walker.run(|| {
    let tx = tx.clone();
    let too_large = too_large.clone();
    let name_str = name_str.clone();
    let unsupported = unsupported_langs.clone();
    let pfail = parse_failures.clone();
    let any_supported = any_supported_lang_walked.clone();
    let opts = opts;
    Box::new(move |result| {
      if let Ok(entry) = result {
        let (path_buf, lang) = match walk_pre_check(&entry, opts) {
          PreCheck::Process(p, l) => (p, l),
          PreCheck::Skip => return ignore::WalkState::Continue,
          PreCheck::SkipWithDiag(d) => {
            too_large.lock().unwrap().push(d);
            return ignore::WalkState::Continue;
          }
        };
        let path = path_buf.as_path();
        {
          {
            // §0.5 — D5 CallGrammar trait dispatch. An
            // empty pattern set means "no call grammar
            // for this language"; emit NOT_IMPLEMENTED.
            let grammar = call_grammar_for(lang);
            let pattern_strs = grammar.call_patterns(&name_str);
            if pattern_strs.is_empty() {
              unsupported.lock().unwrap().insert(format!("{:?}", lang));
              return ignore::WalkState::Continue;
            }
            any_supported.store(true, std::sync::atomic::Ordering::Relaxed);
            if let Ok(content) = fs::read_to_string(path) {
              if !content.contains(&name_str) {
                return ignore::WalkState::Continue;
              }
              let grep = lang.ast_grep(content);
              let root_node = grep.root();
              for pattern_str in pattern_strs {
                // §0.4 / QA §2.1 — try_new instead of
                // unwrapping inside find_all.
                let pat = match Pattern::try_new(&pattern_str, lang) {
                  Ok(p) => p,
                  Err(_) => {
                    pfail.lock().unwrap().insert(format!("{:?}", lang));
                    continue;
                  }
                };
                for m in root_node.find_all(pat) {
                  // §4.D — walk up to the enclosing
                  // declaration so the caller's own
                  // name is recoverable from the
                  // SymbolHit (impact's BFS uses it
                  // instead of line-walk-back).
                  let enclosing = enclosing_decl_name(&m, lang);
                  let (enclosing_name, enclosing_kind) = match enclosing {
                    Some((n, k)) => (Some(n), Some(k)),
                    None => (None, None),
                  };
                  let _ = tx.send(SymbolHit {
                    name: name_str.clone(),
                    kind: "call_site".to_string(),
                    lang: format!("{:?}", lang),
                    file_path: path.to_str().unwrap_or("").to_string(),
                    line_start: (m.start_pos().line() + 1) as u32,
                    content: m.text().to_string(),
                    enclosing_decl: enclosing_name,
                    enclosing_decl_kind: enclosing_kind,
                    ..Default::default()
                  });
                }
              }
              if lang == SgLang::Go {
                for m in go_selector_call_matches(&root_node, &name_str) {
                  let enclosing = enclosing_decl_name(&m, lang);
                  let (enclosing_name, enclosing_kind) = match enclosing {
                    Some((n, k)) => (Some(n), Some(k)),
                    None => (None, None),
                  };
                  let _ = tx.send(SymbolHit {
                    name: name_str.clone(),
                    kind: "call_site".to_string(),
                    lang: format!("{:?}", lang),
                    file_path: path.to_str().unwrap_or("").to_string(),
                    line_start: (m.start_pos().line() + 1) as u32,
                    content: m.text().to_string(),
                    enclosing_decl: enclosing_name,
                    enclosing_decl_kind: enclosing_kind,
                    ..Default::default()
                  });
                }
              }
            }
          }
        }
      }
      ignore::WalkState::Continue
    })
  });
  drop(tx);
  let mut results: Vec<_> = rx.into_iter().collect();
  results.sort_by(|a, b| {
    a.file_path
      .cmp(&b.file_path)
      .then(a.line_start.cmp(&b.line_start))
      .then(a.content.cmp(&b.content))
  });
  results.dedup_by(|a, b| {
    a.file_path == b.file_path && a.line_start == b.line_start && a.content == b.content
  });
  let any_supported = any_supported_lang_walked.load(std::sync::atomic::Ordering::Relaxed);
  let mut env = CallersResult::new(results);
  let unsupported = std::mem::take(&mut *unsupported_langs.lock().unwrap());
  let had_unsupported = !unsupported.is_empty();
  for lang in unsupported {
    env.push_warning(
      Diagnostic::new("NOT_IMPLEMENTED", "callers not implemented for language")
        .with_lang(lang)
        .with_hint("D5 CallGrammar trait will land per-language patterns in Phase 1"),
    );
  }
  // QA TC-080a: if no supported language was reached AND we
  // emitted NOT_IMPLEMENTED for at least one lang, the verb
  // couldn't run anywhere. That's matched + diagnostic, not no_match.
  if !any_supported && had_unsupported && env.exit_status == ExitStatus::NoMatch {
    env.exit_status = ExitStatus::Matched;
  }
  let pfails = std::mem::take(&mut *parse_failures.lock().unwrap());
  for lang in pfails {
    env.push_warning(
      Diagnostic::new(
        "PATTERN_PARSE_FAILED",
        "callers pattern failed to parse for language",
      )
      .with_lang(lang),
    );
  }
  for d in std::mem::take(&mut *too_large.lock().unwrap()) {
    env.push_warning(d);
  }
  Ok(env)
}

fn go_selector_call_matches<'a>(
  root: &lmprobe_lang_core::langs::SgNode<'a>,
  name: &str,
) -> Vec<ast_grep_core::NodeMatch<'a, ast_grep_core::tree_sitter::StrDoc<SgLang>>> {
  root
    .dfs()
    .filter(|node| node.kind() == "call_expression")
    .map(ast_grep_core::NodeMatch::from)
    .filter(|m| go_selector_call_matches_name(&m.text(), name))
    .collect()
}

fn go_selector_call_matches_name(text: &str, name: &str) -> bool {
  let Some(before_args) = text.split('(').next() else {
    return false;
  };
  before_args
    .trim()
    .rsplit('.')
    .next()
    .is_some_and(|field| field == name)
}

// ──────────────────────────── imports ─────────────────────────────

/// Strip outer matched ASCII quotes (`"…"` or `'…'`). Returns the
/// stripped string and a flag indicating whether stripping occurred.
/// QA §2.2 — `imports "\"fmt\""` panicked because the quoted form
/// reached `Pattern::new`; we now strip and emit a `QUOTED_INPUT`
/// diagnostic.
fn strip_outer_quotes(s: &str) -> (String, bool) {
  let bytes = s.as_bytes();
  if bytes.len() >= 2
    && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
      || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
  {
    (s[1..s.len() - 1].to_string(), true)
  } else {
    (s.to_string(), false)
  }
}

pub fn imports(name: &str, root: &Path, opts: &WalkOpts) -> Result<ImportsResult> {
  let (clean_name, was_quoted) = strip_outer_quotes(name);
  let (tx, rx) = mpsc::channel();
  let unsupported_langs: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
  let parse_failures: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
  let too_large: Arc<Mutex<Vec<Diagnostic>>> = Arc::new(Mutex::new(Vec::new()));
  // QA TC-080a: see callers() above for the rationale.
  let any_supported_lang_walked: Arc<std::sync::atomic::AtomicBool> =
    Arc::new(std::sync::atomic::AtomicBool::new(false));
  let name_str = clean_name.clone();
  let mut wb = WalkBuilder::new(root);
  apply_walk_opts(&mut wb, opts, root);
  let walker = wb.build_parallel();
  walker.run(|| {
    let tx = tx.clone();
    let too_large = too_large.clone();
    let name_str = name_str.clone();
    let unsupported = unsupported_langs.clone();
    let pfail = parse_failures.clone();
    let any_supported = any_supported_lang_walked.clone();
    let opts = opts;
    Box::new(move |result| {
      if let Ok(entry) = result {
        let (path_buf, lang) = match walk_pre_check(&entry, opts) {
          PreCheck::Process(p, l) => (p, l),
          PreCheck::Skip => return ignore::WalkState::Continue,
          PreCheck::SkipWithDiag(d) => {
            too_large.lock().unwrap().push(d);
            return ignore::WalkState::Continue;
          }
        };
        let path = path_buf.as_path();
        {
          {
            if let Ok(content) = fs::read_to_string(path) {
              if !content.contains(&name_str) {
                return ignore::WalkState::Continue;
              }
              let grep = lang.ast_grep(content);
              let root_node = grep.root();
              // §2.2 — D5 ImportGrammar trait dispatch
              // (replaces the per-lang `match`; closes
              // the §0.4 NOT_IMPLEMENTED gap for the 12
              // new languages).
              let grammar = lmprobe_registry::import_grammar_for(lang);
              let patterns = grammar.import_patterns(&name_str);
              // Phase 6C round 3: per-lang opt-in for
              // post-match `content.contains(name)`
              // filter. Lifts grammar-level blockers
              // for elixir's qualified alias decomposition
              // and go's import-path basename matching.
              // Disabled by default — round-1 langs
              // unaffected.
              let needs_prefix_filter = grammar.needs_prefix_match();
              if patterns.is_empty() {
                unsupported.lock().unwrap().insert(format!("{:?}", lang));
                return ignore::WalkState::Continue;
              }
              any_supported.store(true, std::sync::atomic::Ordering::Relaxed);
              // Phase 6C round 3 — per-file hit dedup.
              // Multiple patterns can match the same
              // source line (e.g. round-1 narrow + round-3
              // broad); dedup by `(line_start, content)`
              // so each statement emits at most once.
              let mut seen: HashSet<(u32, String)> = HashSet::new();
              for pattern_str in patterns {
                let pat = match Pattern::try_new(&pattern_str, lang) {
                  Ok(p) => p,
                  Err(_) => {
                    pfail.lock().unwrap().insert(format!("{:?}", lang));
                    continue;
                  }
                };
                for m in root_node.find_all(pat) {
                  let text = m.text().to_string();
                  // Phase 6C round 3 — prefix
                  // filter. Simple substring check;
                  // see Go's lib.rs for the
                  // false-positive tradeoff
                  // (e.g. `imports fmt` may match
                  // `import "fmtutil"`). Documented.
                  if needs_prefix_filter && !text.contains(&name_str) {
                    continue;
                  }
                  let line_start = (m.start_pos().line() + 1) as u32;
                  if !seen.insert((line_start, text.clone())) {
                    continue;
                  }
                  let _ = tx.send(SymbolHit {
                    name: name_str.clone(),
                    kind: "import".to_string(),
                    lang: format!("{:?}", lang),
                    file_path: path.to_str().unwrap_or("").to_string(),
                    line_start,
                    content: text,
                    ..Default::default()
                  });
                }
              }
              // Phase 6C round 4 — block-form per-spec
              // DFS for grammars where pattern strings
              // can't enumerate spec positions inside
              // a list parent (tree-sitter-go's
              // `import_spec_list`).
              if let (true, Some((spec_kind, parent_kind))) =
                (needs_prefix_filter, grammar.block_form_spec_kind())
              {
                for n in root_node.dfs().filter(|n| {
                  n.kind() == spec_kind
                    && n.parent().map(|p| p.kind() == parent_kind).unwrap_or(false)
                }) {
                  let text = n.text().to_string();
                  if !text.contains(&name_str) {
                    continue;
                  }
                  let line_start = (n.start_pos().line() + 1) as u32;
                  if !seen.insert((line_start, text.clone())) {
                    continue;
                  }
                  let _ = tx.send(SymbolHit {
                    name: name_str.clone(),
                    kind: "import".to_string(),
                    lang: format!("{:?}", lang),
                    file_path: path.to_str().unwrap_or("").to_string(),
                    line_start,
                    content: text,
                    ..Default::default()
                  });
                }
              }
            }
          }
        }
      }
      ignore::WalkState::Continue
    })
  });
  drop(tx);
  let mut results: Vec<_> = rx.into_iter().collect();
  results.sort_by(|a, b| a.file_path.cmp(&b.file_path));
  let mut env = ImportsResult::new(results);
  if was_quoted {
    env.push_warning(
      Diagnostic::new(
        "QUOTED_INPUT",
        format!("stripped outer quotes from input {:?}", name),
      )
      .with_hint("pass the bare module name; lmprobe adds quoting per language"),
    );
  }
  let any_supported = any_supported_lang_walked.load(std::sync::atomic::Ordering::Relaxed);
  let unsupported = std::mem::take(&mut *unsupported_langs.lock().unwrap());
  let had_unsupported = !unsupported.is_empty();
  for lang in unsupported {
    env.push_warning(
      Diagnostic::new(
        "NOT_IMPLEMENTED",
        "imports grammar not implemented for language",
      )
      .with_lang(lang),
    );
  }
  let pfails = std::mem::take(&mut *parse_failures.lock().unwrap());
  for lang in pfails {
    env.push_warning(
      Diagnostic::new(
        "PATTERN_PARSE_FAILED",
        "import pattern failed to parse for language",
      )
      .with_lang(lang),
    );
  }
  for d in std::mem::take(&mut *too_large.lock().unwrap()) {
    env.push_warning(d);
  }
  // QA TC-080a: promote when no supported lang walked + had NOT_IMPLEMENTED.
  if !any_supported && had_unsupported && env.exit_status == ExitStatus::NoMatch {
    env.exit_status = ExitStatus::Matched;
  }
  Ok(env)
}

// ────────────────────────── decorations ───────────────────────────

pub fn decorations(name: &str, root: &Path, opts: &WalkOpts) -> Result<DecorationsResult> {
  let (tx, rx) = mpsc::channel();
  let too_large: Arc<Mutex<Vec<Diagnostic>>> = Arc::new(Mutex::new(Vec::new()));
  let unsupported_langs: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
  let parse_failures: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
  // QA TC-080a: see callers() above for the rationale.
  let any_supported_lang_walked: Arc<std::sync::atomic::AtomicBool> =
    Arc::new(std::sync::atomic::AtomicBool::new(false));
  let name_str = name.to_string();
  let mut wb = WalkBuilder::new(root);
  apply_walk_opts(&mut wb, opts, root);
  let walker = wb.build_parallel();
  walker.run(|| {
    let tx = tx.clone();
    let too_large = too_large.clone();
    let name_str = name_str.clone();
    let unsupported = unsupported_langs.clone();
    let pfail = parse_failures.clone();
    let any_supported = any_supported_lang_walked.clone();
    let opts = opts;
    Box::new(move |result| {
      if let Ok(entry) = result {
        let (path_buf, lang) = match walk_pre_check(&entry, opts) {
          PreCheck::Process(p, l) => (p, l),
          PreCheck::Skip => return ignore::WalkState::Continue,
          PreCheck::SkipWithDiag(d) => {
            too_large.lock().unwrap().push(d);
            return ignore::WalkState::Continue;
          }
        };
        let path = path_buf.as_path();
        {
          {
            if let Ok(content) = fs::read_to_string(path) {
              if !content.contains(&name_str) {
                return ignore::WalkState::Continue;
              }
              let grep = lang.ast_grep(content);
              let root_node = grep.root();
              let patterns = decoration_grammar_for(lang).decoration_patterns(&name_str);
              if patterns.is_empty() {
                unsupported.lock().unwrap().insert(format!("{:?}", lang));
                return ignore::WalkState::Continue;
              }
              any_supported.store(true, std::sync::atomic::Ordering::Relaxed);
              for pattern_str in patterns {
                let pat = match Pattern::try_new(&pattern_str, lang) {
                  Ok(p) => p,
                  Err(_) => {
                    pfail.lock().unwrap().insert(format!("{:?}", lang));
                    continue;
                  }
                };
                for m in root_node.find_all(pat) {
                  let _ = tx.send(SymbolHit {
                    name: name_str.clone(),
                    kind: "decoration".to_string(),
                    lang: format!("{:?}", lang),
                    file_path: path.to_str().unwrap_or("").to_string(),
                    line_start: (m.start_pos().line() + 1) as u32,
                    content: m.text().to_string(),
                    ..Default::default()
                  });
                }
              }
            }
          }
        }
      }
      ignore::WalkState::Continue
    })
  });
  drop(tx);
  let mut results: Vec<_> = rx.into_iter().collect();
  results.sort_by(|a, b| a.file_path.cmp(&b.file_path));
  let any_supported = any_supported_lang_walked.load(std::sync::atomic::Ordering::Relaxed);
  let mut env = DecorationsResult::new(results);
  let unsupported = std::mem::take(&mut *unsupported_langs.lock().unwrap());
  let had_unsupported = !unsupported.is_empty();
  for lang in unsupported {
    env.push_warning(
      Diagnostic::new(
        "NOT_IMPLEMENTED",
        "decorations grammar not implemented for language",
      )
      .with_lang(lang),
    );
  }
  let pfails = std::mem::take(&mut *parse_failures.lock().unwrap());
  for lang in pfails {
    env.push_warning(
      Diagnostic::new(
        "PATTERN_PARSE_FAILED",
        "decoration pattern failed to parse for language",
      )
      .with_lang(lang),
    );
  }
  for d in std::mem::take(&mut *too_large.lock().unwrap()) {
    env.push_warning(d);
  }
  // QA TC-080a: promote when no supported lang walked + had NOT_IMPLEMENTED.
  if !any_supported && had_unsupported && env.exit_status == ExitStatus::NoMatch {
    env.exit_status = ExitStatus::Matched;
  }
  Ok(env)
}

// ──────────────────────────── evolution ───────────────────────────

/// §1.8 — `--regex` opt-in mirrors def/ref. With `regex_mode` true,
/// the symbol name is matched as a case-sensitive substring; with
/// false (default), exact-token match. The CLI dispatcher passes
/// `regex` straight through; GraphQL passes `false`.
/// Phase 6A #1 (round-2): single source for partial-clone detection.
///
/// Two forms of incomplete clones produce truncated history:
/// - `--depth N` shallow clones: `.git/shallow` exists; detected via
///   `Repository::is_shallow()`.
/// - `--filter=blob:none` (and friends) partial clones: `.git/shallow`
///   does NOT exist, but `extensions.partialclone` and/or
///   `remote.<name>.partialclonefilter` are set in the repo config.
///
/// Either case → emit the same `EVOLUTION_PARTIAL_CLONE` Diagnostic
/// once, up-front. Called before the cache lookup so cache-hit early-
/// returns also surface the warning.
fn detect_partial_clone(repo: &git2::Repository) -> Option<Diagnostic> {
  let is_shallow = repo.is_shallow();
  let has_partial_filter = repo
    .config()
    .ok()
    .map(|cfg| {
      // `extensions.partialclone` is the canonical signal that
      // git itself sets; `remote.<X>.partialclonefilter` carries
      // the filter spec (e.g. "blob:none"). Either one set →
      // partial clone.
      cfg.get_string("extensions.partialclone").is_ok()
        || cfg.get_string("remote.origin.partialclonefilter").is_ok()
    })
    .unwrap_or(false);
  if !is_shallow && !has_partial_filter {
    return None;
  }
  let kind = match (is_shallow, has_partial_filter) {
    (true, true) => "shallow + partial-filter clone",
    (true, false) => "shallow clone",
    (false, true) => "partial-filter clone (e.g. --filter=blob:none)",
    (false, false) => unreachable!(),
  };
  Some(
        Diagnostic::new(
            "EVOLUTION_PARTIAL_CLONE",
            format!(
                "repository is a {} — evolution walks only fetched commits/blobs; results may be missing earlier milestones (INTRODUCED may classify a non-introducing commit)",
                kind
            ),
        )
        .with_hint(
            "`git fetch --unshallow` (or refetch without `--filter=…`) for full history; pass `--no-cache --force` once history is complete",
        ),
    )
}

pub fn evolution_with_regex(
  name: &str,
  root: &Path,
  force: bool,
  no_cache: bool,
  regex_mode: bool,
  opts: &WalkOpts,
) -> Result<EvolutionResult> {
  let mut warnings: Vec<Diagnostic> = Vec::new();

  // Phase 6A #1 (round-2): partial-clone detection up-front, BEFORE
  // the cache lookup, so cache-hit early-returns still surface the
  // warning. Both `--depth N` and `--filter=blob:none` produce
  // truncated history — `detect_partial_clone` handles both.
  let repo = Repository::discover(root).context("Could not find git repository")?;
  if let Some(d) = detect_partial_clone(&repo) {
    warnings.push(d);
  }

  // §0.10 — XDG cache. Read order: XDG (preferred), then legacy
  // back-compat (with a deprecation warning) when XDG misses.
  let xdg_db = xdg_history_db_path();
  let legacy_db = legacy_history_db_path(root);

  if !no_cache && !force {
    // Try XDG first.
    if xdg_db.exists() {
      if let Ok(conn) = init_history_db(&xdg_db) {
        if let Some(rows) = read_milestones(&conn, name)? {
          let mut env = EvolutionResult::new(name.to_string(), rows);
          for w in warnings.drain(..) {
            env.push_warning(w);
          }
          return Ok(env);
        }
      }
    }
    // Fall back to legacy and emit deprecation warning.
    if legacy_db.exists() {
      if let Ok(conn) = init_history_db(&legacy_db) {
        if let Some(rows) = read_milestones(&conn, name)? {
          let mut env = EvolutionResult::new(name.to_string(), rows);
          for w in warnings.drain(..) {
            env.push_warning(w);
          }
          env.push_warning(
            Diagnostic::new(
              "DEPRECATED_CACHE_PATH",
              format!(
                "read milestones from legacy cache at {}",
                legacy_db.display()
              ),
            )
            .with_hint("delete <root>/.lmprobe/ — new writes go to $XDG_CACHE_HOME/lmprobe/"),
          );
          return Ok(env);
        }
      }
    }
  }

  // No cached milestones; walk history.
  let conn = if no_cache {
    None
  } else {
    // Make sure XDG dir exists and DB is open.
    std::fs::create_dir_all(xdg_cache_dir())?;
    Some(init_history_db(&xdg_db)?)
  };

  let mut revwalk = repo.revwalk()?;
  revwalk.push_head()?;
  // §1.8 — walk OLDEST first so the first commit containing the
  // symbol's AST is recognised as INTRODUCED. Closes QA §2.5.
  revwalk.set_sorting(git2::Sort::TIME | git2::Sort::REVERSE)?;

  let (current_matches, _) = search_symbol_hits(
    root,
    true,
    SearchConstraints {
      query: Some(name.to_string()),
      exact_name: !regex_mode,
      ..Default::default()
    },
    opts,
  )?;
  if current_matches.is_empty() {
    return Err(anyhow!("Symbol not found in current HEAD"));
  }
  let file_path = &current_matches[0].file_path;
  let rel_file_path = Path::new(file_path)
    .strip_prefix(fs::canonicalize(root)?)
    .unwrap_or_else(|_| {
      Path::new(file_path)
        .strip_prefix(root)
        .unwrap_or(Path::new(file_path))
    })
    .to_path_buf();

  let mut milestones: Vec<Milestone> = Vec::new();
  let mut last_signature = String::new();
  let symbol_matches = |candidate: &str| -> bool {
    if regex_mode {
      candidate.contains(name)
    } else {
      candidate == name
    }
  };

  for commit_id in revwalk {
    let commit_id = commit_id?;
    let commit = match repo.find_commit(commit_id) {
      Ok(c) => c,
      Err(_) => {
        // Phase 6A #1 (round-2): commit-object missing — likely
        // a partial clone. Up-front warning already informed
        // the user; skip this commit and keep walking.
        continue;
      }
    };
    let tree = match commit.tree() {
      Ok(t) => t,
      Err(_) => continue,
    };
    let entry = match tree.get_path(&rel_file_path) {
      Ok(e) => e,
      Err(_) => {
        // The file didn't exist in this older commit yet —
        // skip and keep walking forward in time.
        continue;
      }
    };
    let blob = match repo.find_blob(entry.id()) {
      Ok(b) => b,
      Err(_) => {
        // Phase 6A #1 (round-2): blob-object missing — typical
        // of `--filter=blob:none` partial clones. Up-front
        // warning already informed the user; skip this commit
        // and keep walking forward in time.
        continue;
      }
    };
    let content = String::from_utf8_lossy(blob.content()).to_string();

    let lang = SgLang::from_path(&rel_file_path).unwrap_or(SgLang::Rust);
    let grep = lang.ast_grep(content);
    let root_node = grep.root();
    let extractor = name_extractor_for(lang);

    let mut current_signature: Option<String> = None;
    'kinds: for kind in EVOLUTION_KINDS {
      let matches = root_node.find_all(KindMatcher::new(kind, lang));
      for m in matches {
        let symbol_name = extractor.extract(&m).unwrap_or_default();
        if symbol_matches(&symbol_name) {
          current_signature = Some(create_skeleton(&m));
          break 'kinds;
        }
      }
    }

    let signature = match current_signature {
      // Symbol absent in this commit — keep walking; the next
      // commit where it appears is the introduction.
      None => continue,
      Some(s) => s,
    };

    if signature != last_signature {
      let change_type = if last_signature.is_empty() {
        "INTRODUCED"
      } else {
        "SIGNATURE_CHANGE"
      };
      let mstone = Milestone {
        symbol_name: name.to_string(),
        commit_hash: commit_id.to_string(),
        author: commit.author().name().unwrap_or("unknown").to_string(),
        timestamp: chrono::DateTime::from_timestamp(commit.time().seconds(), 0)
          .unwrap()
          .to_rfc3339(),
        change_type: change_type.to_string(),
        signature: signature.clone(),
      };
      if let Some(c) = &conn {
        c.execute(
          "INSERT OR REPLACE INTO symbol_milestones \
                     (symbol_name, commit_hash, author, timestamp, change_type, signature) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
          params![
            mstone.symbol_name,
            mstone.commit_hash,
            mstone.author,
            mstone.timestamp,
            mstone.change_type,
            mstone.signature
          ],
        )?;
      }
      milestones.push(mstone);
      last_signature = signature;
    }
  }
  // We walked oldest-first; rendering & cache convention is
  // newest-first, so reverse before returning.
  milestones.reverse();
  let mut env = EvolutionResult::new(name.to_string(), milestones);
  for w in warnings.drain(..) {
    env.push_warning(w);
  }
  Ok(env)
}

fn read_milestones(conn: &rusqlite::Connection, name: &str) -> Result<Option<Vec<Milestone>>> {
  let mut stmt = conn.prepare(
    "SELECT symbol_name, commit_hash, author, timestamp, change_type, signature \
         FROM symbol_milestones WHERE symbol_name = ?1 ORDER BY timestamp DESC",
  )?;
  let rows = stmt
    .query_map([name], |row| {
      Ok(Milestone {
        symbol_name: row.get(0)?,
        commit_hash: row.get(1)?,
        author: row.get(2)?,
        timestamp: row.get(3)?,
        change_type: row.get(4)?,
        signature: row.get(5)?,
      })
    })?
    .collect::<std::result::Result<Vec<_>, _>>()?;
  if rows.is_empty() {
    Ok(None)
  } else {
    Ok(Some(rows))
  }
}

// ──────────────────────────── trace ───────────────────────────────

/// Simple FNV-1a 64-bit. Used for the trace audit log so we can pin
/// pre-instrument file states without dragging in a sha256 dep.
fn fnv1a_64(bytes: &[u8]) -> u64 {
  let mut h: u64 = 0xcbf29ce484222325;
  for &b in bytes {
    h ^= b as u64;
    h = h.wrapping_mul(0x100000001b3);
  }
  h
}

/// §0.9 / §CC5 — refuse to mutate when the working tree is dirty or
/// the directory isn't a git repo. `--force` overrides both. The
/// audit log path is returned in the Diagnostic hint so users can
/// find pre-instrument hashes if rollback fails.
///
/// TODO: §4 `fix` reuses this contract; keep the surface stable.
pub fn check_trace_safety(root: &Path, force: bool) -> std::result::Result<(), Diagnostic> {
  match Repository::discover(root) {
    Err(_) => {
      if force {
        return Ok(());
      }
      Err(
        Diagnostic::new(
          "NOT_GIT_REPO",
          "trace requires a git repository (no rollback target without one)",
        )
        .with_hint(
          "re-run with --force to instrument anyway (audit log will still record file hashes)",
        ),
      )
    }
    Ok(repo) => {
      if force {
        return Ok(());
      }
      // §0.9 — `git status --porcelain` (which is the plan's
      // literal contract) includes untracked by default, so an
      // `add untracked.txt` followed by `trace` should refuse.
      // Reviewer must-fix #1.
      let mut opts = StatusOptions::new();
      opts.include_untracked(true);
      opts.include_ignored(false);
      let statuses = match repo.statuses(Some(&mut opts)) {
        Ok(s) => s,
        Err(e) => {
          return Err(Diagnostic::new(
            "GIT_STATUS_FAILED",
            format!("could not read git status: {}", e),
          ))
        }
      };
      if !statuses.is_empty() {
        return Err(
          Diagnostic::new(
            "DIRTY_TREE",
            "trace refuses to instrument a dirty working tree",
          )
          .with_hint("commit/stash your changes, or re-run with --force"),
        );
      }
      Ok(())
    }
  }
}

fn write_trace_audit_log(files: &[PathBuf]) -> Result<PathBuf> {
  let ts = chrono::Local::now().format("%Y%m%dT%H%M%S").to_string();
  let log_path = trace_audit_log_path(&ts);
  if let Some(parent) = log_path.parent() {
    std::fs::create_dir_all(parent)?;
  }
  let mut lines = Vec::new();
  lines.push(format!("# lmprobe trace audit log {}", ts));
  for f in files {
    let bytes = std::fs::read(f).unwrap_or_default();
    let hash = fnv1a_64(&bytes);
    lines.push(format!("{:016x}  {}", hash, f.display()));
  }
  std::fs::write(&log_path, lines.join("\n"))?;
  Ok(log_path)
}

/// Phase 4B §4.2 trace safety options.
pub struct TraceSafety {
  /// `--worktree [path]`. None = in-place (legacy behavior).
  /// Some(None) = auto temp-dir under XDG.
  /// Some(Some(path)) = caller-specified path.
  pub worktree: Option<Option<PathBuf>>,
  /// `--keep-patch [path]`. Same convention as worktree.
  pub keep_patch: Option<Option<PathBuf>>,
}

impl Default for TraceSafety {
  fn default() -> Self {
    Self {
      worktree: None,
      keep_patch: None,
    }
  }
}

#[derive(Clone, Debug)]
pub struct TraceWatchOptions {
  pub watch: Vec<String>,
  pub json_trace: bool,
  pub include_shape: bool,
  pub include_len: bool,
  pub include_id: bool,
  pub repr_limit: usize,
}

impl Default for TraceWatchOptions {
  fn default() -> Self {
    Self {
      watch: Vec::new(),
      json_trace: false,
      include_shape: false,
      include_len: false,
      include_id: false,
      repr_limit: 120,
    }
  }
}

impl TraceWatchOptions {
  fn enabled(&self) -> bool {
    !self.watch.is_empty()
  }
}

pub fn trace_with_safety(
  query: &str,
  root: &Path,
  exec: &str,
  template: Option<String>,
  force: bool,
  safety: &TraceSafety,
  opts: &WalkOpts,
) -> Result<TraceRunResult> {
  trace_with_safety_and_watch(
    query,
    root,
    exec,
    template,
    force,
    safety,
    opts,
    &TraceWatchOptions::default(),
  )
}

pub fn trace_with_safety_and_watch(
  query: &str,
  root: &Path,
  exec: &str,
  template: Option<String>,
  force: bool,
  safety: &TraceSafety,
  opts: &WalkOpts,
  watch: &TraceWatchOptions,
) -> Result<TraceRunResult> {
  if let Err(diag) = validate_trace_watch_options(watch) {
    return Ok(TraceRunResult::refusal(diag));
  }

  // §0.9 / Lead arbitration: refuse on dirty tree OR non-git dir
  // ONLY when running in-place. With `--worktree`, the dirty
  // refusal is moot — we operate on a fresh clone.
  if safety.worktree.is_none() {
    if let Err(diag) = check_trace_safety(root, force) {
      return Ok(TraceRunResult::refusal(diag));
    }
  }

  // 4B — provision a worktree if requested. The instrumentation,
  // exec, and rollback all run inside the worktree; the user's
  // tree is untouched.
  let (effective_root, worktree_meta) = if let Some(wt_opt) = &safety.worktree {
    match provision_trace_worktree(root, wt_opt.as_deref()) {
      Ok(wt) => {
        let er = map_trace_root_into_worktree(root, &wt.parent, &wt.path);
        (er, Some(wt))
      }
      Err(diag) => return Ok(TraceRunResult::refusal(diag)),
    }
  } else {
    (root.to_path_buf(), None)
  };
  let root = effective_root.as_path();
  let exec_cwd = worktree_meta.as_ref().map(|wt| wt.path.clone());

  // 1. Discovery
  let mut files_to_instrument = Vec::new();
  let mut warnings = Vec::new();
  let mut trace_watch_unsupported_langs: HashSet<String> = HashSet::new();
  if root.is_file() {
    if watch.enabled() && is_r_watch_path(root) {
      if let Ok(content) = fs::read_to_string(root) {
        if r_line_query_matches(&content, query) {
          files_to_instrument.push(root.to_path_buf());
        }
      }
    } else if let Some(lang) = SgLang::from_path(root) {
      if let Ok(content) = fs::read_to_string(root) {
        let grep = lang.ast_grep(content);
        if grep.root().find_all(query).next().is_some() {
          if watch.enabled() && !trace_watch_supports_lang(lang) {
            trace_watch_unsupported_langs.insert(format!("{:?}", lang));
          } else {
            files_to_instrument.push(root.to_path_buf());
          }
        }
      }
    }
  } else {
    let mut wb = WalkBuilder::new(root);
    apply_walk_opts(&mut wb, opts, root);
    let walker = wb.build();
    for entry in walker {
      if let Ok(entry) = entry {
        let path = entry.path();
        if path.is_file() {
          if watch.enabled() && is_r_watch_path(path) {
            if let Ok(content) = fs::read_to_string(path) {
              if r_line_query_matches(&content, query) {
                files_to_instrument.push(path.to_path_buf());
              }
            }
          } else if let Some(lang) = SgLang::from_path(path) {
            if let Ok(content) = fs::read_to_string(path) {
              let grep = lang.ast_grep(content);
              if grep.root().find_all(query).next().is_some() {
                if watch.enabled() && !trace_watch_supports_lang(lang) {
                  trace_watch_unsupported_langs.insert(format!("{:?}", lang));
                } else {
                  files_to_instrument.push(path.to_path_buf());
                }
              }
            }
          }
        }
      }
    }
  }

  for lang in &trace_watch_unsupported_langs {
    warnings.push(trace_watch_language_diagnostic(lang.clone()));
  }
  if files_to_instrument.is_empty() {
    let mut env = TraceRunResult::new(vec![]);
    if !trace_watch_unsupported_langs.is_empty() {
      env = env.with_status(ExitStatus::Error);
    }
    for warning in warnings {
      env.push_warning(warning);
    }
    return Ok(env);
  }
  if let Err(diag) = validate_trace_watch_names_for_paths(watch, &files_to_instrument) {
    if let Some(wt) = &worktree_meta {
      let _ = remove_trace_worktree(wt);
    }
    return Ok(TraceRunResult::refusal(diag));
  }

  // §0.9 — audit log written before any mutation, so a hard crash
  // mid-instrument leaves a recovery breadcrumb.
  let audit_path = write_trace_audit_log(&files_to_instrument)?;
  warnings.push(
    Diagnostic::new(
      "TRACE_AUDIT_LOG",
      format!("audit log written to {}", audit_path.display()),
    )
    .with_hint("delete after the trace completes successfully"),
  );

  // 2. State Guard (Manual Backup)
  let mut backups = Vec::new();
  for path in &files_to_instrument {
    let mut bak_path: PathBuf = path.clone();
    bak_path.set_extension(format!(
      "{}.bak",
      path.extension().unwrap_or_default().to_str().unwrap_or("")
    ));
    fs::copy(path, &bak_path)?;
    backups.push((path.clone(), bak_path));
  }

  let mut events: Vec<TraceEvent> = Vec::new();
  let mut no_template_langs: HashSet<String> = HashSet::new();

  // 3. Injection
  for path in &files_to_instrument {
    let content = fs::read_to_string(path)?;
    if watch.enabled() {
      let instrumented_content = if is_r_watch_path(path) {
        instrument_r_watch_content(&content, query, watch)?
      } else if let Some(lang) = SgLang::from_path(path) {
        let instrumented_content = match lang {
          SgLang::Python => instrument_python_watch_content(&content, query, watch)?,
          SgLang::Bash => instrument_bash_watch_content(&content, query, watch)?,
          _ => continue,
        };
        instrumented_content
      } else {
        continue;
      };
      fs::write(path, instrumented_content.as_bytes())?;
      continue;
    }

    if let Some(lang) = SgLang::from_path(path) {
      // §0.7 / QA §2.11 — refuse trace for languages without a
      // sensible default template (caller can still pass
      // `--template`).
      let t: String = if let Some(s) = template.as_deref() {
        s.to_string()
      } else {
        match trace_template_for(lang).template_for("VAR") {
          Some(s) => s,
          None => {
            no_template_langs.insert(format!("{:?}", lang));
            continue;
          }
        }
      };

      let grep_mut = lang.ast_grep(content);
      let edits = grep_mut.root().replace_all(query, t.as_str());
      let instrumented_content = if !edits.is_empty() {
        let mut source = grep_mut.source().to_string();
        for edit in edits.into_iter().rev() {
          source.replace_range(
            edit.position..edit.position + edit.deleted_length,
            &String::from_utf8_lossy(&edit.inserted_text),
          );
        }
        source
      } else {
        grep_mut.source().to_string()
      };
      fs::write(path, instrumented_content.as_bytes())?;
    }
  }

  // 4. Execution
  let output = if cfg!(target_os = "windows") {
    let mut command = Command::new("cmd");
    command.args(["/C", exec]);
    if let Some(cwd) = &exec_cwd {
      command.current_dir(cwd);
    }
    command.output()?
  } else {
    let mut command = Command::new("sh");
    command.args(["-c", exec]);
    if let Some(cwd) = &exec_cwd {
      command.current_dir(cwd);
    }
    command.output()?
  };

  // 5. Capture
  let stdout = String::from_utf8_lossy(&output.stdout);
  let stderr = String::from_utf8_lossy(&output.stderr);
  for line in stdout.lines().chain(stderr.lines()) {
    if let Some(event) = parse_trace_json_event(line) {
      events.push(event);
    } else if line.contains("[TRACE]") {
      events.push(trace_event(line.replace("[TRACE]", "").trim().to_string()));
    }
  }

  // 5b. (4B) Capture patch BEFORE rollback if --keep-patch.
  let captured_patch = if let Some(kp_opt) = &safety.keep_patch {
    let patch_root = exec_cwd.as_deref().unwrap_or(root);
    match capture_patch(patch_root, &files_to_instrument, kp_opt.as_deref()) {
      Ok(p) => Some(p),
      Err(d) => {
        warnings.push(d);
        None
      }
    }
  } else {
    None
  };

  // 6. Rollback (Restore from backup)
  for (orig, bak) in backups {
    fs::copy(&bak, &orig)?;
    let _ = fs::remove_file(bak);
  }

  // 7. (4B) Worktree cleanup. Preserve on failure for debugging.
  let mut worktree_path_str: Option<String> = None;
  if let Some(wt) = worktree_meta {
    worktree_path_str = Some(wt.path.to_string_lossy().into_owned());
    let exec_succeeded = output.status.success();
    if exec_succeeded {
      let _ = remove_trace_worktree(&wt);
    } else {
      warnings.push(
        Diagnostic::new(
          "WORKTREE_PRESERVED",
          format!(
            "trace exec failed; worktree preserved at {}",
            wt.path.display()
          ),
        )
        .with_hint("inspect, then `git worktree remove --force` when done"),
      );
    }
    warnings.push(Diagnostic::new(
      "WORKTREE_USED",
      "instrumentation ran in an isolated git worktree; user tree untouched",
    ));
  }

  let mut env = TraceRunResult::new(events);
  env.worktree_path = worktree_path_str;
  env.patch_path = captured_patch.map(|p| p.to_string_lossy().into_owned());
  for w in warnings.drain(..) {
    env.push_warning(w);
  }
  for lang in no_template_langs {
    env.push_warning(
      Diagnostic::new(
        "NO_TRACE_TEMPLATE",
        "no default trace template for language; pass --template",
      )
      .with_lang(lang),
    );
    env = env.with_status(ExitStatus::Error);
  }
  Ok(env)
}

fn map_trace_root_into_worktree(
  requested_root: &Path,
  repo_root: &Path,
  worktree_root: &Path,
) -> PathBuf {
  let requested_abs = if requested_root.is_absolute() {
    requested_root.to_path_buf()
  } else {
    std::env::current_dir()
      .unwrap_or_else(|_| PathBuf::from("."))
      .join(requested_root)
  };
  let requested_abs = requested_abs.canonicalize().unwrap_or(requested_abs);
  let repo_root = repo_root
    .canonicalize()
    .unwrap_or_else(|_| repo_root.to_path_buf());
  requested_abs
    .strip_prefix(&repo_root)
    .map(|rel| worktree_root.join(rel))
    .unwrap_or_else(|_| worktree_root.to_path_buf())
}

fn trace_watch_language_diagnostic(lang: String) -> Diagnostic {
  Diagnostic::new(
    "TRACE_WATCH_LANG_UNSUPPORTED",
    "trace watch JSON mode currently supports Python, Bash, and line-based R files only",
  )
  .with_hint(
    "run watch mode on Python, Bash, or .R/.r files, or use legacy `trace --template` for other languages",
  )
  .with_lang(lang)
}

fn trace_watch_supports_lang(lang: SgLang) -> bool {
  matches!(lang, SgLang::Python | SgLang::Bash)
}

fn validate_trace_watch_options(watch: &TraceWatchOptions) -> std::result::Result<(), Diagnostic> {
  if !watch.enabled() {
    return Ok(());
  }
  if !watch.json_trace {
    return Err(
      Diagnostic::new(
        "TRACE_JSON_REQUIRED",
        "`trace --watch` requires `--json-trace` so runtime evidence is machine-readable",
      )
      .with_hint("add `--json-trace`, or remove `--watch` to use legacy trace templates"),
    );
  }
  for name in &watch.watch {
    if !is_simple_python_identifier(name) && !is_simple_r_watch_name(name) {
      return Err(trace_watch_identifier_diagnostic(name));
    }
  }
  Ok(())
}

fn validate_trace_watch_names_for_paths(
  watch: &TraceWatchOptions,
  paths: &[PathBuf],
) -> std::result::Result<(), Diagnostic> {
  if !watch.enabled() {
    return Ok(());
  }
  for path in paths {
    if is_r_watch_path(path) {
      for name in &watch.watch {
        if !is_simple_r_watch_name(name) {
          return Err(trace_watch_identifier_diagnostic(name));
        }
      }
      continue;
    }
    if let Some(lang) = SgLang::from_path(path) {
      if matches!(lang, SgLang::Python | SgLang::Bash) {
        for name in &watch.watch {
          if !is_simple_python_identifier(name) {
            return Err(trace_watch_identifier_diagnostic(name));
          }
        }
      }
    }
  }
  Ok(())
}

fn trace_watch_identifier_diagnostic(name: &str) -> Diagnostic {
  Diagnostic::new(
    "TRACE_WATCH_IDENTIFIER_UNSUPPORTED",
    format!("trace watch only supports simple Python/Bash identifiers or dotted R names: {name}"),
  )
  .with_hint(
    "use names like `value` for Python/Bash and `value` or `sales.total` for R; expressions, subscripts, and Python/Bash dotted paths are not supported yet",
  )
}

fn is_simple_r_watch_name(name: &str) -> bool {
  let mut parts = name.split('.');
  let Some(first) = parts.next() else {
    return false;
  };
  if !is_simple_python_identifier(first) {
    return false;
  }
  parts.all(is_simple_python_identifier)
}

fn is_simple_python_identifier(name: &str) -> bool {
  let mut chars = name.chars();
  let Some(first) = chars.next() else {
    return false;
  };
  (first == '_' || first.is_ascii_alphabetic())
    && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn trace_event(output: String) -> TraceEvent {
  TraceEvent {
    timestamp: chrono::Local::now().to_rfc3339(),
    output,
    var: None,
    type_name: None,
    id: None,
    len: None,
    shape: None,
    repr: None,
  }
}

fn parse_trace_json_event(line: &str) -> Option<TraceEvent> {
  let (_, payload) = line.split_once("[TRACE_JSON]")?;
  let payload = payload.trim();
  let value: serde_json::Value = serde_json::from_str(payload).ok()?;
  Some(TraceEvent {
    timestamp: chrono::Local::now().to_rfc3339(),
    output: payload.to_string(),
    var: value
      .get("var")
      .and_then(|v| v.as_str())
      .map(|s| s.to_string()),
    type_name: value
      .get("type")
      .and_then(|v| v.as_str())
      .map(|s| s.to_string()),
    id: value
      .get("id")
      .and_then(|v| v.as_str())
      .map(|s| s.to_string()),
    len: value
      .get("len")
      .and_then(|v| v.as_u64())
      .and_then(|n| usize::try_from(n).ok()),
    shape: value.get("shape").and_then(|v| {
      let values = v.as_array()?;
      let mut dims = Vec::new();
      for value in values {
        dims.push(usize::try_from(value.as_u64()?).ok()?);
      }
      Some(dims)
    }),
    repr: value
      .get("repr")
      .and_then(|v| v.as_str())
      .map(|s| s.to_string()),
  })
}

fn instrument_python_watch_content(
  content: &str,
  query: &str,
  watch: &TraceWatchOptions,
) -> Result<String> {
  let grep = SgLang::Python.ast_grep(content.to_string());
  let root = grep.root();
  let mut insertions = Vec::new();
  for m in root.find_all(query) {
    let start_line = m.start_pos().line();
    let end_line = m.end_pos().line();
    let offset = insertion_offset_after_line(content, end_line);
    let indent = line_indent(content, start_line);
    let mut block = render_python_watch_block(&indent, watch)?;
    if offset == content.len() && !content.ends_with('\n') {
      block.insert(0, '\n');
    }
    insertions.push((offset, block));
  }

  if insertions.is_empty() {
    return Ok(content.to_string());
  }

  insertions.sort_by(|a, b| b.0.cmp(&a.0));
  let mut source = content.to_string();
  for (offset, block) in insertions {
    source.insert_str(offset, &block);
  }
  Ok(source)
}

fn insertion_offset_after_line(source: &str, line: usize) -> usize {
  let mut offset = 0;
  for (idx, chunk) in source.split_inclusive('\n').enumerate() {
    offset += chunk.len();
    if idx == line {
      return offset;
    }
  }
  source.len()
}

fn line_indent(source: &str, line: usize) -> String {
  source
    .lines()
    .nth(line)
    .unwrap_or("")
    .chars()
    .take_while(|c| *c == ' ' || *c == '\t')
    .collect()
}

fn render_python_watch_block(indent: &str, watch: &TraceWatchOptions) -> Result<String> {
  let mut out = String::new();
  for var in &watch.watch {
    let payload = python_watch_payload_expr(var, watch)?;
    let error_payload = python_watch_error_payload_expr(var, watch)?;
    out.push_str(&format!("{indent}try:\n"));
    out.push_str(&format!(
      "{indent}    print(\"[TRACE_JSON] \" + __import__(\"json\").dumps({payload}, sort_keys=True), flush=True)\n"
    ));
    out.push_str(&format!(
      "{indent}except Exception as __lmprobe_trace_exc:\n"
    ));
    out.push_str(&format!(
      "{indent}    print(\"[TRACE_JSON] \" + __import__(\"json\").dumps({error_payload}, sort_keys=True), flush=True)\n"
    ));
  }
  Ok(out)
}

fn python_watch_payload_expr(var: &str, watch: &TraceWatchOptions) -> Result<String> {
  let var_json = serde_json::to_string(var)
    .map_err(|e| lmprobe_types::error::anyhow!("serialize trace watch var: {}", e))?;
  let mut fields = vec![
    format!("\"var\": {var_json}"),
    format!("\"type\": type({var}).__name__"),
    format!("\"repr\": repr({var})[:{}]", watch.repr_limit),
  ];
  if watch.include_id {
    fields.push(format!("\"id\": hex(id({var}))"));
  }
  if watch.include_len {
    fields.push(format!(
      "\"len\": (len({var}) if hasattr({var}, \"__len__\") else None)"
    ));
  }
  if watch.include_shape {
    fields.push(format!(
      "\"shape\": (list(getattr({var}, \"shape\")) if getattr({var}, \"shape\", None) is not None else None)"
    ));
  }
  Ok(format!("{{{}}}", fields.join(", ")))
}

fn python_watch_error_payload_expr(var: &str, watch: &TraceWatchOptions) -> Result<String> {
  let var_json = serde_json::to_string(var)
    .map_err(|e| lmprobe_types::error::anyhow!("serialize trace watch var: {}", e))?;
  Ok(format!(
    "{{\"var\": {var_json}, \"type\": \"error\", \"repr\": repr(__lmprobe_trace_exc)[:{}]}}",
    watch.repr_limit
  ))
}

fn instrument_bash_watch_content(
  content: &str,
  query: &str,
  watch: &TraceWatchOptions,
) -> Result<String> {
  let grep = SgLang::Bash.ast_grep(content.to_string());
  let root = grep.root();
  let mut insertions = Vec::new();
  for m in root.find_all(query) {
    let start_line = m.start_pos().line();
    let end_line = m.end_pos().line();
    let matched_text = m.text();
    if !match_covers_complete_lines(content, start_line, end_line, matched_text.as_ref()) {
      continue;
    }
    let offset = insertion_offset_after_line(content, end_line);
    let indent = line_indent(content, start_line);
    let mut block = render_bash_watch_block(&indent, watch)?;
    if offset == content.len() && !content.ends_with('\n') {
      block.insert(0, '\n');
    }
    insertions.push((offset, block));
  }

  if insertions.is_empty() {
    return Ok(content.to_string());
  }

  insertions.sort_by(|a, b| b.0.cmp(&a.0));
  let mut source = content.to_string();
  for (offset, block) in insertions {
    source.insert_str(offset, &block);
  }
  Ok(source)
}

fn match_covers_complete_lines(
  source: &str,
  start_line: usize,
  end_line: usize,
  text: &str,
) -> bool {
  let lines: Vec<&str> = source.lines().collect();
  if start_line > end_line || end_line >= lines.len() {
    return false;
  }
  lines[start_line..=end_line].join("\n").trim() == text.trim()
}

fn render_bash_watch_block(indent: &str, watch: &TraceWatchOptions) -> Result<String> {
  let mut out = String::new();
  for var in &watch.watch {
    render_bash_watch_var(&mut out, indent, var, watch)?;
  }
  Ok(out)
}

fn render_bash_watch_var(
  out: &mut String,
  indent: &str,
  var: &str,
  watch: &TraceWatchOptions,
) -> Result<()> {
  let var_json = serde_json::to_string(var)
    .map_err(|e| lmprobe_types::error::anyhow!("serialize trace watch var: {}", e))?;
  let inner = format!("{indent}  ");
  push_indented_line(
    out,
    indent,
    &format!("__lmprobe_trace_decl=$(declare -p {var} 2>/dev/null || true)"),
  );
  push_indented_line(out, indent, "if [[ -z \"$__lmprobe_trace_decl\" ]]; then");
  push_indented_line(out, &inner, "__lmprobe_trace_type=\"unset\"");
  push_indented_line(out, &inner, "__lmprobe_trace_value=\"\"");
  push_indented_line(out, &inner, "__lmprobe_trace_len_json=\"null\"");
  push_indented_line(out, indent, "else");
  push_indented_line(out, &inner, "case \"$__lmprobe_trace_decl\" in");
  push_indented_line(out, &inner, "  declare\\ -a*|declare\\ -A*)");
  push_indented_line(out, &inner, "    __lmprobe_trace_type=\"array\"");
  push_indented_line(
    out,
    &inner,
    "    __lmprobe_trace_value=\"$__lmprobe_trace_decl\"",
  );
  push_indented_line(
    out,
    &inner,
    &format!("    __lmprobe_trace_len_json=\"${{#{var}[@]}}\""),
  );
  push_indented_line(out, &inner, "    ;;");
  push_indented_line(out, &inner, "  *)");
  push_indented_line(out, &inner, "    __lmprobe_trace_type=\"str\"");
  push_indented_line(
    out,
    &inner,
    &format!("    __lmprobe_trace_value=\"${{{var}}}\""),
  );
  push_indented_line(
    out,
    &inner,
    &format!("    __lmprobe_trace_len_json=\"${{#{var}}}\""),
  );
  push_indented_line(out, &inner, "    ;;");
  push_indented_line(out, &inner, "esac");
  push_indented_line(out, indent, "fi");
  push_indented_line(
    out,
    indent,
    &format!(
      "__lmprobe_trace_value=\"${{__lmprobe_trace_value:0:{}}}\"",
      watch.repr_limit
    ),
  );
  push_indented_line(
    out,
    indent,
    r#"__lmprobe_trace_repr=${__lmprobe_trace_value//\\/\\\\}"#,
  );
  push_indented_line(
    out,
    indent,
    r#"__lmprobe_trace_repr=${__lmprobe_trace_repr//\"/\\\"}"#,
  );
  push_indented_line(
    out,
    indent,
    r#"__lmprobe_trace_repr=${__lmprobe_trace_repr//$'\n'/\\n}"#,
  );
  push_indented_line(
    out,
    indent,
    r#"__lmprobe_trace_repr=${__lmprobe_trace_repr//$'\r'/\\r}"#,
  );
  push_indented_line(
    out,
    indent,
    r#"__lmprobe_trace_repr=${__lmprobe_trace_repr//$'\t'/\\t}"#,
  );
  let mut printf_line =
    format!("printf '[TRACE_JSON] {{\"var\":{var_json},\"type\":\"%s\",\"repr\":\"%s\"");
  if watch.include_len {
    printf_line.push_str(",\"len\":%s");
  }
  printf_line.push_str("}\\n' \"$__lmprobe_trace_type\" \"$__lmprobe_trace_repr\"");
  if watch.include_len {
    printf_line.push_str(" \"$__lmprobe_trace_len_json\"");
  }
  push_indented_line(out, indent, &printf_line);
  push_indented_line(
    out,
    indent,
    "unset __lmprobe_trace_decl __lmprobe_trace_type __lmprobe_trace_value __lmprobe_trace_repr __lmprobe_trace_len_json",
  );
  Ok(())
}

fn push_indented_line(out: &mut String, indent: &str, line: &str) {
  out.push_str(indent);
  out.push_str(line);
  out.push('\n');
}

fn is_r_watch_path(path: &Path) -> bool {
  path
    .extension()
    .and_then(|ext| ext.to_str())
    .is_some_and(|ext| ext.eq_ignore_ascii_case("r"))
}

fn r_line_query_matches(content: &str, query: &str) -> bool {
  let query = query.trim();
  !query.is_empty()
    && content
      .lines()
      .enumerate()
      .any(|(line_idx, line)| r_line_is_safe_query_match(content, line_idx, line, query))
}

fn instrument_r_watch_content(
  content: &str,
  query: &str,
  watch: &TraceWatchOptions,
) -> Result<String> {
  let query = query.trim();
  let mut insertions = Vec::new();
  for (line_idx, line) in content.lines().enumerate() {
    if !r_line_is_safe_query_match(content, line_idx, line, query) {
      continue;
    }
    let offset = insertion_offset_after_line(content, line_idx);
    let indent = line_indent(content, line_idx);
    let mut block = render_r_watch_block(&indent, watch)?;
    if offset == content.len() && !content.ends_with('\n') {
      block.insert(0, '\n');
    }
    insertions.push((offset, block));
  }

  if insertions.is_empty() {
    return Ok(content.to_string());
  }

  insertions.sort_by(|a, b| b.0.cmp(&a.0));
  let mut source = content.to_string();
  for (offset, block) in insertions {
    source.insert_str(offset, &block);
  }
  Ok(source)
}

fn r_line_is_safe_query_match(content: &str, line_idx: usize, line: &str, query: &str) -> bool {
  if line.trim() != query {
    return false;
  }
  let code = r_code_before_comment(line);
  let trimmed_code = code.trim();
  if trimmed_code.is_empty()
    || trimmed_code.ends_with(',')
    || r_line_ends_with_open_delimiter(trimmed_code)
    || r_line_ends_with_infix_operator(trimmed_code)
  {
    return false;
  }
  !r_has_unclosed_context_before_line(content, line_idx)
    && !r_has_unclosed_context_through_line(content, line_idx)
}

fn r_line_ends_with_open_delimiter(line: &str) -> bool {
  line.ends_with('(') || line.ends_with('[') || line.ends_with('{')
}

fn r_line_ends_with_infix_operator(line: &str) -> bool {
  if line.ends_with('%') && line[..line.len() - 1].rfind('%').is_some() {
    return true;
  }
  const MULTI: &[&str] = &[
    ":::", "<<-", "->>", "<-", "->", "<=", ">=", "==", "!=", "&&", "||", "::", ":=", "|>",
  ];
  if MULTI.iter().any(|op| line.ends_with(op)) {
    return true;
  }
  line.chars().next_back().is_some_and(|c| {
    matches!(
      c,
      '+' | '-' | '*' | '/' | '^' | '&' | '|' | '<' | '>' | '=' | '~' | ':' | '!' | '?'
    )
  })
}

fn r_has_unclosed_context_before_line(content: &str, line_idx: usize) -> bool {
  r_delimiter_state_for_lines(content.lines().take(line_idx)).has_unclosed_continuation()
}

fn r_has_unclosed_context_through_line(content: &str, line_idx: usize) -> bool {
  r_delimiter_state_for_lines(content.lines().take(line_idx + 1)).has_unclosed_continuation()
}

fn r_delimiter_state_for_lines<'a>(lines: impl Iterator<Item = &'a str>) -> RDelimiterState {
  let mut state = RDelimiterState::default();
  for line in lines {
    state.scan_line(line);
  }
  state
}

#[derive(Default)]
struct RDelimiterState {
  parens: usize,
  brackets: usize,
  braces: usize,
  in_single: bool,
  in_double: bool,
  escaped: bool,
}

impl RDelimiterState {
  fn has_unclosed_continuation(&self) -> bool {
    self.parens > 0 || self.brackets > 0 || self.in_single || self.in_double
  }

  fn scan_line(&mut self, line: &str) {
    for c in line.chars() {
      if self.scan_string_char(c) {
        continue;
      }
      match c {
        '#' => break,
        '\'' => self.in_single = true,
        '"' => self.in_double = true,
        '(' => self.parens += 1,
        '[' => self.brackets += 1,
        '{' => self.braces += 1,
        ')' => self.parens = self.parens.saturating_sub(1),
        ']' => self.brackets = self.brackets.saturating_sub(1),
        '}' => self.braces = self.braces.saturating_sub(1),
        _ => {}
      }
    }
    self.escaped = false;
  }

  fn scan_string_char(&mut self, c: char) -> bool {
    if !self.in_single && !self.in_double {
      return false;
    }
    if self.escaped {
      self.escaped = false;
      return true;
    }
    if c == '\\' {
      self.escaped = true;
      return true;
    }
    if self.in_single && c == '\'' {
      self.in_single = false;
    } else if self.in_double && c == '"' {
      self.in_double = false;
    }
    true
  }
}

fn r_code_before_comment(line: &str) -> String {
  let mut out = String::new();
  let mut in_single = false;
  let mut in_double = false;
  let mut escaped = false;
  for c in line.chars() {
    if in_single || in_double {
      out.push(c);
      if escaped {
        escaped = false;
      } else if c == '\\' {
        escaped = true;
      } else if in_single && c == '\'' {
        in_single = false;
      } else if in_double && c == '"' {
        in_double = false;
      }
      continue;
    }
    match c {
      '#' => break,
      '\'' => {
        in_single = true;
        out.push(c);
      }
      '"' => {
        in_double = true;
        out.push(c);
      }
      _ => out.push(c),
    }
  }
  out
}

fn render_r_watch_block(indent: &str, watch: &TraceWatchOptions) -> Result<String> {
  let mut out = String::new();
  let inner = format!("{indent}  ");
  let body = format!("{indent}    ");
  let nested = format!("{indent}      ");
  push_indented_line(&mut out, indent, "local({");
  push_indented_line(
    &mut out,
    &inner,
    ".__lmprobe_env <- parent.env(environment())",
  );
  push_indented_line(
    &mut out,
    &inner,
    r#".__lmprobe_json_string <- function(x) encodeString(paste(as.character(x), collapse = "\n"), quote = "\"")"#,
  );
  push_indented_line(
    &mut out,
    &inner,
    r#".__lmprobe_shape_json <- function(x) { d <- dim(x); if (is.null(d)) "null" else paste0("[", paste(as.integer(d), collapse = ","), "]") }"#,
  );
  push_indented_line(&mut out, &inner, ".__lmprobe_emit <- function(name) {");
  push_indented_line(&mut out, &body, "tryCatch({");
  push_indented_line(
    &mut out,
    &nested,
    "if (exists(name, envir = .__lmprobe_env, inherits = FALSE)) {",
  );
  push_indented_line(
    &mut out,
    &format!("{nested}  "),
    "value <- get(name, envir = .__lmprobe_env, inherits = FALSE)",
  );
  push_indented_line(
    &mut out,
    &format!("{nested}  "),
    "type <- paste(class(value), collapse = \"/\")",
  );
  push_indented_line(
    &mut out,
    &format!("{nested}  "),
    "repr <- paste(capture.output(print(value)), collapse = \"\\n\")",
  );
  push_indented_line(
    &mut out,
    &format!("{nested}  "),
    &format!("repr <- substr(repr, 1, {})", watch.repr_limit),
  );
  push_indented_line(
    &mut out,
    &format!("{nested}  "),
    r#"fields <- c(paste0("\"var\":", .__lmprobe_json_string(name)), paste0("\"type\":", .__lmprobe_json_string(type)), paste0("\"repr\":", .__lmprobe_json_string(repr)))"#,
  );
  if watch.include_len {
    push_indented_line(
      &mut out,
      &format!("{nested}  "),
      r#"fields <- c(fields, paste0("\"len\":", length(value)))"#,
    );
  }
  if watch.include_shape {
    push_indented_line(
      &mut out,
      &format!("{nested}  "),
      r#"fields <- c(fields, paste0("\"shape\":", .__lmprobe_shape_json(value)))"#,
    );
  }
  if watch.include_id {
    push_indented_line(
      &mut out,
      &format!("{nested}  "),
      r#"fields <- c(fields, "\"id\":null")"#,
    );
  }
  push_indented_line(&mut out, &nested, "} else {");
  push_indented_line(
    &mut out,
    &format!("{nested}  "),
    r#"fields <- c(paste0("\"var\":", .__lmprobe_json_string(name)), "\"type\":\"unset\"", "\"repr\":\"\"")"#,
  );
  if watch.include_len {
    push_indented_line(
      &mut out,
      &format!("{nested}  "),
      r#"fields <- c(fields, "\"len\":null")"#,
    );
  }
  if watch.include_shape {
    push_indented_line(
      &mut out,
      &format!("{nested}  "),
      r#"fields <- c(fields, "\"shape\":null")"#,
    );
  }
  if watch.include_id {
    push_indented_line(
      &mut out,
      &format!("{nested}  "),
      r#"fields <- c(fields, "\"id\":null")"#,
    );
  }
  push_indented_line(&mut out, &nested, "}");
  push_indented_line(
    &mut out,
    &nested,
    "cat(\"[TRACE_JSON] {\", paste(fields, collapse = \",\"), \"}\\n\", sep = \"\")",
  );
  push_indented_line(&mut out, &body, "}, error = function(e) {");
  push_indented_line(
    &mut out,
    &nested,
    &format!(
      r#"repr <- substr(conditionMessage(e), 1, {})"#,
      watch.repr_limit
    ),
  );
  push_indented_line(
    &mut out,
    &nested,
    r#"fields <- c(paste0("\"var\":", .__lmprobe_json_string(name)), "\"type\":\"error\"", paste0("\"repr\":", .__lmprobe_json_string(repr)))"#,
  );
  if watch.include_len {
    push_indented_line(&mut out, &nested, r#"fields <- c(fields, "\"len\":null")"#);
  }
  if watch.include_shape {
    push_indented_line(
      &mut out,
      &nested,
      r#"fields <- c(fields, "\"shape\":null")"#,
    );
  }
  if watch.include_id {
    push_indented_line(&mut out, &nested, r#"fields <- c(fields, "\"id\":null")"#);
  }
  push_indented_line(
    &mut out,
    &nested,
    "cat(\"[TRACE_JSON] {\", paste(fields, collapse = \",\"), \"}\\n\", sep = \"\")",
  );
  push_indented_line(&mut out, &body, "})");
  push_indented_line(&mut out, &inner, "}");
  for var in &watch.watch {
    let var_json = serde_json::to_string(var)
      .map_err(|e| lmprobe_types::error::anyhow!("serialize trace watch var: {}", e))?;
    push_indented_line(&mut out, &inner, &format!(".__lmprobe_emit({var_json})"));
  }
  push_indented_line(&mut out, indent, "})");
  Ok(out)
}

// ──────────────────────── trace worktree (4B) ─────────────────────

/// Bookkeeping for a trace-provisioned worktree so we can clean up.
struct TraceWorktree {
  /// Filesystem path of the worktree (the new clean checkout).
  path: PathBuf,
  /// Path of the *parent* repository — `git worktree remove` runs
  /// from there.
  parent: PathBuf,
}

fn provision_trace_worktree(
  root: &Path,
  explicit: Option<&Path>,
) -> std::result::Result<TraceWorktree, Diagnostic> {
  let parent = match Repository::discover(root) {
    Ok(repo) => repo
      .workdir()
      .map(|p| p.to_path_buf())
      .unwrap_or_else(|| root.to_path_buf()),
    Err(_) => {
      return Err(Diagnostic::new(
        "NOT_GIT_REPO",
        "trace --worktree requires the source to be inside a git repo",
      ));
    }
  };
  let target = match explicit {
    Some(p) => p.to_path_buf(),
    None => {
      let ts = chrono::Local::now().format("%Y%m%dT%H%M%S").to_string();
      crate::history::xdg_cache_dir()
        .join("worktrees")
        .join(format!("trace-{}", ts))
    }
  };
  if let Some(parent_dir) = target.parent() {
    if let Err(e) = std::fs::create_dir_all(parent_dir) {
      return Err(Diagnostic::new(
        "WORKTREE_PROVISION_FAILED",
        format!("could not create worktree parent dir: {}", e),
      ));
    }
  }
  let out = Command::new("git")
    .args(["worktree", "add", "--detach", target.to_str().unwrap_or("")])
    .current_dir(&parent)
    .output()
    .map_err(|e| {
      Diagnostic::new(
        "WORKTREE_PROVISION_FAILED",
        format!("git worktree add failed to spawn: {}", e),
      )
    })?;
  if !out.status.success() {
    return Err(Diagnostic::new(
      "WORKTREE_PROVISION_FAILED",
      format!(
        "git worktree add returned non-zero: {}",
        String::from_utf8_lossy(&out.stderr).trim()
      ),
    ));
  }
  Ok(TraceWorktree {
    path: target,
    parent,
  })
}

fn remove_trace_worktree(wt: &TraceWorktree) -> std::result::Result<(), Diagnostic> {
  let out = Command::new("git")
    .args([
      "worktree",
      "remove",
      "--force",
      wt.path.to_str().unwrap_or(""),
    ])
    .current_dir(&wt.parent)
    .output()
    .map_err(|e| {
      Diagnostic::new(
        "WORKTREE_CLEANUP_FAILED",
        format!("git worktree remove spawn: {}", e),
      )
    })?;
  if !out.status.success() {
    // Fall back to a manual removal so the cache dir doesn't
    // grow unbounded if libgit2 leaves dangling state.
    let _ = std::fs::remove_dir_all(&wt.path);
  }
  Ok(())
}

/// Capture a unified diff of every instrumented file's mid-trace
/// state. Runs BEFORE backup-restore so the diff reflects what the
/// trace actually changed.
fn capture_patch(
  root: &Path,
  files: &[PathBuf],
  explicit: Option<&Path>,
) -> std::result::Result<PathBuf, Diagnostic> {
  let target = match explicit {
    Some(p) => p.to_path_buf(),
    None => {
      let ts = chrono::Local::now().format("%Y%m%dT%H%M%S").to_string();
      crate::history::xdg_cache_dir()
        .join("patches")
        .join(format!("trace-{}.patch", ts))
    }
  };
  if let Some(parent_dir) = target.parent() {
    if let Err(e) = std::fs::create_dir_all(parent_dir) {
      return Err(Diagnostic::new(
        "PATCH_CAPTURE_FAILED",
        format!("could not create patch parent dir: {}", e),
      ));
    }
  }
  let mut args: Vec<String> = vec!["diff".into(), "--no-color".into(), "--".into()];
  for f in files {
    let rel = f
      .strip_prefix(root)
      .unwrap_or(f.as_path())
      .to_string_lossy()
      .into_owned();
    args.push(rel);
  }
  let out = Command::new("git")
    .args(&args)
    .current_dir(root)
    .output()
    .map_err(|e| Diagnostic::new("PATCH_CAPTURE_FAILED", format!("git diff spawn: {}", e)))?;
  if let Err(e) = std::fs::write(&target, &out.stdout) {
    return Err(Diagnostic::new(
      "PATCH_CAPTURE_FAILED",
      format!("write patch: {}", e),
    ));
  }
  Ok(target)
}

// ───────────── Phase 4E §6.4: persistent-marker trace ─────────────
//
// `trace_persistent` mutates the user's working tree and leaves the
// instrumentation in place. D7 dirty-tree refusal is enforced —
// there's no worktree to escape into. An audit log is written to
// `$XDG_CACHE_HOME/lmprobe/trace-<marker>.audit.json` BEFORE any
// mutation, recording per-insertion `file_path` / `line_number` /
// `inserted_text` so `untrace` can revert deterministically.

/// One audit-log entry per inserted line.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct TraceAuditInsertion {
  pub file_path: String,
  pub line_number: u32,
  pub inserted_text: String,
}

/// JSON shape of `$XDG_CACHE_HOME/lmprobe/trace-<marker>.audit.json`.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct TraceAuditLog {
  pub schema: String,
  pub marker: String,
  pub created_at: String,
  pub scope_summary: String,
  pub insertions: Vec<TraceAuditInsertion>,
}

/// XDG path for a marker's audit log.
pub fn persistent_audit_path(marker: &str) -> PathBuf {
  crate::history::xdg_cache_dir().join(format!("trace-{}.audit.json", marker))
}

/// Render a template with the `$marker` placeholder replaced and
/// the literal `[TRACE-<marker>]` token guaranteed in the output.
/// Returns the rendered text. `Err` if the user-supplied template
/// is empty.
fn render_persistent_template(
  template: &str,
  marker: &str,
) -> std::result::Result<String, Diagnostic> {
  if template.trim().is_empty() {
    return Err(Diagnostic::new(
      "INVALID_TRACE_MODE",
      "persistent trace requires a non-empty template",
    ));
  }
  let mut rendered = template.replace("$marker", marker);
  let token = format!("[TRACE-{}]", marker);
  if !rendered.contains(&token) {
    // Append the marker token as a trailing comment if the user
    // didn't include it. This keeps `untrace`'s regex fallback
    // viable when the audit log is missing.
    rendered.push_str(&format!(" // {}", token));
  }
  Ok(rendered)
}

/// `lmprobe query 'mutation { trace(... worktree: false, exec: null,
/// marker: "...") }'` — persistent-marker mode. Walks `query`
/// matches in `root`, inserts the rendered template ABOVE each
/// match, writes an audit log, returns a `TraceRunResult` with one
/// `TraceEvent` per insertion (events repurposed as a write
/// receipt for this mode — `output` is the inserted text).
pub fn trace_persistent(
  query: &str,
  root: &Path,
  template: &str,
  marker: &str,
  force: bool,
  opts: &WalkOpts,
) -> Result<TraceRunResult> {
  if let Err(diag) = check_trace_safety(root, force) {
    return Ok(TraceRunResult::refusal(diag));
  }

  // Discovery — same shape as transient mode.
  let mut files_to_instrument = Vec::new();
  if root.is_file() {
    if let Some(lang) = SgLang::from_path(root) {
      if let Ok(content) = fs::read_to_string(root) {
        let grep = lang.ast_grep(content);
        if grep.root().find_all(query).next().is_some() {
          files_to_instrument.push(root.to_path_buf());
        }
      }
    }
  } else {
    let mut wb = WalkBuilder::new(root);
    apply_walk_opts(&mut wb, opts, root);
    let walker = wb.build();
    for entry in walker.flatten() {
      let path = entry.path();
      if !path.is_file() {
        continue;
      }
      if let Some(lang) = SgLang::from_path(path) {
        if let Ok(content) = fs::read_to_string(path) {
          let grep = lang.ast_grep(content);
          if grep.root().find_all(query).next().is_some() {
            files_to_instrument.push(path.to_path_buf());
          }
        }
      }
    }
  }
  if files_to_instrument.is_empty() {
    return Ok(TraceRunResult::new(vec![]));
  }

  let rendered = match render_persistent_template(template, marker) {
    Ok(s) => s,
    Err(diag) => return Ok(TraceRunResult::refusal(diag)),
  };

  // Plan insertions: per file, find each match's start line and
  // determine post-instrumentation line numbers. Process in
  // ascending order with a running offset so the audit log
  // records the inserted-line position in the final file (the
  // line the user — and `untrace` — will see).
  let mut insertions: Vec<TraceAuditInsertion> = Vec::new();
  let mut file_plans: Vec<(PathBuf, Vec<usize>, Vec<String>)> = Vec::new();
  for path in &files_to_instrument {
    let lang = match SgLang::from_path(path) {
      Some(l) => l,
      None => continue,
    };
    let content = fs::read_to_string(path)?;
    let grep = lang.ast_grep(content.clone());
    let mut lines: Vec<usize> = grep
      .root()
      .find_all(query)
      .map(|m| m.start_pos().line()) // 0-based
      .collect();
    lines.sort_unstable();
    lines.dedup();
    if lines.is_empty() {
      continue;
    }
    // Compute the indentation of each target line so each
    // inserted line sits flush with its match.
    let text_lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut indents: Vec<String> = Vec::with_capacity(lines.len());
    for &line0 in &lines {
      let indent: String = text_lines
        .get(line0)
        .map(|l| l.chars().take_while(|c| c.is_whitespace()).collect())
        .unwrap_or_default();
      indents.push(indent);
    }
    // Post-instrumentation line numbers: each insert at
    // ascending original line L adds 1 to the offset for
    // later targets, so the inserted text ends up at line
    // (L + offset_so_far + 1) (1-based) in the final file.
    for (i, &line0) in lines.iter().enumerate() {
      let final_line = (line0 + i + 1) as u32; // i = number of prior inserts
      let inserted_text = format!("{}{}", indents[i], rendered);
      insertions.push(TraceAuditInsertion {
        file_path: path.to_string_lossy().into_owned(),
        line_number: final_line,
        inserted_text,
      });
    }
    file_plans.push((path.clone(), lines, indents));
  }

  if insertions.is_empty() {
    return Ok(TraceRunResult::new(vec![]));
  }

  // Audit log written BEFORE mutation so a hard crash leaves a
  // recovery breadcrumb. Per arbitration F shape.
  let audit = TraceAuditLog {
    schema: "lmprobe.trace_audit.v1".to_string(),
    marker: marker.to_string(),
    created_at: chrono::Local::now().to_rfc3339(),
    scope_summary: format!("pattern={:?} root={}", query, root.display()),
    insertions: insertions.clone(),
  };
  let audit_path = persistent_audit_path(marker);
  fs::create_dir_all(audit_path.parent().unwrap_or(Path::new(".")))?;
  let audit_json = serde_json::to_string_pretty(&audit)
    .map_err(|e| lmprobe_types::error::anyhow!("serialize audit log: {}", e))?;
  fs::write(&audit_path, audit_json.as_bytes())?;

  // Apply: ascending order with running offset so each insertion
  // lands at its planned post-instrumentation index. Indents
  // were captured pre-mutation against the same lines vector.
  let mut events: Vec<TraceEvent> = Vec::new();
  for (path, lines, indents) in file_plans {
    let content = fs::read_to_string(&path)?;
    let mut text_lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let trailing_newline = content.ends_with('\n');
    for (i, &line0) in lines.iter().enumerate() {
      let insert_idx = line0 + i; // running offset
      let inserted = format!("{}{}", indents[i], rendered);
      if insert_idx <= text_lines.len() {
        text_lines.insert(insert_idx, inserted.clone());
      } else {
        text_lines.push(inserted.clone());
      }
      events.push(trace_event(format!(
        "inserted at {}:{} -- {}",
        path.display(),
        insert_idx + 1,
        rendered
      )));
    }
    let mut joined = text_lines.join("\n");
    if trailing_newline {
      joined.push('\n');
    }
    fs::write(&path, joined.as_bytes())?;
  }

  let mut res = TraceRunResult::new(events);
  res.push_warning(
    Diagnostic::new(
      "TRACE_AUDIT_LOG",
      format!("audit log written to {}", audit_path.display()),
    )
    .with_hint("call `untrace` with the same marker to revert"),
  );
  Ok(res)
}

// ─────────────────── Phase 4E §6.4: untrace ────────────────────────
//
// Revert insertions by deleting matching lines. Two paths:
// - `audit_log` (preferred): read `trace-<marker>.audit.json`,
//   delete each recorded insertion exactly.
// - `regex_fallback`: scan files, delete any line containing the
//   literal `[TRACE-<marker>]` token (per arbitration F).

/// Walk `root` and delete every line containing the literal
/// `[TRACE-<marker>]` token. Returns the per-site detail of each
/// deletion. `marker` is regex-escaped before matching (defensive
/// even though our marker validator restricts the alphabet).
/// Read-only output-collision preflight matching the fallback's exact file scope.
pub fn untrace_fallback_targets(root: &Path, marker: &str, opts: &WalkOpts) -> Vec<PathBuf> {
  let token = format!("[TRACE-{}]", marker);
  let mut builder = WalkBuilder::new(root);
  apply_walk_opts(&mut builder, opts, root);
  builder
    .build()
    .flatten()
    .filter_map(|entry| {
      let path = entry.path();
      if path.is_file() && fs::read_to_string(path).is_ok_and(|content| content.contains(&token)) {
        Some(path.to_path_buf())
      } else {
        None
      }
    })
    .collect()
}

pub fn untrace_regex_fallback(
  root: &Path,
  marker: &str,
  opts: &WalkOpts,
) -> Result<Vec<lmprobe_types::schema::RevertedSite>> {
  let token = format!("[TRACE-{}]", marker);
  let mut sites: Vec<lmprobe_types::schema::RevertedSite> = Vec::new();
  let walk = {
    let mut wb = WalkBuilder::new(root);
    apply_walk_opts(&mut wb, opts, root);
    wb.build()
  };
  for entry in walk.flatten() {
    let path = entry.path();
    if !path.is_file() {
      continue;
    }
    let content = match fs::read_to_string(path) {
      Ok(c) => c,
      Err(_) => continue, // binary / unreadable — skip
    };
    if !content.contains(&token) {
      continue;
    }
    let trailing_newline = content.ends_with('\n');
    let mut kept: Vec<String> = Vec::new();
    for (idx, line) in content.lines().enumerate() {
      if line.contains(&token) {
        sites.push(lmprobe_types::schema::RevertedSite {
          file_path: path.to_string_lossy().into_owned(),
          line_number: (idx + 1) as u32,
          reverted_text: line.to_string(),
        });
      } else {
        kept.push(line.to_string());
      }
    }
    let mut joined = kept.join("\n");
    if trailing_newline {
      joined.push('\n');
    }
    fs::write(path, joined.as_bytes())?;
  }
  Ok(sites)
}

/// Read `$XDG_CACHE_HOME/lmprobe/trace-<marker>.audit.json` and
/// revert each recorded insertion. Returns the per-site detail of
/// every deletion. Returns `Err` (signaling the resolver should
/// fall back to regex mode) when the audit log is missing or
/// corrupt.
pub fn untrace_from_audit_log(
  marker: &str,
) -> std::result::Result<Vec<lmprobe_types::schema::RevertedSite>, std::io::Error> {
  let audit_path = persistent_audit_path(marker);
  let audit_json = fs::read_to_string(&audit_path)?;
  let audit: TraceAuditLog = serde_json::from_str(&audit_json).map_err(|e| {
    std::io::Error::new(
      std::io::ErrorKind::InvalidData,
      format!("parse audit: {}", e),
    )
  })?;

  // Group insertions by file. Process each file's insertions in
  // descending line order so earlier deletions don't shift later
  // target indices.
  let mut by_file: HashMap<String, Vec<TraceAuditInsertion>> = HashMap::new();
  for ins in audit.insertions {
    by_file.entry(ins.file_path.clone()).or_default().push(ins);
  }

  let mut sites: Vec<lmprobe_types::schema::RevertedSite> = Vec::new();
  for (path_str, mut entries) in by_file {
    entries.sort_unstable_by(|a, b| b.line_number.cmp(&a.line_number));
    let path = PathBuf::from(&path_str);
    let content = fs::read_to_string(&path)?;
    let trailing_newline = content.ends_with('\n');
    let mut text_lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    for ins in entries {
      let line_idx = (ins.line_number as usize).saturating_sub(1);
      if line_idx >= text_lines.len() {
        continue;
      }
      // Match by substring rather than full equality so
      // indentation prefix doesn't break the revert.
      let inserted_trim = ins.inserted_text.trim();
      if !text_lines[line_idx].contains(inserted_trim) {
        // Line content drifted — skip rather than corrupt.
        continue;
      }
      let removed = text_lines.remove(line_idx);
      sites.push(lmprobe_types::schema::RevertedSite {
        file_path: path_str.clone(),
        line_number: ins.line_number,
        reverted_text: removed,
      });
    }
    let mut joined = text_lines.join("\n");
    if trailing_newline {
      joined.push('\n');
    }
    fs::write(&path, joined.as_bytes())?;
  }
  Ok(sites)
}
