//! Filesystem source for the `lmprobe secrets` Layer-1 scanner.
//!
//! This module owns walking and skip policy; `lmprobe-secrets` owns
//! gitleaks-compatible rule logic. Default walking uses lmprobe's
//! native ignore-aware `WalkOpts`; `gitleaks_compat` disables ignore
//! files so differential tests compare like with like.

use crate::sensors::{apply_walk_opts, WalkOpts};
use ignore::WalkBuilder;
use lmprobe_secrets::{
  analyze_source, analyze_sources_with_module_graph, apply_ast_fp_cut,
  content_has_module_graph_token, content_has_sink_token, content_has_source_token, AstFpCutMode,
  FileScanInput, GitleaksConfig, Layer1Engine, Rule, SecretDiagnostic, SecretFinding,
  SecretsExitStatus, SecretsResult, SinkEvidenceMode, SuppressedSecretFinding,
  DEFAULT_GITLEAKS_CONFIG, SINK_CATALOG_VERSION,
};
use lmprobe_types::error::{anyhow, Result};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{mpsc, Arc, Mutex};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SecretsSourceMode {
  Filesystem,
  Git,
}

#[derive(Debug, Clone)]
pub struct SecretsScanOptions {
  pub config_path: Option<PathBuf>,
  pub max_target_megabytes: usize,
  pub follow_symlinks: bool,
  pub gitleaks_compat: bool,
  pub ast_fp_cut: AstFpCutMode,
  pub sink_evidence: SinkEvidenceMode,
  pub source_mode: SecretsSourceMode,
  pub baseline_path: Option<PathBuf>,
}

impl Default for SecretsScanOptions {
  fn default() -> Self {
    Self {
      config_path: None,
      max_target_megabytes: 5,
      follow_symlinks: false,
      gitleaks_compat: false,
      ast_fp_cut: AstFpCutMode::Annotate,
      sink_evidence: SinkEvidenceMode::Off,
      source_mode: SecretsSourceMode::Filesystem,
      baseline_path: None,
    }
  }
}

#[derive(Debug, Clone, Default)]
struct FileSecrets {
  path: PathBuf,
  content: Option<String>,
  findings: Vec<SecretFinding>,
  suppressed: Vec<SuppressedSecretFinding>,
  semantic: lmprobe_secrets::SemanticScanResult,
  ast_files_parsed: usize,
}

