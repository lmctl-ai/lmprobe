//! Render layer: schema → stdout / stderr.
//!
//! D4 dictates that markdown / JSON output is a pure function of the
//! sensor result. This module owns those functions; sensors return
//! shapes, the CLI dispatcher picks a renderer.
//!
//! ### Broken-pipe handling (QA §2.14, fixed in §0.4)
//! `println!` panics on a `BrokenPipe` write — that's how the
//! pre-Phase-0 binary exited 101 when piped through `head`. We
//! replace every output write with [`stdout_line`] / [`stdout_str`],
//! which silently exits the process with status 0 when the
//! downstream reader closes its end. Other write errors propagate
//! to exit 2 via stderr.

use std::io::{self, Write};
use std::process;

use clap::ValueEnum;

use crate::schema::{
  Diagnostic, DiscoverResult, EvolutionResult, FileDeleteResult, FileEntry, FindResult,
  GrepResultEnvelope, PrTriageResult, RefResult, SymbolHit, TraceRunResult,
};

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
  /// Pick at runtime: TTY → markdown, pipe → ndjson.
  Auto,
  Markdown,
  Json,
  /// Newline-delimited JSON: one item per line + a final summary.
  Ndjson,
  /// SARIF 2.1.0 — GitHub Advanced Security can ingest this directly.
  Sarif,
  /// GitHub Actions annotations.
  Github,
  /// Mermaid graph (graph commands only).
  Mermaid,
  /// Graphviz DOT graph (graph commands only).
  Dot,
}

impl OutputFormat {
  /// Resolve `Auto` → Markdown / Ndjson based on stdout tty status.
  pub fn resolve(self) -> OutputFormat {
    use std::io::IsTerminal;
    if matches!(self, OutputFormat::Auto) {
      if std::io::stdout().is_terminal() {
        OutputFormat::Markdown
      } else {
        OutputFormat::Ndjson
      }
    } else {
      self
    }
  }
}

/// Write a single line + newline to stdout. On `BrokenPipe` the
/// process exits cleanly with status 0 (closes QA §2.14).
pub fn stdout_line(s: &str) {
  let stdout = io::stdout();
  let mut h = stdout.lock();
  if let Err(e) = h.write_all(s.as_bytes()).and_then(|_| h.write_all(b"\n")) {
    handle_stdout_err(e);
  }
}

fn handle_stdout_err(e: io::Error) -> ! {
  if e.kind() == io::ErrorKind::BrokenPipe {
    process::exit(0);
  }
  eprintln!("lmprobe: stdout write error: {}", e);
  process::exit(2);
}

/// Render diagnostics to stderr in a stable, parseable form. The CLI
/// dispatcher decides when to call this; explicit JSON output keeps
/// non-error warnings envelope-only unless `-v/--verbose` is set.
///
/// Phase 6A #4: incidental workspace-walk codes (NOT_IMPLEMENTED,
/// PATTERN_PARSE_FAILED) are suppressed unless `verbose=true` to
/// reduce noise on multi-language workspaces. User-facing diagnostics
/// mirror for human-oriented formats; JSON mode keeps non-error
/// diagnostics in the envelope by default.
pub fn render_diagnostics_stderr(verb: &str, diags: &[Diagnostic], verbose: bool) {
  for d in diags {
    if !verbose && is_incidental_walk_code(&d.code) {
      continue;
    }
    let lang = d
      .lang
      .as_deref()
      .map(|l| format!(" language={}", l))
      .unwrap_or_default();
    eprintln!("lmprobe: {} [{}]: {}{}", verb, d.code, d.message, lang);
    if let Some(hint) = &d.hint {
      eprintln!("lmprobe:   hint: {}", hint);
    }
  }
}

/// Codes treated as "incidental workspace-walk noise" — emitted when
/// the walker encountered a file in a language for which the verb's
/// grammar isn't wired (NOT_IMPLEMENTED) or the pattern failed to
/// parse (PATTERN_PARSE_FAILED). They live in the envelope's
/// `warnings[]` for programmatic consumers; suppressing the stderr
/// mirror by default keeps `lmprobe <verb> .` readable on big
/// workspaces. `-v/--verbose` re-enables.
fn is_incidental_walk_code(code: &str) -> bool {
  matches!(code, "NOT_IMPLEMENTED" | "PATTERN_PARSE_FAILED")
}

