//! Phase 5A — `cargo xtask new-lang <name>` and `check-lang [<name>]`.
//!
//! `new-lang` reads `xtask/templates/lang/`, substitutes
//! `{{lang}}` / `{{Lang}}` / `{{LANG}}` / `{{tree_sitter_dep}}`,
//! and writes the result to `crates/lmprobe-lang-<name>/`. Then
//! it inserts a per-language dispatch arm into each of the 8
//! marker regions in `crates/lmprobe-registry/src/lib.rs`
//! (idempotent — re-running for the same lang is a no-op) and
//! adds the new crate as a path-dep on
//! `crates/lmprobe-registry/Cargo.toml`.
//!
//! `check-lang` is the drift detector. For each
//! `crates/lmprobe-lang-*/` directory, asserts the required
//! files exist + the path-dep is registered.
//!
//! Substitution: plain `str::replace`; no Handlebars / minijinja
//! dep per design correction §9.5.

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

const TEMPLATE_DIR: &str = "xtask/templates/lang";

/// `cargo xtask new-lang <name>`.
///
/// Three pre-existing-state cases:
/// 1. **Dir doesn't exist** → mkdir + scaffold normally.
/// 2. **Dir exists with only `.gitkeep`** ("seeded empty dir",
///    Phase 5A.6 model): delete the .gitkeep, scaffold into the
///    now-empty dir, remove the dir from the workspace
///    `exclude` list. This is the per-Coder session-spawn path.
/// 3. **Dir exists with other files** → refuse with a clear
///    error.
pub fn scaffold(lang: &str) -> Result<()> {
  validate_name(lang)?;
  let dest = PathBuf::from(format!("crates/lmprobe-lang-{}", lang));
  let was_seeded_empty = is_seeded_empty_dir(&dest)?;
  if dest.exists() && !was_seeded_empty {
    bail!(
            "{} already exists with non-placeholder files; refusing to overwrite. Delete it first if you really want to regenerate.",
            dest.display()
        );
  }
  if was_seeded_empty {
    // Remove the seed placeholder so copy_template_dir() can
    // write into a clean dir without colliding.
    let gitkeep = dest.join(".gitkeep");
    if gitkeep.exists() {
      fs::remove_file(&gitkeep).with_context(|| format!("remove {}", gitkeep.display()))?;
    }
  }
  let template_root = PathBuf::from(TEMPLATE_DIR);
  if !template_root.exists() {
    bail!(
      "template dir `{}` not found — running from workspace root?",
      template_root.display()
    );
  }
  let lang_lc = lang.to_lowercase();
  let lang_pc = pascal_case(lang);
  let lang_uc = lang.to_uppercase();
  let tree_sitter_dep = format!("tree-sitter-{}", lang_lc);

  copy_template_dir(
    &template_root,
    &dest,
    &lang_lc,
    &lang_pc,
    &lang_uc,
    &tree_sitter_dep,
  )?;

  register_dispatch_arms(&lang_lc, &lang_pc)?;
  register_workspace_dep(&lang_lc)?;
  if was_seeded_empty {
    unexclude_workspace_member(&lang_lc)?;
  }

  println!(
    "✔ scaffolded `{}` at {} ({} files written){}",
    lang_lc,
    dest.display(),
    count_files(&dest).unwrap_or(0),
    if was_seeded_empty {
      " [from seeded empty dir; removed from workspace exclude]"
    } else {
      ""
    }
  );
  println!(
    "  next: implement the trait stubs in src/lib.rs and run `cargo build -p lmprobe-lang-{}`",
    lang_lc
  );
  Ok(())
}

/// True iff `dir` exists and contains only `.gitkeep` (no other
/// files, no subdirs). The "seeded empty dir" placeholder shape
/// from Phase 5A.6.
fn is_seeded_empty_dir(dir: &Path) -> Result<bool> {
  if !dir.exists() {
    return Ok(false);
  }
  let mut entries: Vec<String> = Vec::new();
  for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
    let entry = entry?;
    entries.push(entry.file_name().to_string_lossy().into_owned());
  }
  Ok(entries.len() == 1 && entries[0] == ".gitkeep")
}