pub fn scan_filesystem(
  root: &Path,
  opts: &WalkOpts,
  scan_opts: &SecretsScanOptions,
) -> Result<SecretsResult> {
  if scan_opts.source_mode == SecretsSourceMode::Git {
    return scan_git_history(root, scan_opts);
  }
  let config = load_config(root, scan_opts.config_path.as_deref())?;
  let engine = Arc::new(Layer1Engine::from_config_str(&config)?);
  let (tx, rx) = mpsc::channel();
  let warnings: Arc<Mutex<Vec<SecretDiagnostic>>> = Arc::new(Mutex::new(Vec::new()));

  let mut walk_opts = opts.clone();
  if scan_opts.gitleaks_compat {
    walk_opts.no_ignore = true;
  }

  let mut wb = WalkBuilder::new(root);
  apply_walk_opts(&mut wb, &walk_opts, root);
  if scan_opts.gitleaks_compat {
    wb.hidden(false);
  }
  wb.follow_links(scan_opts.follow_symlinks);
  let walker = wb.build_parallel();
  let max_bytes = (scan_opts.max_target_megabytes as u64).saturating_mul(1_000_000);
  let source_mode = if scan_opts.gitleaks_compat {
    "filesystem-gitleaks-compat"
  } else {
    "filesystem"
  };

  walker.run(|| {
    let tx = tx.clone();
    let warnings = warnings.clone();
    let engine = engine.clone();
    Box::new(move |result| {
      match result {
        Ok(entry) => {
          let path = entry.path();
          if !path.is_file() {
            return ignore::WalkState::Continue;
          }
          if is_symlink(path) && !scan_opts.follow_symlinks {
            return ignore::WalkState::Continue;
          }
          let Ok(meta) = path.metadata() else {
            warnings.lock().unwrap().push(SecretDiagnostic::new(
              "FILE_METADATA_FAILED",
              format!("failed to read metadata for {}", path.display()),
            ));
            return ignore::WalkState::Continue;
          };
          if meta.len() == 0 {
            return ignore::WalkState::Continue;
          }
          if max_bytes > 0 && meta.len() > max_bytes {
            warnings.lock().unwrap().push(SecretDiagnostic::new(
              "FILE_TOO_LARGE",
              format!(
                "skipped {} ({} bytes > --max-target-megabytes {} MB)",
                path.display(),
                meta.len(),
                scan_opts.max_target_megabytes
              ),
            ));
            return ignore::WalkState::Continue;
          }
          let Ok(bytes) = fs::read(path) else {
            warnings.lock().unwrap().push(SecretDiagnostic::new(
              "FILE_READ_FAILED",
              format!("failed to read {}", path.display()),
            ));
            return ignore::WalkState::Continue;
          };
          let is_application = is_application_mime(&bytes);
          let Ok(content) = String::from_utf8(bytes) else {
            return ignore::WalkState::Continue;
          };
          if is_application && content.contains('\0') {
            return ignore::WalkState::Continue;
          }
          let file_path = path.to_string_lossy().into_owned();
          let findings = engine.scan_file(FileScanInput {
            file_path: &file_path,
            content: &content,
            source_mode,
          });
          let outcome = apply_ast_fp_cut(&file_path, &content, findings, scan_opts.ast_fp_cut);
          let keep_for_module_graph =
            content_has_sink_token(&content) || content_has_module_graph_token(&content);
          let semantic = if !outcome.findings.is_empty()
            || scan_opts.sink_evidence == SinkEvidenceMode::On
            || content_has_sink_token(&content)
            || content_has_source_token(&content)
          {
            analyze_source(path, &content, scan_opts.sink_evidence)
          } else {
            lmprobe_secrets::SemanticScanResult::new()
          };
          let _ = tx.send(FileSecrets {
            path: path.to_path_buf(),
            content: keep_for_module_graph.then_some(content),
            findings: outcome.findings,
            suppressed: outcome.suppressed,
            semantic,
            ast_files_parsed: outcome.ast_files_parsed,
          });
        }
        Err(err) => warnings.lock().unwrap().push(SecretDiagnostic::new(
          "WALK_ERROR",
          format!("filesystem walk error: {err}"),
        )),
      }
      ignore::WalkState::Continue
    })
  });

  drop(tx);
  let mut findings: Vec<SecretFinding> = Vec::new();
  let mut suppressed: Vec<SuppressedSecretFinding> = Vec::new();
  let mut semantic_findings = Vec::new();
  let mut source_evidence = Vec::new();
  let mut confirmed_flows = Vec::new();
  let mut sink_evidence = Vec::new();
  let mut source_files = Vec::new();
  let mut ast_files_parsed = 0;
  for file in rx {
    if let Some(content) = file.content {
      source_files.push((file.path, content));
    }
    findings.extend(file.findings);
    suppressed.extend(file.suppressed);
    semantic_findings.extend(file.semantic.symbol_reachability);
    source_evidence.extend(file.semantic.source_evidence);
    confirmed_flows.extend(file.semantic.confirmed_flows);
    sink_evidence.extend(file.semantic.sink_evidence);
    for warning in file.semantic.warnings {
      warnings
        .lock()
        .unwrap()
        .push(SecretDiagnostic::new(warning.code, warning.message));
    }
    ast_files_parsed += file.ast_files_parsed;
  }
  let cross_file_semantic =
    analyze_sources_with_module_graph(&source_files, scan_opts.sink_evidence);
  semantic_findings.extend(cross_file_semantic.symbol_reachability);
  source_evidence.extend(cross_file_semantic.source_evidence);
  confirmed_flows.extend(cross_file_semantic.confirmed_flows);
  sink_evidence.extend(cross_file_semantic.sink_evidence);
  for warning in cross_file_semantic.warnings {
    warnings
      .lock()
      .unwrap()
      .push(SecretDiagnostic::new(warning.code, warning.message));
  }
  findings.sort_by(|a, b| {
    a.file_path
      .cmp(&b.file_path)
      .then(a.start_line.cmp(&b.start_line))
      .then(a.start_column.cmp(&b.start_column))
      .then(a.rule_id.cmp(&b.rule_id))
  });
  suppressed.sort_by(|a, b| {
    a.finding
      .file_path
      .cmp(&b.finding.file_path)
      .then(a.finding.start_line.cmp(&b.finding.start_line))
      .then(a.finding.start_column.cmp(&b.finding.start_column))
      .then(a.finding.rule_id.cmp(&b.finding.rule_id))
  });

  let findings = apply_suppressions(root, scan_opts, findings);
  let mut result = SecretsResult::new(findings);
  result.sink_catalog_version = SINK_CATALOG_VERSION.into();
  result.suppressed = suppressed;
  result.semantic_findings = semantic_findings;
  result.source_evidence = source_evidence;
  result.confirmed_flows = confirmed_flows;
  result.sink_evidence = sink_evidence;
  if result.exit_status == SecretsExitStatus::NoMatch
    && (!result.semantic_findings.is_empty()
      || !result.confirmed_flows.is_empty()
      || !result.sink_evidence.is_empty())
  {
    result.exit_status = SecretsExitStatus::Matched;
  }
  result.rebuild_canonical_findings();
  result.ast_files_parsed = ast_files_parsed;
  result.warnings = std::mem::take(&mut *warnings.lock().unwrap());
  result.partial = result
    .warnings
    .iter()
    .any(|warning| is_coverage_partial_diagnostic_code(&warning.code));
  if !result.warnings.is_empty() && result.exit_status == SecretsExitStatus::NoMatch {
    result.exit_status = SecretsExitStatus::NoMatch;
  }
  Ok(result)
}

