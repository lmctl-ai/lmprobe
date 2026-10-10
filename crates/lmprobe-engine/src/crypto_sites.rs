//! Evidence-only Go crypto algorithm anchor scanner.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ast_grep_language::Language;
use ignore::WalkBuilder;
use regex::Regex;

use lmprobe_lang_core::langs::SgLang;
use lmprobe_types::error::Result;
use lmprobe_types::schema::{
  CryptoGuardAnchor, CryptoSite, CryptoSitesResult, Diagnostic, ExitStatus, LineContext,
  StructuralBody, StructuralParent,
};

use crate::sensors::{
  apply_walk_opts, structural_expansions_for_content, walk_pre_check, PreCheck, WalkOpts,
};

struct AlgorithmMatcher {
  name: String,
  re: Regex,
  import_fragment: String,
}

#[derive(Clone)]
struct RawAnchor {
  line_number: usize,
  line_content: String,
  algorithm: String,
  match_text: String,
}

#[derive(Clone)]
struct LineExpansion {
  parents: Vec<StructuralParent>,
  body: Option<StructuralBody>,
}

pub fn crypto_sites(
  root: &Path,
  algorithms: &[String],
  fips_check: bool,
  around: Option<usize>,
  parents: bool,
  body: bool,
  opts: &WalkOpts,
) -> Result<CryptoSitesResult> {
  let matchers = algorithm_matchers(algorithms);
  if matchers.is_empty() {
    let mut res = CryptoSitesResult::new(Vec::new());
    res.exit_status = ExitStatus::Error;
    res.push_warning(Diagnostic::new(
      "CRYPTO_SITES_NO_ALGORITHMS",
      "crypto-sites requires at least one non-empty --algorithm value",
    ));
    return Ok(res);
  }

  let (files, warnings) = candidate_files(root, opts);
  let mut sites = Vec::new();
  for file in files {
    if let Ok(content) = std::fs::read_to_string(&file) {
      sites.extend(crypto_sites_in_file(
        &file, &content, &matchers, fips_check, around, parents, body, opts,
      ));
    }
  }
  let mut res = CryptoSitesResult::new(sites);
  for warning in warnings {
    res.push_warning(warning);
  }
  Ok(res)
}

fn algorithm_matchers(algorithms: &[String]) -> Vec<AlgorithmMatcher> {
  let mut seen = BTreeSet::new();
  algorithms
    .iter()
    .filter_map(|algorithm| {
      let trimmed = algorithm.trim();
      if trimmed.is_empty() {
        return None;
      }
      let normalized = normalize_algorithm(trimmed);
      if !seen.insert(normalized.clone()) {
        return None;
      }
      Some(AlgorithmMatcher {
        import_fragment: format!("crypto/{normalized}"),
        re: algorithm_regex(&normalized, trimmed),
        name: normalized,
      })
    })
    .collect()
}

fn normalize_algorithm(algorithm: &str) -> String {
  let lower = algorithm.to_ascii_lowercase();
  if let Some(last) = lower.strip_prefix("crypto/") {
    return last.to_string();
  }
  lower.replace(['-', '_'], "")
}

fn algorithm_regex(normalized: &str, original: &str) -> Regex {
  let pattern = match normalized {
    "sha1" => r"(?i)(?:\bsha-?1\b|crypto/sha1|sha1\s*\.\s*(?:New|Sum)|NewSHA1)".to_string(),
    "md5" => r"(?i)(?:\bmd5\b|crypto/md5|md5\s*\.\s*(?:New|Sum)|NewMD5)".to_string(),
    "des" => {
      r"(?i)(?:\bdes\b|crypto/des|des\s*\.\s*(?:NewCipher|NewTripleDESCipher)|NewDES)".to_string()
    }
    _ => format!(r"(?i)\b{}\b", regex::escape(original.trim())),
  };
  Regex::new(&pattern).expect("crypto-sites algorithm regex should compile")
}