/// Remove `crates/lmprobe-lang-<lang>` from the workspace
/// `exclude` array in `Cargo.toml`. Idempotent — no-op if not
/// present.
fn unexclude_workspace_member(lang_lc: &str) -> Result<()> {
  let path = PathBuf::from("Cargo.toml");
  let src = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
  let needle = format!("    \"crates/lmprobe-lang-{}\",\n", lang_lc);
  if !src.contains(&needle) {
    return Ok(()); // already removed (or never present)
  }
  let out = src.replacen(&needle, "", 1);
  fs::write(&path, out).with_context(|| format!("write {}", path.display()))?;
  Ok(())
}

/// `cargo xtask check-lang [<name>]`. Per-crate structural
/// integrity check; returns Err with a helpful message on the
/// first violation. Lints all crates if no arg.
pub fn check(lang: Option<&str>) -> Result<()> {
  let crates_dir = PathBuf::from("crates");
  if !crates_dir.exists() {
    bail!("crates/ not found — running from workspace root?");
  }
  let mut targets: Vec<String> = Vec::new();
  if let Some(name) = lang {
    validate_name(name)?;
    targets.push(name.to_string());
  } else {
    for entry in fs::read_dir(&crates_dir)? {
      let entry = entry?;
      let name = entry.file_name().to_string_lossy().into_owned();
      if let Some(rest) = name.strip_prefix("lmprobe-lang-") {
        if rest != "core" {
          targets.push(rest.to_string());
        }
      }
    }
  }
  if targets.is_empty() {
    println!("check-lang: no per-language crates to lint");
    return Ok(());
  }
  let mut violations: Vec<String> = Vec::new();
  let mut checked = 0usize;
  let mut seeded_skipped: Vec<String> = Vec::new();
  for lang in &targets {
    let dir = PathBuf::from(format!("crates/lmprobe-lang-{}", lang));
    // Skip seeded empty dirs (Phase 5A.6 placeholders) — they're
    // not yet scaffolded crates and the per-Coder workflow is
    // expected to scaffold them via `cargo xtask new-lang <X>`.
    // The lint surfaces them in the final tally so the operator
    // sees the count of unscaffolded slots.
    if is_seeded_empty_dir(&dir).unwrap_or(false) {
      seeded_skipped.push(lang.clone());
      continue;
    }
    if let Err(e) = check_single(lang) {
      violations.push(format!("{}: {}", lang, e));
    } else {
      checked += 1;
    }
  }
  if !violations.is_empty() {
    bail!(
      "check-lang found {} violation(s):\n  - {}",
      violations.len(),
      violations.join("\n  - ")
    );
  }
  let suffix = if seeded_skipped.is_empty() {
    String::new()
  } else {
    format!(
      "; {} seeded empty dir(s) skipped: {}",
      seeded_skipped.len(),
      seeded_skipped.join(", ")
    )
  };
  println!(
    "✔ check-lang clean ({} crate(s) verified{})",
    checked, suffix
  );
  Ok(())
}

// ────────────────────────── internals ──────────────────────────

fn validate_name(name: &str) -> Result<()> {
  if name.is_empty() {
    bail!("language name must be non-empty");
  }
  if !name
    .chars()
    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
  {
    bail!(
      "language name `{}` must match /[a-z0-9-]+/ (lowercase, digits, hyphens)",
      name
    );
  }
  Ok(())
}

fn pascal_case(s: &str) -> String {
  // SgLang variants whose canonical name diverges from naive
  // title-case. Hard-coded because the divergences are upstream
  // in `ast-grep-language` and stable across releases. Without
  // this map, `pascal_case("javascript")` returns `"Javascript"`
  // and the xtask-generated registry arm
  // `SgLang::Javascript => &lmprobe_lang_javascript::JavascriptExtractor`
  // fails to compile against the real `SgLang::JavaScript` variant.
  match s {
    "csharp" => return "CSharp".into(),
    "javascript" => return "JavaScript".into(),
    "typescript" => return "TypeScript".into(),
    _ => {}
  }
  // Fallback: hyphen-separated → PascalCase. Handles `c-sharp`
  // → `CSharp` for any future SgLang variant that uses hyphens
  // in the workspace dir name; bare-word langs collapse to
  // simple title-case (`rust` → `Rust`, `kotlin` → `Kotlin`).
  let mut out = String::with_capacity(s.len());
  let mut next_upper = true;
  for c in s.chars() {
    if c == '-' {
      next_upper = true;
    } else if next_upper {
      out.push(c.to_ascii_uppercase());
      next_upper = false;
    } else {
      out.push(c);
    }
  }
  out
}