// ───────────────────────── markdown helpers ───────────────────────

/// Markdown-render a list of symbol hits. Used by `search`, `def`,
/// `callers`, `imports`, `decorations`.
pub fn render_symbol_hits_markdown(hits: &[SymbolHit]) {
  let mut current_file = String::new();
  for res in hits {
    if res.file_path != current_file {
      stdout_line("");
      stdout_line(&format!("#### File: {}", res.file_path));
      current_file = res.file_path.clone();
    }
    stdout_line(&format!("- **{}** ({})", res.name, res.kind));
    stdout_line(&format!("```{}", res.lang));
    stdout_line(res.content.trim());
    stdout_line("```");
    if !res.source_context.is_empty() {
      stdout_line("source context:");
      for line in &res.source_context {
        stdout_line(&format!("{:>6}: {}", line.line_number, line.line_content));
      }
    }
  }
}

/// `search`/`def`/`callers`/`imports`/`decorations` JSON output —
/// the envelope (with diagnostics + exit_status) is wrapped here.
pub fn render_envelope_json<E: serde::Serialize>(envelope: &E) {
  crate::program_output::emit(envelope);
}

pub fn render_discover_markdown(res: &DiscoverResult) {
  for p in &res.projects {
    stdout_line("---");
    stdout_line(&format!(
      "Name: {}\nType: {}\nVersion: {}\nPath: {}\nDeps: {}",
      p.name, p.project_type, p.version, p.path, p.dependencies_count
    ));
  }
}

pub fn render_pr_triage_markdown(res: &PrTriageResult) {
  stdout_line(&format!(
    "# pr-triage: {}/{}#{}",
    res.target.owner, res.target.repo, res.target.number
  ));
  if let Some(status) = &res.status {
    stdout_line(&format!(
      "state: {}{}",
      status.state.as_deref().unwrap_or("unknown"),
      if status.draft { " (draft)" } else { "" }
    ));
    if let Some(title) = &status.title {
      stdout_line(&format!("title: {}", title));
    }
    if let Some(author) = &status.author {
      stdout_line(&format!("author: {}", author));
    }
    if let Some(mergeable) = &status.mergeable {
      stdout_line(&format!("mergeable: {}", mergeable));
    }
    if let Some(review_decision) = &status.review_decision {
      stdout_line(&format!("review_decision: {}", review_decision));
    }
  } else {
    stdout_line("status: unavailable");
  }
  stdout_line(&format!(
    "checks: {} total, {} passing, {} failing, {} pending, {} skipped/neutral",
    res.check_summary.total,
    res.check_summary.passing,
    res.check_summary.failing,
    res.check_summary.pending,
    res.check_summary.skipped_neutral
  ));
  for check in &res.checks {
    stdout_line(&format!(
      "- check [{}] {}{}",
      check.bucket,
      check.name,
      check
        .conclusion
        .as_deref()
        .or(check.state.as_deref())
        .or(check.status.as_deref())
        .map(|state| format!(" ({state})"))
        .unwrap_or_default()
    ));
  }
  if !res.evidence.is_empty() {
    stdout_line(&format!("evidence since {}:", res.since));
    for event in &res.evidence {
      let terms = if event.action_terms.is_empty() {
        String::new()
      } else {
        format!(" terms={}", event.action_terms.join(","))
      };
      let location = match (&event.path, event.line) {
        (Some(path), Some(line)) => format!(" {}:{}", path, line),
        (Some(path), None) => format!(" {}", path),
        _ => String::new(),
      };
      stdout_line(&format!(
        "- {} {} by {}{}{}: {}",
        event.created_at, event.kind, event.author, location, terms, event.body_excerpt
      ));
    }
  }
}

/// §2.1 — `--tree` rendering. Walks the parent → children graph in
/// `DiscoverResult` and prints an indented tree. Top-level (root)
/// projects are those whose path doesn't appear as another project's
/// child.
pub fn render_discover_tree(res: &DiscoverResult) {
  use std::collections::{HashMap, HashSet};
  let by_path: HashMap<&str, &crate::schema::Project> =
    res.projects.iter().map(|p| (p.path.as_str(), p)).collect();
  // Mark every path that appears as someone's child; the rest are
  // tree roots.
  let mut child_paths: HashSet<&str> = HashSet::new();
  for p in &res.projects {
    for c in &p.children {
      child_paths.insert(c.as_str());
    }
  }
  let mut roots: Vec<&crate::schema::Project> = res
    .projects
    .iter()
    .filter(|p| !child_paths.contains(p.path.as_str()))
    .collect();
  roots.sort_by(|a, b| a.path.cmp(&b.path));
  for r in roots {
    render_discover_tree_node(r, &by_path, 0);
  }
}