#[allow(clippy::too_many_arguments)]
fn crypto_sites_in_file(
  path: &Path,
  content: &str,
  matchers: &[AlgorithmMatcher],
  fips_check: bool,
  around: Option<usize>,
  include_parents: bool,
  include_body: bool,
  opts: &WalkOpts,
) -> Vec<CryptoSite> {
  let lines = content.lines().map(str::to_string).collect::<Vec<_>>();
  let comment_lines = go_comment_only_lines(&lines);
  let imports = go_import_paths(&lines);
  let mut raw = Vec::new();
  let mut dedupe = BTreeSet::new();

  for (idx, line) in lines.iter().enumerate() {
    let line_number = idx + 1;
    if comment_lines.contains(&line_number) {
      continue;
    }
    for matcher in matchers {
      for found in matcher.re.find_iter(line) {
        let import_paths = imports_for_matcher(&imports, matcher);
        let key = (
          line_number,
          matcher.name.clone(),
          found.as_str().to_ascii_lowercase(),
          import_paths.join("\n"),
        );
        if dedupe.insert(key) {
          raw.push(RawAnchor {
            line_number,
            line_content: line.clone(),
            algorithm: matcher.name.clone(),
            match_text: found.as_str().to_string(),
          });
        }
      }
    }
  }

  if raw.is_empty() {
    return Vec::new();
  }

  let mut expansion_lines = raw
    .iter()
    .map(|anchor| anchor.line_number)
    .collect::<Vec<_>>();
  if fips_check {
    expansion_lines.extend(
      guard_anchors(&lines, None)
        .iter()
        .map(|guard| guard.line_number),
    );
  }
  expansion_lines.sort_unstable();
  expansion_lines.dedup();
  let expansions = structural_expansions_for_content(content, path, opts, &expansion_lines)
    .unwrap_or_else(|| vec![(Vec::new(), None); expansion_lines.len()]);
  let expansion_by_line = expansion_lines
    .into_iter()
    .zip(expansions)
    .map(|(line_number, (parents, body))| (line_number, LineExpansion { parents, body }))
    .collect::<BTreeMap<_, _>>();

  let guard_window = if fips_check {
    Some(around.unwrap_or(6))
  } else {
    None
  };
  let guards = guard_anchors(&lines, guard_window);
  let file_path = path.to_string_lossy().into_owned();
  raw
    .into_iter()
    .map(|anchor| {
      let expansion = expansion_by_line.get(&anchor.line_number).cloned();
      let matcher = matchers
        .iter()
        .find(|matcher| matcher.name == anchor.algorithm)
        .expect("raw anchor algorithm should have matcher");
      let nearby_guards = if fips_check {
        guards
          .iter()
          .filter(|guard| guard.line_number.abs_diff(anchor.line_number) <= guard_window.unwrap())
          .cloned()
          .collect()
      } else {
        Vec::new()
      };
      CryptoSite {
        file_path: file_path.clone(),
        line_number: anchor.line_number,
        line_content: anchor.line_content,
        algorithm: anchor.algorithm,
        match_text: anchor.match_text,
        import_paths: imports_for_matcher(&imports, matcher),
        nearby_guards,
        source_context: around
          .map(|around| source_context(&lines, anchor.line_number, around))
          .unwrap_or_default(),
        parents: if include_parents {
          expansion
            .as_ref()
            .map(|expansion| expansion.parents.clone())
            .unwrap_or_default()
        } else {
          Vec::new()
        },
        body: if include_body {
          expansion.and_then(|expansion| expansion.body)
        } else {
          None
        },
        confidence: "syntax-evidence-only".to_string(),
      }
    })
    .collect()
}