#[cfg(test)]
mod tests {
  use super::pascal_case;

  #[test]
  fn pascal_case_special_cases() {
    // The three SgLang variants whose canonical name diverges
    // from naive title-case. Verified against
    // `crates/lmprobe-lang-core/src/langs.rs` `SgLang::*`.
    assert_eq!(pascal_case("csharp"), "CSharp");
    assert_eq!(pascal_case("javascript"), "JavaScript");
    assert_eq!(pascal_case("typescript"), "TypeScript");
  }

  #[test]
  fn pascal_case_simple() {
    // The 25 other workspace lang dirs collapse to title-case.
    assert_eq!(pascal_case("rust"), "Rust");
    assert_eq!(pascal_case("kotlin"), "Kotlin");
    assert_eq!(pascal_case("tsx"), "Tsx");
    assert_eq!(pascal_case("cpp"), "Cpp");
    assert_eq!(pascal_case("hcl"), "Hcl");
  }

  #[test]
  fn pascal_case_hyphenated_fallback() {
    // Hyphen-separated fallback for any future SgLang variant
    // whose workspace dir uses a hyphen (none in 5A.6 seed).
    assert_eq!(pascal_case("c-sharp"), "CSharp");
  }
}

fn copy_template_dir(
  src: &Path,
  dst: &Path,
  lang_lc: &str,
  lang_pc: &str,
  lang_uc: &str,
  tree_sitter_dep: &str,
) -> Result<()> {
  fs::create_dir_all(dst).with_context(|| format!("mkdir {}", dst.display()))?;
  for entry in fs::read_dir(src)? {
    let entry = entry?;
    let src_path = entry.path();
    let file_name = entry.file_name().to_string_lossy().into_owned();
    let dst_path = dst.join(&file_name);
    if src_path.is_dir() {
      copy_template_dir(
        &src_path,
        &dst_path,
        lang_lc,
        lang_pc,
        lang_uc,
        tree_sitter_dep,
      )?;
    } else {
      let raw =
        fs::read_to_string(&src_path).with_context(|| format!("read {}", src_path.display()))?;
      let rendered = raw
        .replace("{{lang}}", lang_lc)
        .replace("{{Lang}}", lang_pc)
        .replace("{{LANG}}", lang_uc)
        .replace("{{tree_sitter_dep}}", tree_sitter_dep);
      fs::write(&dst_path, rendered).with_context(|| format!("write {}", dst_path.display()))?;
    }
  }
  Ok(())
}

fn count_files(dir: &Path) -> Result<usize> {
  let mut count = 0;
  for entry in fs::read_dir(dir)? {
    let entry = entry?;
    if entry.path().is_dir() {
      count += count_files(&entry.path())?;
    } else {
      count += 1;
    }
  }
  Ok(count)
}

