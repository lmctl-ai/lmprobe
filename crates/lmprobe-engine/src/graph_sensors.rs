//! Phase 3B §3.3 graph-primitive sensors: callees, supertypes,
//! subtypes, overrides, exports.
//!
//! All five share the same shape: walk the workspace, ask the
//! per-language grammar trait for patterns matching the verb's
//! semantics, run each pattern against the file's AST, emit
//! `SymbolHit`s. NOT_IMPLEMENTED diagnostic when a language returns
//! an empty pattern Vec — same Phase 2 pattern as callers/imports/
//! decorations.

use std::collections::HashSet;
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex};

use ast_grep_core::matcher::Pattern;
use ast_grep_language::{Language, LanguageExt};
use ignore::WalkBuilder;
use std::fs;

use crate::sensors::{apply_walk_opts, walk_pre_check, PreCheck, WalkOpts};
use lmprobe_lang_core::langs::SgLang;
use lmprobe_registry::{
  call_grammar_for, import_grammar_for, inheritance_grammar_for, name_extractor_for,
};
use lmprobe_types::error::Result;
use lmprobe_types::schema::{
  CalleesResult, Diagnostic, ExitStatus, ExportsResult, OverridesResult, SubtypesResult,
  SupertypesResult, SymbolHit,
};

/// Generic walker shared by the 5 graph primitives. `patterns_for`
/// returns the per-language patterns (empty Vec → NOT_IMPLEMENTED).
/// `kind_label` is the verb name surfaced in `SymbolHit::kind`.
///
/// Returns (hits, warnings, any_supported_lang_walked). The third
/// element is QA TC-080a: callers use it to decide whether NoMatch
/// should be promoted to Matched (no supported lang reached → the
/// verb couldn't run anywhere → warn-but-OK shape).
fn pattern_walk<F>(
  name: &str,
  root: &Path,
  opts: &WalkOpts,
  kind_label: &'static str,
  not_impl_msg: &'static str,
  patterns_for: F,
) -> Result<(Vec<SymbolHit>, Vec<Diagnostic>, bool)>
where
  F: Fn(SgLang, &str) -> Vec<String> + Send + Sync + 'static,
{
  let (tx, rx) = mpsc::channel();
  let unsupported: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
  let parse_fail: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
  let too_large: Arc<Mutex<Vec<Diagnostic>>> = Arc::new(Mutex::new(Vec::new()));
  let any_supported_lang_walked: Arc<std::sync::atomic::AtomicBool> =
    Arc::new(std::sync::atomic::AtomicBool::new(false));
  let name_str = name.to_string();
  let patterns_for = Arc::new(patterns_for);

  let mut wb = WalkBuilder::new(root);
  apply_walk_opts(&mut wb, opts, root);
  let walker = wb.build_parallel();
  walker.run(|| {
    let tx = tx.clone();
    let too_large = too_large.clone();
    let unsupported = unsupported.clone();
    let parse_fail = parse_fail.clone();
    let any_supported = any_supported_lang_walked.clone();
    let name_str = name_str.clone();
    let patterns_for = patterns_for.clone();
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
        let pats = patterns_for(lang, &name_str);
        if pats.is_empty() {
          unsupported.lock().unwrap().insert(format!("{:?}", lang));
          return ignore::WalkState::Continue;
        }
        any_supported.store(true, std::sync::atomic::Ordering::Relaxed);
        let content = match fs::read_to_string(path) {
          Ok(c) => c,
          Err(_) => return ignore::WalkState::Continue,
        };
        if !content.contains(&name_str) {
          return ignore::WalkState::Continue;
        }
        let grep = lang.ast_grep(content);
        let root_node = grep.root();
        let extractor = name_extractor_for(lang);
        let mut parsed_any = false;
        let mut parse_failed_any = false;
        for pat_str in pats {
          let pat = match Pattern::try_new(&pat_str, lang) {
            Ok(p) => {
              parsed_any = true;
              p
            }
            Err(_) => {
              parse_failed_any = true;
              continue;
            }
          };
          for m in root_node.find_all(pat) {
            let extracted = extractor
              .extract(&m)
              .and_then(|name| normalize_relation_name(&name, &m.text()))
              .unwrap_or_default();
            let display_name = if extracted.is_empty() {
              name_str.clone()
            } else {
              extracted
            };
            let _ = tx.send(SymbolHit {
              name: display_name,
              kind: kind_label.to_string(),
              lang: format!("{:?}", lang),
              file_path: path.to_str().unwrap_or("").to_string(),
              line_start: (m.start_pos().line() + 1) as u32,
              content: m.text().to_string(),
              ..Default::default()
            });
          }
        }
        if parse_failed_any && !parsed_any {
          parse_fail.lock().unwrap().insert(format!("{:?}", lang));
        }
      }
      ignore::WalkState::Continue
    })
  });
  drop(tx);
  let mut hits: Vec<SymbolHit> = rx.into_iter().collect();
  hits.sort_by(|a, b| {
    a.file_path
      .cmp(&b.file_path)
      .then(a.line_start.cmp(&b.line_start))
      .then(a.name.cmp(&b.name))
  });
  hits.dedup_by(|a, b| {
    a.file_path == b.file_path && a.line_start == b.line_start && a.name == b.name
  });

  let mut warnings: Vec<Diagnostic> = Vec::new();
  let unsup = std::mem::take(&mut *unsupported.lock().unwrap());
  for lang in unsup {
    warnings.push(Diagnostic::new("NOT_IMPLEMENTED", not_impl_msg).with_lang(lang));
  }
  let pf = std::mem::take(&mut *parse_fail.lock().unwrap());
  for lang in pf {
    warnings.push(
      Diagnostic::new(
        "PATTERN_PARSE_FAILED",
        format!("{} pattern failed to parse for language", kind_label),
      )
      .with_lang(lang),
    );
  }
  for d in std::mem::take(&mut *too_large.lock().unwrap()) {
    warnings.push(d);
  }
  let any_supported = any_supported_lang_walked.load(std::sync::atomic::Ordering::Relaxed);
  Ok((hits, warnings, any_supported))
}