fn go_comment_only_lines(lines: &[String]) -> BTreeSet<usize> {
  let mut out = BTreeSet::new();
  let mut in_block = false;
  for (idx, line) in lines.iter().enumerate() {
    let line_number = idx + 1;
    let trimmed = line.trim_start();
    if in_block {
      out.insert(line_number);
      if trimmed.contains("*/") {
        in_block = false;
      }
      continue;
    }
    if trimmed.starts_with("//") {
      out.insert(line_number);
      continue;
    }
    if trimmed.starts_with("/*") {
      out.insert(line_number);
      if !trimmed.contains("*/") {
        in_block = true;
      }
    }
  }
  out
}

fn go_import_paths(lines: &[String]) -> Vec<String> {
  let quoted = Regex::new(r#""([^"]+)""#).expect("import regex should compile");
  let mut paths = BTreeSet::new();
  let mut in_block = false;
  for line in lines {
    let trimmed = line.trim();
    if trimmed.starts_with("import (") {
      in_block = true;
      continue;
    }
    if in_block && trimmed == ")" {
      in_block = false;
      continue;
    }
    if trimmed.starts_with("import ") || in_block {
      for caps in quoted.captures_iter(line) {
        if let Some(path) = caps.get(1) {
          paths.insert(path.as_str().to_string());
        }
      }
    }
  }
  paths.into_iter().collect()
}

fn imports_for_matcher(imports: &[String], matcher: &AlgorithmMatcher) -> Vec<String> {
  imports
    .iter()
    .filter(|path| path.to_ascii_lowercase().contains(&matcher.import_fragment))
    .cloned()
    .collect()
}

fn guard_anchors(lines: &[String], window: Option<usize>) -> Vec<CryptoGuardAnchor> {
  let guard_re = Regex::new(r"(?i)(fips|boringcrypto|GODEBUG|GOFIPS|//go:build|// \+build)")
    .expect("guard regex should compile");
  let mut out = Vec::new();
  for (idx, line) in lines.iter().enumerate() {
    let line_number = idx + 1;
    if window.is_some_and(|window| window == 0) {
      continue;
    }
    if let Some(found) = guard_re.find(line) {
      out.push(CryptoGuardAnchor {
        match_text: found.as_str().to_string(),
        line_number,
        line_content: line.clone(),
      });
    }
  }
  out
}

fn source_context(lines: &[String], line_number: usize, around: usize) -> Vec<LineContext> {
  let target_idx = line_number.saturating_sub(1);
  let start = target_idx.saturating_sub(around);
  let end = (target_idx + around + 1).min(lines.len());
  (start..end)
    .filter_map(|idx| {
      lines.get(idx).map(|line_content| LineContext {
        line_number: idx + 1,
        line_content: line_content.clone(),
      })
    })
    .collect()
}

fn candidate_files(root: &Path, opts: &WalkOpts) -> (Vec<PathBuf>, Vec<Diagnostic>) {
  let mut files = BTreeSet::new();
  let mut warnings = Vec::new();
  if !root.exists() {
    warnings.push(Diagnostic::new(
      "PATH_NOT_FOUND",
      format!("path not found: {}", root.display()),
    ));
    return (Vec::new(), warnings);
  }
  if root.is_file() {
    if is_supported_path(root, opts) {
      files.insert(root.to_path_buf());
    }
    return (files.into_iter().collect(), warnings);
  }

  let mut wb = WalkBuilder::new(root);
  apply_walk_opts(&mut wb, opts, root);
  for entry in wb.build() {
    let Ok(entry) = entry else {
      continue;
    };
    match walk_pre_check(&entry, opts) {
      PreCheck::Process(path, SgLang::Go) => {
        files.insert(path);
      }
      PreCheck::SkipWithDiag(diagnostic) => warnings.push(diagnostic),
      PreCheck::Process(_, _) | PreCheck::Skip => {}
    }
  }
  (files.into_iter().collect(), warnings)
}

fn is_supported_path(path: &Path, opts: &WalkOpts) -> bool {
  opts
    .lang_filter
    .or_else(|| SgLang::from_path(path))
    .is_some_and(|lang| lang == SgLang::Go)
}