fn is_coverage_partial_diagnostic_code(code: &str) -> bool {
  matches!(
    code,
    "FILE_METADATA_FAILED"
      | "FILE_TOO_LARGE"
      | "FILE_READ_FAILED"
      | "WALK_ERROR"
      | "WALK_ENTRY_ERROR"
  )
}

fn scan_git_history(root: &Path, scan_opts: &SecretsScanOptions) -> Result<SecretsResult> {
  let config = load_config(root, scan_opts.config_path.as_deref())?;
  let engine = Layer1Engine::from_config_str(&config)?;
  let output = Command::new("git")
    .arg("-C")
    .arg(root)
    .args([
      "log",
      "-p",
      "-U0",
      "--full-history",
      "--all",
      "--diff-filter=tuxdb",
    ])
    .output()
    .map_err(|err| anyhow!("failed to run git log: {err}"))?;
  if !output.status.success() {
    return Err(anyhow!(
      "git log failed: {}",
      String::from_utf8_lossy(&output.stderr)
    ));
  }
  let patch = String::from_utf8_lossy(&output.stdout);
  let findings = apply_suppressions(root, scan_opts, scan_git_patch(&engine, &patch));
  let mut result = SecretsResult::new(findings);
  result.sink_catalog_version = SINK_CATALOG_VERSION.into();
  result.rebuild_canonical_findings();
  Ok(result)
}