fn render_discover_tree_node(
  p: &crate::schema::Project,
  by_path: &std::collections::HashMap<&str, &crate::schema::Project>,
  depth: usize,
) {
  let indent = "  ".repeat(depth);
  let prefix = if depth == 0 { "" } else { "└─ " };
  stdout_line(&format!(
    "{}{}{} ({}) {} — deps: {}, path: {}",
    indent, prefix, p.name, p.project_type, p.version, p.dependencies_count, p.path
  ));
  for child_path in &p.children {
    if let Some(child) = by_path.get(child_path.as_str()) {
      render_discover_tree_node(child, by_path, depth + 1);
    }
  }
}

/// §1.5 — `-A/--after`, `-B/--before`, `-C/--context` context lines.
/// Markdown renderers for `grep` / `ref` use this to surround each
/// hit with N lines from the source file. `grep` JSON/NDJSON also
/// exposes this as optional context fields on each emitted hit.
#[derive(Default, Clone, Copy, Debug)]
pub struct ContextLines {
  pub before: usize,
  pub after: usize,
}

impl ContextLines {
  pub fn is_off(&self) -> bool {
    self.before == 0 && self.after == 0
  }
}

fn render_line_hits_markdown<I: Iterator<Item = (String, usize, String)>>(
  iter: I,
  ctx: ContextLines,
) {
  use std::collections::HashMap;
  let mut file_cache: HashMap<String, Vec<String>> = HashMap::new();
  let mut last_separator_file: Option<String> = None;
  for (file_path, line_number, line_content) in iter {
    if ctx.is_off() {
      stdout_line(&format!("{}:{}: {}", file_path, line_number, line_content));
      continue;
    }
    let lines = file_cache.entry(file_path.clone()).or_insert_with(|| {
      std::fs::read_to_string(&file_path)
        .unwrap_or_default()
        .lines()
        .map(|s| s.to_string())
        .collect()
    });
    if last_separator_file.as_deref() == Some(file_path.as_str()) {
      stdout_line("--");
    }
    let target_idx = line_number.saturating_sub(1);
    let start = target_idx.saturating_sub(ctx.before);
    let end = (target_idx + ctx.after + 1).min(lines.len());
    for i in start..end {
      let sep = if i == target_idx { ":" } else { "-" };
      let line_text = lines.get(i).cloned().unwrap_or_default();
      stdout_line(&format!("{}{}{}: {}", file_path, sep, i + 1, line_text));
    }
    last_separator_file = Some(file_path);
  }
}

pub fn render_grep_hits_markdown(res: &GrepResultEnvelope, ctx: ContextLines) {
  render_line_hits_markdown(
    res
      .hits
      .iter()
      .map(|h| (h.file_path.clone(), h.line_number, h.line_content.clone())),
    ctx,
  );
}