fn normalize_relation_name(extracted: &str, content: &str) -> Option<String> {
  if extracted != "export" {
    return Some(extracted.to_string()).filter(|name| !name.is_empty());
  }
  exported_decl_name(content).or_else(|| Some(extracted.to_string()))
}

fn exported_decl_name(content: &str) -> Option<String> {
  let trimmed = content.trim_start();
  for prefix in ["export class ", "export default class "] {
    if let Some(rest) = trimmed.strip_prefix(prefix) {
      let name = rest
        .split(|c: char| !(c == '_' || c.is_ascii_alphanumeric()))
        .next()
        .unwrap_or("");
      if !name.is_empty() {
        return Some(name.to_string());
      }
    }
  }
  None
}

// ──────────────────────────── callees ─────────────────────────────

/// `lmprobe callees <name>` — find what `name` itself calls.
///
/// 3B implementation: locate `name`'s declaration via
/// `callee_decl_patterns`; for each match, walk the body's AST
/// descendants for call-site nodes and emit them. The shared
/// `pattern_walk` does the workspace walk; we then post-process each
/// matched declaration's body internally.
pub fn callees(name: &str, root: &Path, opts: &WalkOpts) -> Result<CalleesResult> {
  let (decls, mut warnings, any_supported) = pattern_walk(
    name,
    root,
    opts,
    "callee_decl",
    "callees not implemented for language",
    |lang, n| call_grammar_for(lang).callee_decl_patterns(n),
  )?;
  let had_unsupported = warnings.iter().any(|w| w.code == "NOT_IMPLEMENTED");

  // Post-process each declaration: re-parse the file, find the
  // node, walk descendants for call-site kinds.
  let mut hits: Vec<SymbolHit> = Vec::new();
  let mut seen: HashSet<(String, u32, String)> = HashSet::new();
  for d in &decls {
    let decl_start = d.line_start;
    let decl_end = d
      .line_start
      .saturating_add(d.content.lines().count().max(1) as u32)
      .saturating_sub(1);
    // The SymbolHit `lang` field holds the SupportLang Debug repr,
    // which has no public reverse lookup; infer from the file path
    // instead (same approach evolution + history use).
    let lang = match SgLang::from_path(Path::new(&d.file_path)) {
      Some(l) => l,
      None => continue,
    };
    let content = match fs::read_to_string(&d.file_path) {
      Ok(c) => c,
      Err(_) => continue,
    };
    let grep = lang.ast_grep(content);
    let root_node = grep.root();
    for n in root_node.dfs() {
      let nk = n.kind().to_string();
      if !nk.contains("call") {
        continue;
      }
      let m = ast_grep_core::NodeMatch::from(n);
      let line = (m.start_pos().line() + 1) as u32;
      if line < decl_start || line > decl_end {
        continue;
      }
      let text = m.text().to_string();
      let key = (d.file_path.clone(), line, text.clone());
      if !seen.insert(key) {
        continue;
      }
      // Heuristic: the callee's identifier is the first word
      // before `(` in the call text.
      let callee_name = text
        .split('(')
        .next()
        .unwrap_or("")
        .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
        .next()
        .unwrap_or("")
        .to_string();
      if callee_name.is_empty() || callee_name == name {
        continue;
      }
      hits.push(SymbolHit {
        name: callee_name,
        kind: "call_site".to_string(),
        lang: d.lang.clone(),
        file_path: d.file_path.clone(),
        line_start: line,
        content: text,
        ..Default::default()
      });
    }
  }
  let mut env = CalleesResult::new(hits);
  for w in warnings.drain(..) {
    env.push_warning(w);
  }
  if !any_supported && had_unsupported && env.exit_status == ExitStatus::NoMatch {
    env.exit_status = ExitStatus::Matched;
  }
  Ok(env)
}