fn scan_git_patch(engine: &Layer1Engine, patch: &str) -> Vec<SecretFinding> {
  let mut findings = Vec::new();
  let mut commit = String::new();
  let mut path = String::new();
  let mut new_line = 0usize;
  let mut in_hunk = false;
  for line in patch.lines() {
    if let Some(rest) = line.strip_prefix("commit ") {
      commit = rest
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
      in_hunk = false;
      continue;
    }
    if let Some(rest) = line.strip_prefix("+++ b/") {
      path = rest.to_string();
      continue;
    }
    if line.starts_with("+++ /dev/null") {
      path.clear();
      continue;
    }
    if line.starts_with("@@ ") {
      new_line = parse_hunk_new_start(line).unwrap_or(0);
      in_hunk = true;
      continue;
    }
    if !in_hunk || path.is_empty() {
      continue;
    }
    if line.starts_with('+') && !line.starts_with("+++") {
      let added = &line[1..];
      let input = FileScanInput {
        file_path: &path,
        content: added,
        source_mode: "git",
      };
      for mut finding in engine.scan_file(input) {
        finding.line_number = new_line;
        finding.start_line = new_line;
        finding.end_line = new_line;
        finding.file_path = path.clone();
        finding.fingerprint = format!("{}:{}:{}:{}", commit, path, finding.rule_id, new_line);
        for evidence in &mut finding.evidence {
          evidence.file_path = path.clone();
          evidence.start_line = new_line;
          evidence.end_line = new_line;
        }
        findings.push(finding);
      }
      new_line = new_line.saturating_add(1);
    } else if line.starts_with(' ') {
      new_line = new_line.saturating_add(1);
    }
  }
  findings
}

fn parse_hunk_new_start(line: &str) -> Option<usize> {
  let plus = line.split(' ').find(|part| part.starts_with('+'))?;
  let number = plus
    .trim_start_matches('+')
    .split(',')
    .next()
    .unwrap_or_default();
  number.parse().ok()
}

fn apply_suppressions(
  root: &Path,
  scan_opts: &SecretsScanOptions,
  findings: Vec<SecretFinding>,
) -> Vec<SecretFinding> {
  let mut suppressed = std::collections::HashSet::new();
  let ignore_path = root.join(".gitleaksignore");
  if let Ok(content) = fs::read_to_string(ignore_path) {
    for line in content
      .lines()
      .map(str::trim)
      .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
      suppressed.insert(line.to_string());
    }
  }
  if let Some(path) = &scan_opts.baseline_path {
    if let Ok(content) = fs::read_to_string(path) {
      if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
        collect_baseline_fingerprints(&value, &mut suppressed);
      }
    }
  }
  if suppressed.is_empty() {
    return findings;
  }
  findings
    .into_iter()
    .filter(|finding| {
      let global_fingerprint = format!(
        "{}:{}:{}",
        finding.file_path, finding.rule_id, finding.start_line
      );
      // gitleaks derives git-mode line numbers from go-gitdiff text fragments.
      // Embedded diff examples can land one line later than our raw patch
      // counter; keep this narrow to git-mode global .gitleaksignore entries.
      let gitdiff_compat_fingerprints = if finding.source_mode == "git" {
        let adjusted_line = finding.start_line.saturating_add(1);
        let mut fingerprints = vec![format!(
          "{}:{}:{}",
          finding.file_path, finding.rule_id, adjusted_line
        )];
        if let Some((commit, _)) = finding.fingerprint.split_once(':') {
          fingerprints.push(format!(
            "{}:{}:{}:{}",
            commit, finding.file_path, finding.rule_id, adjusted_line
          ));
        }
        fingerprints
      } else {
        Vec::new()
      };
      !suppressed.contains(&finding.fingerprint)
        && !suppressed.contains(&global_fingerprint)
        && !gitdiff_compat_fingerprints
          .iter()
          .any(|fingerprint| suppressed.contains(fingerprint))
    })
    .collect()
}

fn collect_baseline_fingerprints(
  value: &serde_json::Value,
  out: &mut std::collections::HashSet<String>,
) {
  match value {
    serde_json::Value::Array(items) => {
      for item in items {
        collect_baseline_fingerprints(item, out);
      }
    }
    serde_json::Value::Object(map) => {
      if let Some(fp) = map
        .get("Fingerprint")
        .or_else(|| map.get("fingerprint"))
        .and_then(|value| value.as_str())
      {
        out.insert(fp.to_string());
      }
      for value in map.values() {
        collect_baseline_fingerprints(value, out);
      }
    }
    _ => {}
  }
}

fn load_config(root: &Path, config_path: Option<&Path>) -> Result<String> {
  match config_path {
    Some(path) => load_config_path(path),
    None => {
      let discovered = root.join(".gitleaks.toml");
      if root.is_dir() && discovered.exists() {
        load_config_path(&discovered)
      } else {
        Ok(DEFAULT_GITLEAKS_CONFIG.to_string())
      }
    }
  }
}