pub fn render_ref_hits_markdown(res: &RefResult, ctx: ContextLines) {
  render_line_hits_markdown(
    res
      .hits
      .iter()
      .map(|h| (h.file_path.clone(), h.line_number, h.line_content.clone())),
    ctx,
  );
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum FileColumn {
  Name,
  Lastmod,
  Size,
  Type,
}

pub fn render_file(file: &FileEntry, columns: &[FileColumn]) {
  let fields: Vec<String> = columns
    .iter()
    .map(|column| match column {
      FileColumn::Name => {
        if file.path.chars().any(char::is_control) {
          serde_json::to_string(&file.path).unwrap()
        } else {
          file.path.clone()
        }
      }
      FileColumn::Lastmod => file
        .modified_at
        .map(|time| time.to_rfc3339())
        .unwrap_or_else(|| "?".into()),
      FileColumn::Size => {
        if file.file_type == "file" {
          file.size_bytes.to_string()
        } else {
          "-".into()
        }
      }
      FileColumn::Type => file.file_type.clone(),
    })
    .collect();
  stdout_line(&fields.join("\t"));
}

pub fn render_find_columns(res: &FindResult, columns: &[FileColumn]) {
  let files: std::collections::HashMap<_, _> = res
    .files
    .iter()
    .map(|file| (file.path.as_str(), file))
    .collect();
  for path in &res.paths {
    if let Some(file) = files.get(path.as_str()) {
      render_file(file, columns);
    } else {
      stdout_line(path);
    }
  }
}

pub fn render_delete_markdown(res: &FileDeleteResult, columns: &[FileColumn]) {
  for entry in &res.files {
    render_file(&entry.file, columns);
    if let Some(message) = &entry.message {
      eprintln!("lmprobe: {}: {} ({message})", entry.status, entry.file.path);
    }
  }
  stdout_line(&format!(
    "{}: {} selected, {} deleted",
    res.mode,
    res.files.len(),
    res.deleted_count
  ));
}

/// Dogfood finding (2026-09): `find`'s piped (ndjson/json) output
/// quotes each path as a JSON string (`"src/main.rs"`), which needs a
/// `sed 's/"//g'` before it's usable by anything shell-shaped.
/// `--paths-only` bypasses the envelope entirely and writes raw paths,
/// one per line (or NUL-separated with `--null`, for `xargs -0`/paths
/// containing newlines) — ignores `--format` on purpose, since the
/// whole point is "no JSON envelope at all" for this one field.
pub fn render_find_paths_only(res: &FindResult, null_sep: bool) {
  if !null_sep {
    for p in &res.paths {
      stdout_line(p);
    }
    return;
  }
  let stdout = io::stdout();
  let mut h = stdout.lock();
  for p in &res.paths {
    if let Err(e) = h.write_all(p.as_bytes()).and_then(|_| h.write_all(b"\0")) {
      handle_stdout_err(e);
    }
  }
}

pub fn render_evolution_markdown(res: &EvolutionResult) {
  stdout_line(&format!("### Evolution of `{}`", res.symbol));
  for m in &res.milestones {
    stdout_line(&format!(
      "- **{}**: {} ({}) [Commit: {}]",
      m.timestamp,
      m.author,
      m.change_type,
      &m.commit_hash[..8]
    ));
    stdout_line(&format!("  ```{}", m.change_type));
    stdout_line(&format!("  {}", m.signature.trim()));
    stdout_line("  ```");
  }
}

pub fn render_trace_markdown(res: &TraceRunResult) {
  stdout_line("--- Trace Results ---");
  for ev in &res.events {
    stdout_line(&format!("[{}] {}", ev.timestamp, ev.output));
  }
}

pub fn render_impact_markdown(res: &crate::schema::ImpactResult) {
  stdout_line(&format!(
    "### Impact of `{}` ({}, depth {})",
    res.origin, res.direction, res.max_depth
  ));
  for n in &res.nodes {
    stdout_line(&format!(
      "- d{} [{}] {} — {}:{}",
      n.depth, n.confidence, n.name, n.file_path, n.line_start
    ));
  }
  if !res.edges.is_empty() {
    stdout_line(&format!("\n_{} edges_", res.edges.len()));
  }
}

// ─────────────────── §4.4 Mermaid + DOT renderers ─────────────────

/// Sanitise a name for use as a Mermaid/DOT node id — alphanumeric +
/// underscore. Strips path separators, dots, etc. Falls back to a
/// hash if the result would be empty.
fn graph_id(s: &str) -> String {
  let cleaned: String = s
    .chars()
    .map(|c| {
      if c.is_alphanumeric() || c == '_' {
        c
      } else {
        '_'
      }
    })
    .collect();
  if cleaned.trim_matches('_').is_empty() {
    format!(
      "n{:x}",
      s.bytes().fold(0u32, |a, b| a.wrapping_add(b as u32))
    )
  } else {
    cleaned
  }
}

pub fn render_impact_mermaid(res: &crate::schema::ImpactResult) {
  stdout_line("```mermaid");
  stdout_line("graph TD");
  let origin_id = graph_id(&res.origin);
  stdout_line(&format!("  {}[\"{}\"]", origin_id, res.origin));
  for n in &res.nodes {
    let nid = graph_id(&n.name);
    stdout_line(&format!(
      "  {}[\"{}<br/>d{} {}\"]",
      nid, n.name, n.depth, n.confidence
    ));
  }
  for e in &res.edges {
    let from_id = graph_id(&e.from);
    let to_id = graph_id(&e.to);
    stdout_line(&format!("  {} -->|{}| {}", from_id, e.edge_type, to_id));
  }
  stdout_line("```");
}

pub fn render_impact_dot(res: &crate::schema::ImpactResult) {
  stdout_line("digraph impact {");
  stdout_line("  rankdir=LR;");
  let origin_id = graph_id(&res.origin);
  stdout_line(&format!(
    "  {} [label=\"{}\", shape=ellipse];",
    origin_id, res.origin
  ));
  for n in &res.nodes {
    let nid = graph_id(&n.name);
    stdout_line(&format!(
      "  {} [label=\"{}\\nd{} {}\", shape=box];",
      nid, n.name, n.depth, n.confidence
    ));
  }
  for e in &res.edges {
    let from_id = graph_id(&e.from);
    let to_id = graph_id(&e.to);
    stdout_line(&format!(
      "  {} -> {} [label=\"{}\"];",
      from_id, to_id, e.edge_type
    ));
  }
  stdout_line("}");
}

/// Symmetric renderer for the SymbolHit-shaped graph verbs
/// (callers / callees / supertypes / subtypes). Origin is the
/// queried symbol; each hit becomes an edge from origin to the
/// hit's caller/callee.
pub fn render_symbol_graph_mermaid(
  origin: &str,
  hits: &[SymbolHit],
  relation: &str,
  direction: &str,
) {
  stdout_line("```mermaid");
  stdout_line(&format!("graph {}", direction));
  let oid = graph_id(origin);
  stdout_line(&format!("  {}[\"{}\"]", oid, origin));
  let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
  for h in hits {
    let label = if h.name.is_empty() {
      "?".to_string()
    } else {
      h.name.clone()
    };
    let nid = graph_id(&label);
    if !seen.insert(nid.clone()) {
      continue;
    }
    stdout_line(&format!("  {}[\"{}\"]", nid, label));
    stdout_line(&format!("  {} -->|{}| {}", oid, relation, nid));
  }
  stdout_line("```");
}

pub fn render_symbol_graph_dot(origin: &str, hits: &[SymbolHit], relation: &str) {
  stdout_line("digraph symbol_graph {");
  stdout_line("  rankdir=LR;");
  let oid = graph_id(origin);
  stdout_line(&format!("  {} [label=\"{}\", shape=ellipse];", oid, origin));
  let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
  for h in hits {
    let label = if h.name.is_empty() {
      "?".to_string()
    } else {
      h.name.clone()
    };
    let nid = graph_id(&label);
    if !seen.insert(nid.clone()) {
      continue;
    }
    stdout_line(&format!("  {} [label=\"{}\", shape=box];", nid, label));
    stdout_line(&format!("  {} -> {} [label=\"{}\"];", oid, nid, relation));
  }
  stdout_line("}");
}

pub fn render_context_markdown(res: &crate::schema::ContextResult) {
  stdout_line(&format!(
    "### Context for `{}` (depth {})",
    res.symbol, res.depth
  ));
  if let Some(ref d) = res.definition {
    stdout_line(&format!(
      "\n**definition** — {}:{}",
      d.file_path, d.line_start
    ));
    stdout_line(&format!(
      "```\n{}\n```",
      d.content.lines().take(3).collect::<Vec<_>>().join("\n")
    ));
  }
  if !res.source_context.is_empty() {
    stdout_line("\n**source context**");
    for line in &res.source_context {
      stdout_line(&format!("{:>6}: {}", line.line_number, line.line_content));
    }
  }
  if !res.usages.is_empty() {
    stdout_line(&format!("\n**usages** ({}):", res.usages.len()));
    for u in res.usages.iter().take(8) {
      stdout_line(&format!(
        "- {}:{}  {}",
        u.file_path, u.line_number, u.line_content
      ));
      if !u.before_context.is_empty() || !u.after_context.is_empty() {
        for line in &u.before_context {
          stdout_line(&format!("  {:>6}- {}", line.line_number, line.line_content));
        }
        stdout_line(&format!("  {:>6}: {}", u.line_number, u.line_content));
        for line in &u.after_context {
          stdout_line(&format!("  {:>6}- {}", line.line_number, line.line_content));
        }
      }
      if !u.parents.is_empty() {
        let parents = u
          .parents
          .iter()
          .map(|parent| {
            let name = parent.name.as_deref().unwrap_or("<anonymous>");
            format!(
              "{} `{}` L{}-{}",
              parent.kind, name, parent.line_start, parent.line_end
            )
          })
          .collect::<Vec<_>>()
          .join(" > ");
        stdout_line(&format!("  parents: {}", parents));
      }
      if let Some(body) = &u.body {
        let name = body.name.as_deref().unwrap_or("<anonymous>");
        stdout_line(&format!(
          "  body: {} `{}` L{}-{}",
          body.kind, name, body.line_start, body.line_end
        ));
        stdout_line(&format!("  ```\n{}\n  ```", body.content));
      }
    }
  }
  if !res.callers.is_empty() {
    stdout_line(&format!("\n**callers** ({}):", res.callers.len()));
    for c in res.callers.iter().take(5) {
      stdout_line(&format!(
        "- {}:{}  {}",
        c.file_path,
        c.line_start,
        c.content.lines().next().unwrap_or("")
      ));
    }
  }
  if !res.callees.is_empty() {
    stdout_line(&format!("\n**callees** ({}):", res.callees.len()));
    for c in res.callees.iter().take(5) {
      stdout_line(&format!("- {} ({}:{})", c.name, c.file_path, c.line_start));
    }
  }
  if !res.imports.is_empty() {
    stdout_line(&format!("\n**imports** ({}):", res.imports.len()));
    for i in res.imports.iter().take(8) {
      stdout_line(&format!("- {}", i.content.trim()));
    }
  }
  if !res.recent_history.is_empty() {
    stdout_line(&format!(
      "\n**recent history** ({}):",
      res.recent_history.len()
    ));
    for h in &res.recent_history {
      stdout_line(&format!(
        "- {} [{}] {} — {}",
        &h.commit_hash[..h.commit_hash.len().min(12)],
        h.change_type,
        h.author,
        h.timestamp
      ));
    }
  }
  if !res.todos_inline.is_empty() {
    stdout_line(&format!("\n**todos** ({}):", res.todos_inline.len()));
    for t in &res.todos_inline {
      stdout_line(&format!("- L{} [{}] {}", t.line_number, t.marker, t.text));
    }
  }
  if !res.file_outline.is_empty() {
    stdout_line(&format!(
      "\n**file outline** ({} symbols)",
      res.file_outline.len()
    ));
  }
}

// ──────────────────────── §1.6 format dispatch ────────────────────────
//
// json   — pretty-print the entire envelope as a single JSON document.
// ndjson — one item per line; final line is a summary object carrying
//          schema, exit_status, warnings.
// sarif  — SARIF 2.1.0 minimal-viable: runs[0].results[] mapped from
//          the envelope's hits/projects/paths. Items without file/line
//          (TraceEvent, Milestone) become driver `notifications`.
// github — `::notice file=…,line=…::msg` per item with a file/line.

fn item_array_field(envelope: &serde_json::Value) -> Option<&serde_json::Value> {
  if envelope
    .get("schema")
    .and_then(|value| value.as_str())
    .is_some_and(|schema| schema.starts_with("lmprobe.git_") || schema == "lmprobe.aws_catalog.v1")
  {
    if let Some(rows) = envelope.get("rows").filter(|rows| rows.is_array()) {
      return Some(rows);
    }
  }
  // Probe the well-known item-array fields used by every envelope.
  for k in &[
    "hits",
    "projects",
    "paths",
    "files",
    "milestones",
    "events",
    "candidates",
    "callsites",
    "anchors",
    "sites",
    "keys",
    "operations",
    "routes",
    "calls",
    "evidence",
    "findings",
  ] {
    if let Some(v) = envelope.get(*k) {
      if v.is_array() {
        return Some(v);
      }
    }
  }
  None
}

pub fn render_envelope_ndjson<E: serde::Serialize>(envelope: &E) {
  let v = serde_json::to_value(envelope).expect("serialize envelope");
  if let Some(items) = item_array_field(&v) {
    for item in items.as_array().unwrap() {
      stdout_line(&serde_json::to_string(item).unwrap());
    }
  }
  // Summary line: envelope without the items array.
  let mut summary = v.clone();
  if let Some(map) = summary.as_object_mut() {
    for k in &[
      "files",
      "hits",
      "projects",
      "paths",
      "milestones",
      "events",
      "candidates",
      "callsites",
      "anchors",
      "sites",
      "keys",
      "operations",
      "routes",
      "calls",
      "evidence",
      "findings",
    ] {
      // v1 find NDJSON retains path-string rows. Add the typed snapshots to
      // its summary so a piped action can validate the original selection.
      if *k != "files"
        || v.get("schema").and_then(|schema| schema.as_str()) != Some("lmprobe.find.v1")
      {
        map.remove(*k);
      }
    }
    map.insert("kind".into(), serde_json::Value::String("summary".into()));
    if v
      .get("schema")
      .and_then(|value| value.as_str())
      .is_some_and(|schema| {
        schema.starts_with("lmprobe.git_") || schema == "lmprobe.aws_catalog.v1"
      })
    {
      map.remove("rows");
    }
  }
  stdout_line(&serde_json::to_string(&summary).unwrap());
}

pub fn render_envelope_sarif<E: serde::Serialize>(envelope: &E, verb: &str) {
  let v = serde_json::to_value(envelope).expect("serialize envelope");
  let mut results: Vec<serde_json::Value> = Vec::new();
  if let Some(items) = item_array_field(&v) {
    for item in items.as_array().unwrap() {
      let file = item
        .get("file_path")
        .and_then(|x| x.as_str())
        .or_else(|| item.get("path").and_then(|x| x.as_str()));
      let line = item
        .get("line_start")
        .and_then(|x| x.as_u64())
        .or_else(|| item.get("line_number").and_then(|x| x.as_u64()));
      let msg = item
        .get("name")
        .and_then(|x| x.as_str())
        .or_else(|| item.get("rule_id").and_then(|x| x.as_str()))
        .or_else(|| item.get("line_content").and_then(|x| x.as_str()))
        .or_else(|| item.get("project_type").and_then(|x| x.as_str()))
        .unwrap_or("");
      let mut sarif_result = serde_json::json!({
          "ruleId": format!("lmprobe.{}", verb),
          "level": "note",
          "message": { "text": msg },
      });
      if let (Some(f), Some(l)) = (file, line) {
        sarif_result["locations"] = serde_json::json!([{
            "physicalLocation": {
                "artifactLocation": { "uri": f },
                "region": { "startLine": l }
            }
        }]);
      } else if let Some(f) = file {
        sarif_result["locations"] = serde_json::json!([{
            "physicalLocation": {
                "artifactLocation": { "uri": f }
            }
        }]);
      }
      results.push(sarif_result);
    }
  }
  let sarif = serde_json::json!({
      "version": "2.1.0",
      "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
      "runs": [{
          "tool": {
              "driver": {
                  "name": "lmprobe",
                  "informationUri": "https://github.com/ast-grep/ast-grep",
                  "rules": [{ "id": format!("lmprobe.{}", verb) }]
              }
          },
          "results": results
      }]
  });
  stdout_line(&serde_json::to_string_pretty(&sarif).unwrap());
}

pub fn render_envelope_github<E: serde::Serialize>(envelope: &E, verb: &str) {
  let v = serde_json::to_value(envelope).expect("serialize envelope");
  if let Some(items) = item_array_field(&v) {
    for item in items.as_array().unwrap() {
      let file = item
        .get("file_path")
        .and_then(|x| x.as_str())
        .or_else(|| item.get("path").and_then(|x| x.as_str()));
      let line = item
        .get("line_start")
        .and_then(|x| x.as_u64())
        .or_else(|| item.get("line_number").and_then(|x| x.as_u64()));
      let msg = item
        .get("name")
        .and_then(|x| x.as_str())
        .or_else(|| item.get("rule_id").and_then(|x| x.as_str()))
        .or_else(|| item.get("line_content").and_then(|x| x.as_str()))
        .or_else(|| item.get("project_type").and_then(|x| x.as_str()))
        .unwrap_or("");
      match (file, line) {
        (Some(f), Some(l)) => stdout_line(&format!(
          "::notice file={},line={}::lmprobe.{}: {}",
          f, l, verb, msg
        )),
        (Some(f), None) => stdout_line(&format!("::notice file={}::lmprobe.{}: {}", f, verb, msg)),
        _ => stdout_line(&format!("::notice ::lmprobe.{}: {}", verb, msg)),
      }
    }
  }
}