/// Insert `SgLang::<Lang> => &lmprobe_lang_<lang>::<Lang><Trait>`
/// arms into the 8 dispatch fns in
/// `crates/lmprobe-registry/src/lib.rs`. Each arm is added
/// immediately after the `xtask:per-lang:<fn>:start` marker line
/// inside its `start`/`end` region — works regardless of whether
/// other languages already have arms in the same region.
///
/// Idempotent: if the exact arm is already present, skips.
fn register_dispatch_arms(lang_lc: &str, lang_pc: &str) -> Result<()> {
  let path = PathBuf::from("crates/lmprobe-registry/src/lib.rs");
  let src = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
  let mut out = src;
  let dispatches: &[(&str, &str)] = &[
    ("name_extractor_for", "Extractor"),
    ("call_grammar_for", "CallGrammar"),
    ("import_grammar_for", "ImportGrammar"),
    ("inheritance_grammar_for", "InheritanceGrammar"),
    ("decoration_grammar_for", "DecorationGrammar"),
    ("trace_template_for", "TraceTemplate"),
    ("taxonomy_for", "KindTaxonomy"),
    ("top_level_kinds_for", "TopLevelKinds"),
  ];
  for (fn_name, trait_suffix) in dispatches {
    let start_marker = format!("// xtask:per-lang:{}:start", fn_name);
    let end_marker = format!("// xtask:per-lang:{}:end", fn_name);
    let arm_line = format!(
      "        SgLang::{lang_pc} => &lmprobe_lang_{lang_lc}::{lang_pc}{trait_suffix},\n",
      lang_pc = lang_pc,
      lang_lc = lang_lc,
      trait_suffix = trait_suffix,
    );

    // Idempotency: if the exact arm is already present, skip.
    if out.contains(arm_line.trim_end()) {
      continue;
    }

    // Find the start marker; sanity-check the matching end exists.
    let start_pos = out.find(&start_marker).with_context(|| {
      format!(
        "registry marker `{}` not found — check `crates/lmprobe-registry/src/lib.rs`",
        start_marker
      )
    })?;
    if !out[start_pos..].contains(&end_marker) {
      bail!(
        "registry end marker `{}` not found after start — region malformed",
        end_marker
      );
    }
    // Insert arm right after the start marker line.
    let line_break = out[start_pos..]
      .find('\n')
      .map(|n| start_pos + n + 1)
      .context("malformed registry: no newline after start marker")?;
    out.insert_str(line_break, &arm_line);
  }
  fs::write(&path, out).with_context(|| format!("write {}", path.display()))?;
  Ok(())
}

/// Append `lmprobe-lang-<name>` to the registry's path-dep list
/// inside its `Cargo.toml`.
fn register_workspace_dep(lang_lc: &str) -> Result<()> {
  let path = PathBuf::from("crates/lmprobe-registry/Cargo.toml");
  let src = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
  let dep_line = format!(
    "lmprobe-lang-{lang_lc} = {{ path = \"../lmprobe-lang-{lang_lc}\", version = \"0.1.0\" }}",
    lang_lc = lang_lc
  );
  if src.contains(&dep_line) {
    return Ok(()); // idempotent — already wired
  }
  // Insert under [dependencies]. Find the existing
  // `lmprobe-lang-core = ...` line and append after it.
  let anchor = "lmprobe-lang-core = { path";
  let Some(anchor_pos) = src.find(anchor) else {
    bail!("could not find anchor `{}` in {}", anchor, path.display());
  };
  let line_end = src[anchor_pos..]
    .find('\n')
    .map(|n| anchor_pos + n)
    .unwrap_or(src.len());
  let mut out = String::with_capacity(src.len() + dep_line.len() + 1);
  out.push_str(&src[..line_end]);
  out.push('\n');
  out.push_str(&dep_line);
  out.push_str(&src[line_end..]);
  fs::write(&path, out).with_context(|| format!("write {}", path.display()))?;
  Ok(())
}

fn check_single(lang_lc: &str) -> Result<()> {
  let dir = PathBuf::from(format!("crates/lmprobe-lang-{}", lang_lc));
  let must_exist = [
    "Cargo.toml",
    "src/lib.rs",
    "tests/smoke.rs",
    "fixtures",
    "README.md",
  ];
  for path in &must_exist {
    let p = dir.join(path);
    if !p.exists() {
      bail!("missing {} (required by template)", p.display());
    }
  }
  // Verify the crate is registered as a path-dep on
  // `lmprobe-registry`. Dispatch-arm presence is NOT asserted
  // here — Phase 5A.5 batch-scaffolds 28 stub crates with
  // `--scaffold-only`, intentionally leaving registry arms
  // unwired so the catch-all `_ =>` branch keeps using
  // lang-core's working impls. Per-language Coders wire the
  // dispatch in their own PR when impls are ready.
  let registry_cargo = fs::read_to_string("crates/lmprobe-registry/Cargo.toml")
    .context("read crates/lmprobe-registry/Cargo.toml")?;
  let dep_needle = format!("lmprobe-lang-{} = ", lang_lc);
  if !registry_cargo.contains(&dep_needle) {
    bail!(
      "registry Cargo.toml has no path-dep for `lmprobe-lang-{}` (looked for `{}`)",
      lang_lc,
      dep_needle
    );
  }
  Ok(())
}
