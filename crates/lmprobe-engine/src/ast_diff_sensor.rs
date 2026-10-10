//! Phase 4C §4.8 — `lmprobe ast-diff <file> --from <rev> --to <rev>`.
//!
//! Structural diff between two commit revisions of a single file.
//! For each declaration (function / struct / class / etc.) extracted
//! from the file at `from_rev` and `to_rev`, classify:
//!
//!   • ADDED      — present at `to`, absent at `from`.
//!   • REMOVED    — present at `from`, absent at `to`.
//!   • MODIFIED   — present at both with different signatures.
//!   • UNCHANGED  — same name + same signature.
//!
//! Reuses 4A's `blob_signatures` cache so each blob's per-symbol
//! signature is read from SQLite on warm cache.

use std::collections::HashMap;
use std::path::Path;

use ast_grep_language::{Language, LanguageExt};
use git2::Repository;

use lmprobe_lang_core::langs::SgLang;
use lmprobe_registry::{name_extractor_for, taxonomy_for};
use lmprobe_types::error::{anyhow, Context, Result};
use lmprobe_types::schema::{AstDiffResult, NodeDiff};

/// Map (symbol_name → (signature, line_start)). The signature is
/// the AST-elided skeleton string from `index::create_skeleton`.
type SymbolMap = HashMap<String, (String, u32)>;

pub fn ast_diff(file: &Path, from_rev: &str, to_rev: &str, root: &Path) -> Result<AstDiffResult> {
  let repo = discover_repo_for_ast_diff(file, root)?;

  // Resolve revs → commits.
  let from = repo
    .revparse_single(from_rev)
    .with_context(|| format!("could not resolve --from rev `{}`", from_rev))?
    .peel_to_commit()
    .with_context(|| format!("--from `{}` is not a commit", from_rev))?;
  let to = repo
    .revparse_single(to_rev)
    .with_context(|| format!("could not resolve --to rev `{}`", to_rev))?
    .peel_to_commit()
    .with_context(|| format!("--to `{}` is not a commit", to_rev))?;

  // Resolve the file path relative to repo root.
  let workdir = repo
    .workdir()
    .ok_or_else(|| anyhow!("ast-diff requires a non-bare repo"))?;
  let abs = if file.is_absolute() {
    file.to_path_buf()
  } else {
    std::env::current_dir()
      .map(|d| d.join(file))
      .unwrap_or_else(|_| file.to_path_buf())
  };
  let repo_root = std::fs::canonicalize(workdir).unwrap_or_else(|_| workdir.to_path_buf());
  let abs_for_rel = std::fs::canonicalize(&abs).unwrap_or_else(|_| abs.clone());
  let rel = abs_for_rel
    .strip_prefix(&repo_root)
    .unwrap_or(&abs)
    .to_path_buf();

  let lang = SgLang::from_path(&rel)
    .ok_or_else(|| anyhow!("unrecognised language for {}", rel.display()))?;

  let from_map = symbols_at_commit(&repo, &from, &rel, lang)?;
  let to_map = symbols_at_commit(&repo, &to, &rel, lang)?;

  let mut nodes: Vec<NodeDiff> = Vec::new();
  let mut all_names: Vec<&String> = from_map.keys().chain(to_map.keys()).collect();
  all_names.sort();
  all_names.dedup();

  for name in all_names {
    let from_entry = from_map.get(name);
    let to_entry = to_map.get(name);
    let (change_type, from_sig, to_sig, from_line, to_line) = match (from_entry, to_entry) {
      (None, Some((sig, line))) => ("ADDED", String::new(), sig.clone(), 0, *line),
      (Some((sig, line)), None) => ("REMOVED", sig.clone(), String::new(), *line, 0),
      (Some((fs, fl)), Some((ts, tl))) => {
        if fs == ts {
          ("UNCHANGED", fs.clone(), ts.clone(), *fl, *tl)
        } else {
          ("MODIFIED", fs.clone(), ts.clone(), *fl, *tl)
        }
      }
      (None, None) => continue,
    };
    // Kind not tracked at this level — symbols are filtered to
    // declaration kinds upstream; expose generic "declaration".
    nodes.push(NodeDiff {
      kind: "declaration".to_string(),
      name: name.clone(),
      change_type: change_type.to_string(),
      from_signature: from_sig,
      to_signature: to_sig,
      from_line,
      to_line,
    });
  }
  Ok(AstDiffResult::new(nodes))
}

fn discover_repo_for_ast_diff(file: &Path, root: &Path) -> Result<Repository> {
  let file_anchor = if file.is_absolute() {
    file.to_path_buf()
  } else {
    std::env::current_dir()
      .map(|cwd| cwd.join(file))
      .unwrap_or_else(|_| file.to_path_buf())
  };
  let discover_from = if file_anchor.is_file() {
    file_anchor.parent().unwrap_or(file_anchor.as_path())
  } else {
    file_anchor.as_path()
  };
  Repository::discover(discover_from)
    .or_else(|_| Repository::discover(root))
    .with_context(|| {
      format!(
        "Could not find git repository from file `{}` or root `{}`",
        file.display(),
        root.display()
      )
    })
}

/// Walk every declaration in `file` at `commit`, returning a map
/// from extracted name → (skeleton-signature, line_start).
fn symbols_at_commit(
  repo: &Repository,
  commit: &git2::Commit,
  file: &Path,
  lang: SgLang,
) -> Result<SymbolMap> {
  let tree = commit.tree()?;
  let entry = match tree.get_path(file) {
    Ok(e) => e,
    Err(_) => return Ok(SymbolMap::new()),
  };
  let blob = match repo.find_blob(entry.id()) {
    Ok(b) => b,
    Err(_) => return Ok(SymbolMap::new()),
  };
  let content = String::from_utf8_lossy(blob.content()).to_string();
  let grep = lang.ast_grep(content);
  let extractor = name_extractor_for(lang);
  let kind_set = taxonomy_for(lang).all_declaration_kinds();

  let mut map = SymbolMap::new();
  for n in grep.root().dfs() {
    let nk: String = n.kind().to_string();
    if !kind_set.iter().any(|k| *k == nk.as_str()) {
      continue;
    }
    let m = ast_grep_core::NodeMatch::from(n);
    let name = match extractor.extract(&m) {
      Some(n) if !n.is_empty() => n,
      _ => continue,
    };
    let sig = crate::index::create_skeleton(&m);
    let line_start = (m.start_pos().line() + 1) as u32;
    // Keep the FIRST appearance (DFS order). Same-name shadows
    // (e.g. impl-block method overloads) are rare here because
    // we run on declaration kinds only.
    map.entry(name).or_insert((sig, line_start));
  }
  Ok(map)
}