fn load_config_path(path: &Path) -> Result<String> {
  let content = fs::read_to_string(path)
    .map_err(|err| anyhow!("failed to read secrets config {}: {err}", path.display()))?;
  resolve_config_extends(&content, path)
}

fn resolve_config_extends(content: &str, path: &Path) -> Result<String> {
  let mut config = lmprobe_secrets::load_gitleaks_config_str(content)
    .map_err(|err| anyhow!("failed to parse secrets config {}: {err}", path.display()))?;
  if config.extend.path.is_empty() && !config.extend.use_default {
    return Ok(content.to_string());
  }
  if !config.extend.path.is_empty() && config.extend.use_default {
    return Err(anyhow!(
      "failed to load secrets config {}: extend.path and extend.useDefault cannot both be set",
      path.display()
    ));
  }

  let extension = if config.extend.use_default {
    lmprobe_secrets::load_gitleaks_config_str(DEFAULT_GITLEAKS_CONFIG)
      .map_err(|err| anyhow!("failed to parse vendored gitleaks default config: {err}"))?
  } else {
    let extend_path = Path::new(&config.extend.path);
    let extend_path = if extend_path.is_absolute() {
      extend_path.to_path_buf()
    } else {
      path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(extend_path)
    };
    let extended = fs::read_to_string(&extend_path).map_err(|err| {
      anyhow!(
        "failed to read extended secrets config {} from {}: {err}",
        extend_path.display(),
        path.display()
      )
    })?;
    let resolved = resolve_config_extends(&extended, &extend_path)?;
    lmprobe_secrets::load_gitleaks_config_str(&resolved).map_err(|err| {
      anyhow!(
        "failed to parse extended secrets config {}: {err}",
        extend_path.display()
      )
    })?
  };

  merge_extended_config(&mut config, extension);
  toml::to_string(&config).map_err(|err| {
    anyhow!(
      "failed to serialize merged secrets config {}: {err}",
      path.display()
    )
  })
}

fn merge_extended_config(config: &mut GitleaksConfig, extension: GitleaksConfig) {
  let disabled: HashSet<&str> = config
    .extend
    .disabled_rules
    .iter()
    .map(String::as_str)
    .collect();

  for base_rule in extension.rules {
    if disabled.contains(base_rule.id.as_str()) {
      continue;
    }
    if let Some(current) = config.rules.iter_mut().find(|rule| rule.id == base_rule.id) {
      *current = merge_rule(base_rule, current.clone());
    } else {
      config.rules.push(base_rule);
    }
  }
  if let Some(allowlist) = extension.allowlist {
    config.allowlists.push(allowlist);
  }
  config.allowlists.extend(extension.allowlists);
  config.rules.sort_by(|a, b| a.id.cmp(&b.id));
  config.extend = Default::default();
}

fn merge_rule(mut base: Rule, current: Rule) -> Rule {
  if !current.description.is_empty() {
    base.description = current.description;
  }
  if current.entropy != 0.0 {
    base.entropy = current.entropy;
  }
  if current.secret_group != 0 {
    base.secret_group = current.secret_group;
  }
  if current
    .regex
    .as_deref()
    .is_some_and(|regex| !regex.is_empty())
  {
    base.regex = current.regex;
  }
  if current.path.as_deref().is_some_and(|path| !path.is_empty()) {
    base.path = current.path;
  }
  base.tags.extend(current.tags);
  base.keywords.extend(current.keywords);
  if let Some(allowlist) = current.allowlist {
    base.allowlists.push(allowlist);
  }
  base.allowlists.extend(current.allowlists);
  base.required.extend(current.required);
  base.skip_report |= current.skip_report;
  base
}

fn is_symlink(path: &Path) -> bool {
  fs::symlink_metadata(path)
    .map(|metadata| metadata.file_type().is_symlink())
    .unwrap_or(false)
}

fn is_application_mime(bytes: &[u8]) -> bool {
  infer::get(bytes).is_some_and(|kind| kind.mime_type().starts_with("application/"))
}