// ────────────────────────── supertypes ────────────────────────────

pub fn supertypes(name: &str, root: &Path, opts: &WalkOpts) -> Result<SupertypesResult> {
  let (hits, mut warnings, any_supported) = pattern_walk(
    name,
    root,
    opts,
    "supertype_decl",
    "supertypes not implemented for language",
    |lang, n| inheritance_grammar_for(lang).supertype_decl_patterns(n),
  )?;
  let had_unsupported = warnings.iter().any(|w| w.code == "NOT_IMPLEMENTED");
  let mut env = SupertypesResult::new(hits);
  for w in warnings.drain(..) {
    env.push_warning(w);
  }
  if !any_supported && had_unsupported && env.exit_status == ExitStatus::NoMatch {
    env.exit_status = ExitStatus::Matched;
  }
  Ok(env)
}

// ─────────────────────────── subtypes ─────────────────────────────

pub fn subtypes(name: &str, root: &Path, opts: &WalkOpts) -> Result<SubtypesResult> {
  let (hits, mut warnings, any_supported) = pattern_walk(
    name,
    root,
    opts,
    "subtype",
    "subtypes not implemented for language",
    |lang, n| inheritance_grammar_for(lang).subtype_patterns(n),
  )?;
  let had_unsupported = warnings.iter().any(|w| w.code == "NOT_IMPLEMENTED");
  let mut env = SubtypesResult::new(hits);
  for w in warnings.drain(..) {
    env.push_warning(w);
  }
  if !any_supported && had_unsupported && env.exit_status == ExitStatus::NoMatch {
    env.exit_status = ExitStatus::Matched;
  }
  Ok(env)
}

// ─────────────────────────── overrides ────────────────────────────

pub fn overrides(name: &str, root: &Path, opts: &WalkOpts) -> Result<OverridesResult> {
  let (hits, mut warnings, any_supported) = pattern_walk(
    name,
    root,
    opts,
    "override",
    "overrides not implemented for language",
    |lang, n| inheritance_grammar_for(lang).override_patterns(n),
  )?;
  let had_unsupported = warnings.iter().any(|w| w.code == "NOT_IMPLEMENTED");
  let mut env = OverridesResult::new(hits);
  for w in warnings.drain(..) {
    env.push_warning(w);
  }
  if !any_supported && had_unsupported && env.exit_status == ExitStatus::NoMatch {
    env.exit_status = ExitStatus::Matched;
  }
  Ok(env)
}

// ──────────────────────────── exports ─────────────────────────────

pub fn exports(name: &str, root: &Path, opts: &WalkOpts) -> Result<ExportsResult> {
  let (hits, mut warnings, any_supported) = pattern_walk(
    name,
    root,
    opts,
    "export",
    "exports not implemented for language",
    |lang, n| import_grammar_for(lang).export_patterns(n),
  )?;
  let had_unsupported = warnings.iter().any(|w| w.code == "NOT_IMPLEMENTED");
  let mut env = ExportsResult::new(hits);
  for w in warnings.drain(..) {
    env.push_warning(w);
  }
  if !any_supported && had_unsupported && env.exit_status == ExitStatus::NoMatch {
    env.exit_status = ExitStatus::Matched;
  }
  Ok(env)
}
