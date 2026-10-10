//! Phase 3B §3.4 — `lmprobe context <symbol>`.
//!
//! Composes a one-shot bundle: definition, signature, file outline,
//! 1-hop callers + callees, file imports, last 3 commits touching
//! the symbol, and any TODO/FIXME inside the def-containing region.
//!
//! Performance budget per CC3: <250ms warm-cache. Achieved by
//! running each component sensor against the file already located
//! by the def-pass, not re-walking the workspace per component.
//!
//! Exit-status semantics: a found definition makes the bundle
//! meaningfully anchored, even when callers/callees are empty.

use std::path::{Path, PathBuf};

use crate::sensors::WalkOpts;
use lmprobe_types::error::Result;
use lmprobe_types::schema::{
  BlameEntry, ContextResult, Diagnostic, GrepHit, LineContext, SymbolHit, TodoEntry,
};

const RECENT_COMMITS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextAnchorMode {
  Def,
  Usage,
}

pub fn context(
  symbol: &str,
  root: &Path,
  depth: u32,
  opts: &WalkOpts,
  recent_history_limit: Option<usize>,
  import_aware: bool,
) -> Result<ContextResult> {
  context_with_around(
    symbol,
    root,
    depth,
    opts,
    recent_history_limit,
    import_aware,
    None,
  )
}

pub fn context_with_around(
  symbol: &str,
  root: &Path,
  depth: u32,
  opts: &WalkOpts,
  recent_history_limit: Option<usize>,
  import_aware: bool,
  around: Option<usize>,
) -> Result<ContextResult> {
  context_with_options(
    symbol,
    root,
    depth,
    opts,
    recent_history_limit,
    import_aware,
    around,
    ContextAnchorMode::Def,
    false,
    false,
  )
}

#[allow(clippy::too_many_arguments)]
pub fn context_with_options(
  symbol: &str,
  root: &Path,
  depth: u32,
  opts: &WalkOpts,
  recent_history_limit: Option<usize>,
  import_aware: bool,
  around: Option<usize>,
  mode: ContextAnchorMode,
  parents: bool,
  body: bool,
) -> Result<ContextResult> {
  if mode == ContextAnchorMode::Usage {
    return usage_context(symbol, root, depth, opts, around, parents, body);
  }

  let location_resolution = resolve_location_symbol(symbol, root, opts)?;
  let bundle_symbol = location_resolution
    .as_ref()
    .map(|resolved| resolved.symbol.as_str())
    .unwrap_or(symbol);
  let mut env = ContextResult::new(bundle_symbol.to_string(), depth);

  // 1. Locate the definition. `def` is exact-match by default.
  let mut import_bridge = None;
  let mut resolved_symbol = bundle_symbol.to_string();
  let mut synthetic_svelte = false;
  let def_hit = match location_resolution.as_ref() {
    Some(resolved) => Some(resolved.definition.clone()),
    None => match find_definition(symbol, root, opts)? {
      Some(def) => Some(def),
      None if import_aware => match find_one_hop_imported_definition(symbol, root, opts)? {
        Some(resolved) => {
          import_bridge = Some(resolved.import_hit);
          resolved_symbol = resolved.symbol;
          Some(resolved.definition)
        }
        None => None,
      },
      None => {
        let synthetic = synthetic_svelte_definition(symbol, root);
        synthetic_svelte = synthetic.is_some();
        synthetic
      }
    },
  };
  if let Some(resolved) = location_resolution.as_ref() {
    env.push_warning(lmprobe_types::schema::Diagnostic::new(
      "CONTEXT_LOCATION_RESOLVED",
      format!(
        "resolved `{}` to enclosing symbol `{}` at {}:{}",
        symbol, resolved.symbol, resolved.definition.file_path, resolved.definition.line_start
      ),
    ));
  }
  env.definition = def_hit.clone();

  // If we couldn't find the symbol, return early — most components
  // need the def to anchor their work.
  let def = match def_hit {
    Some(d) => d,
    None => {
      let mut diagnostic = lmprobe_types::schema::Diagnostic::new(
        "SYMBOL_NOT_FOUND",
        format!("`{}` not found at HEAD; context bundle is partial", symbol),
      );
      if has_possible_usages(symbol, root, opts)? {
        diagnostic = diagnostic.with_hint(format!(
          "found possible usages; try `lmprobe context --usage {} {} --around 8`",
          symbol,
          root.display()
        ));
      }
      env.push_warning(diagnostic);
      env.exit_status = lmprobe_types::schema::ExitStatus::NoMatch;
      return Ok(env);
    }
  };
  if let Some(around) = around {
    let anchor_path = location_resolution
      .as_ref()
      .map(|resolved| resolved.anchor_path.clone())
      .unwrap_or_else(|| PathBuf::from(&def.file_path));
    let anchor_line = location_resolution
      .as_ref()
      .map(|resolved| resolved.anchor_line)
      .unwrap_or(def.line_start);
    env.source_context = source_context(&anchor_path, root, anchor_line, around);
  }
  if synthetic_svelte {
    let svelte_outline = svelte_file_outline(Path::new(&def.file_path), body);
    env.push_warning(
      Diagnostic::new(
        "SVELTE_SYNTHETIC_CONTEXT",
        "resolved .svelte file as a synthetic component anchor",
      )
      .with_hint("Svelte structural parsing is limited; file_outline contains synthetic script/template anchors, while callers, callees, AST parents, and AST body are omitted"),
    );
    env.callers = Vec::new();
    env.callees = Vec::new();
    env.imports = Vec::new();
    env.recent_history = Vec::new();
    env.todos_inline = inline_todos(&def);
    env.file_outline = svelte_outline;
    env.exit_status = lmprobe_types::schema::ExitStatus::Matched;
    return Ok(env);
  }
  let import_aware_resolved = import_bridge.is_some();
  let location_resolved = location_resolution.is_some();
  let component_root = if location_resolved || (import_aware_resolved && root.is_file()) {
    PathBuf::from(&def.file_path)
  } else {
    root.to_path_buf()
  };

  // 2. 1-hop callers + callees — only meaningful for function-
  // shaped symbols. Skip the workspace walk for struct/class/enum/
  // trait declarations where calls don't apply.
  let is_function = is_function_kind(&def.kind);
  if is_function {
    env.callers = crate::sensors::callers(&resolved_symbol, &component_root, opts)?.hits;
    env.callees = crate::graph_sensors::callees(&resolved_symbol, &component_root, opts)?.hits;
  } else {
    env.callers = Vec::new();
    env.callees = Vec::new();
  }

  // 3. File imports — run `imports` on every name in the def file
  // would be wasteful; instead extract `use`/`import`/`require`
  // lines from the def file directly.
  env.imports = file_imports(&def);
  if let Some(import_hit) = import_bridge {
    env.imports.insert(0, import_hit);
    env.push_warning(lmprobe_types::schema::Diagnostic::new(
      "IMPORT_AWARE_CONTEXT",
      "resolved definition through one relative import/re-export hop",
    ));
  }

  // 4. Recent history — last N entries from blame on the symbol.
  // Phase 6B M4 — `--recent-history-limit N` overrides the default
  // `RECENT_COMMITS`. `None` keeps the existing default-3 shape so
  // the no-flag path is byte-equal to round-1 behavior.
  let cap = recent_history_limit.unwrap_or(RECENT_COMMITS);
  let history = crate::history_sensors::blame(&resolved_symbol, &component_root)?;
  env.recent_history = history
    .entries
    .into_iter()
    .take(cap)
    .collect::<Vec<BlameEntry>>();

  // 5. Inline TODOs — scan the def text for TODO/FIXME markers.
  env.todos_inline = inline_todos(&def);

  // 6. File outline — ast-grep search with --outline restricted
  // to the def's file.
  env.file_outline = file_outline(&def, opts)?;

  env.exit_status = if has_meaningful_payload(&env) {
    lmprobe_types::schema::ExitStatus::Matched
  } else {
    lmprobe_types::schema::ExitStatus::NoMatch
  };
  Ok(env)
}

fn usage_context(
  symbol: &str,
  root: &Path,
  depth: u32,
  opts: &WalkOpts,
  around: Option<usize>,
  parents: bool,
  body: bool,
) -> Result<ContextResult> {
  let mut env = ContextResult::new(symbol.to_string(), depth);
  let mut usages = find_usage_hits(symbol, root, opts)?;
  if let Some(around) = around {
    attach_usage_around(&mut usages, around);
  }
  if parents || body {
    crate::sensors::attach_grep_structural_expansions(&mut usages, opts, parents, body);
  }
  env.exit_status = lmprobe_types::schema::ExitStatus::from_count(usages.len());
  env.usages = usages;
  Ok(env)
}

fn find_usage_hits(symbol: &str, root: &Path, opts: &WalkOpts) -> Result<Vec<GrepHit>> {
  Ok(crate::sensors::refs(symbol, root, true, false, false, opts)?.hits)
}

fn has_possible_usages(symbol: &str, root: &Path, opts: &WalkOpts) -> Result<bool> {
  crate::sensors::refs_exists(symbol, root, opts)
}

fn attach_usage_around(usages: &mut [GrepHit], around: usize) {
  use std::collections::HashMap;

  let mut file_cache: HashMap<String, Vec<String>> = HashMap::new();
  for usage in usages {
    let lines = file_cache
      .entry(usage.file_path.clone())
      .or_insert_with(|| {
        std::fs::read_to_string(&usage.file_path)
          .unwrap_or_default()
          .lines()
          .map(str::to_string)
          .collect()
      });
    let target_idx = usage.line_number.saturating_sub(1);
    let before_start = target_idx.saturating_sub(around);
    usage.before_context = (before_start..target_idx)
      .filter_map(|idx| {
        lines.get(idx).map(|line_content| LineContext {
          line_number: idx + 1,
          line_content: line_content.clone(),
        })
      })
      .collect();
    let after_end = (target_idx + around + 1).min(lines.len());
    usage.after_context = ((target_idx + 1)..after_end)
      .filter_map(|idx| {
        lines.get(idx).map(|line_content| LineContext {
          line_number: idx + 1,
          line_content: line_content.clone(),
        })
      })
      .collect();
  }
}

fn find_definition(symbol: &str, root: &Path, opts: &WalkOpts) -> Result<Option<SymbolHit>> {
  let (def_hits, _) = crate::sensors::search_symbol_hits(
    root,
    true,
    crate::sensors::SearchConstraints {
      query: Some(symbol.to_string()),
      exact_name: true,
      ..Default::default()
    },
    opts,
  )?;
  Ok(def_hits.into_iter().next())
}

fn synthetic_svelte_definition(symbol: &str, root: &Path) -> Option<SymbolHit> {
  if !is_svelte_file(root) {
    return None;
  }
  let stem = root.file_stem().and_then(|stem| stem.to_str())?;
  if stem != symbol {
    return None;
  }
  let content = std::fs::read_to_string(root).ok()?;
  let first_line = content.lines().next().unwrap_or_default().to_string();
  Some(SymbolHit {
    name: symbol.to_string(),
    kind: "svelte_component".to_string(),
    lang: "Svelte".to_string(),
    file_path: root.to_string_lossy().into_owned(),
    line_start: 1,
    content: first_line,
    ..Default::default()
  })
}

fn is_svelte_file(path: &Path) -> bool {
  path
    .extension()
    .and_then(|ext| ext.to_str())
    .is_some_and(|ext| ext.eq_ignore_ascii_case("svelte"))
}

fn svelte_file_outline(path: &Path, include_body: bool) -> Vec<SymbolHit> {
  let Ok(content) = std::fs::read_to_string(path) else {
    return Vec::new();
  };
  let lines = content.lines().collect::<Vec<_>>();
  let mut outline = Vec::new();
  let mut script_ranges = Vec::new();
  let mut idx = 0;
  while idx < lines.len() {
    let line = lines[idx];
    if !line.contains("<script") {
      idx += 1;
      continue;
    }
    let start = idx;
    let mut end = idx;
    while end < lines.len() && !lines[end].contains("</script>") {
      end += 1;
    }
    if end >= lines.len() {
      end = lines.len().saturating_sub(1);
    }
    let is_module = is_svelte_module_script_tag(line);
    let kind = if is_module {
      "svelte_module_script"
    } else {
      "svelte_script"
    };
    let name = if is_module { "module_script" } else { "script" };
    outline.push(svelte_hit(
      path,
      name,
      kind,
      start + 1,
      svelte_anchor_content(&lines, start, end, include_body),
    ));
    for decl_idx in (start + 1)..end {
      if let Some((name, kind)) = svelte_script_anchor(lines[decl_idx]) {
        outline.push(svelte_hit(
          path,
          &name,
          kind,
          decl_idx + 1,
          lines[decl_idx].trim().to_string(),
        ));
      }
    }
    script_ranges.push((start, end));
    idx = end.saturating_add(1);
  }

  for (line_idx, line) in lines.iter().enumerate() {
    if script_ranges
      .iter()
      .any(|(start, end)| line_idx >= *start && line_idx <= *end)
    {
      continue;
    }
    for (name, kind) in svelte_template_anchors(line) {
      outline.push(svelte_hit(
        path,
        &name,
        kind,
        line_idx + 1,
        line.trim().to_string(),
      ));
    }
  }
  outline
}

fn svelte_anchor_content(lines: &[&str], start: usize, end: usize, include_body: bool) -> String {
  if include_body {
    lines[start..=end].join("\n")
  } else {
    lines[start].trim().to_string()
  }
}

fn svelte_script_anchor(line: &str) -> Option<(String, &'static str)> {
  let trimmed = line.trim();
  for rune in ["$effect", "$derived", "$state", "$props"] {
    if trimmed.starts_with(rune) {
      return Some((rune.to_string(), "svelte_rune_call"));
    }
  }
  for prefix in ["export let ", "let ", "const ", "function "] {
    if let Some(rest) = trimmed.strip_prefix(prefix) {
      let name = rest
        .trim_start()
        .split(|c: char| !(c == '_' || c.is_ascii_alphanumeric()))
        .next()
        .unwrap_or("");
      if !name.is_empty() {
        return Some((name.to_string(), "svelte_script_declaration"));
      }
    }
  }
  None
}

fn is_svelte_module_script_tag(line: &str) -> bool {
  let tag = line.split('>').next().unwrap_or(line);
  tag.contains("context=\"module\"")
    || tag.contains("context='module'")
    || tag.split_whitespace().any(|part| {
      part
        .trim_matches(|c| c == '<' || c == '>' || c == '/')
        .trim_end_matches('/')
        == "module"
    })
}

fn svelte_template_anchors(line: &str) -> Vec<(String, &'static str)> {
  let trimmed = line.trim_start();
  let mut anchors = Vec::new();
  if trimmed.contains("{#if") {
    anchors.push(("{#if}".to_string(), "svelte_if_block"));
  }
  if trimmed.contains("{#each") {
    anchors.push(("{#each}".to_string(), "svelte_each_block"));
  }
  if trimmed.contains("bind:this") {
    anchors.push(("bind:this".to_string(), "svelte_bind_this"));
  }
  if let Some(name) = component_tag_name(trimmed) {
    anchors.push((name, "svelte_component_usage"));
  }
  anchors
}

fn component_tag_name(trimmed: &str) -> Option<String> {
  let rest = trimmed.strip_prefix('<')?;
  let first = rest.chars().next()?;
  if !first.is_ascii_uppercase() {
    return None;
  }
  let name = rest
    .split(|c: char| c.is_whitespace() || c == '>' || c == '/' || c == '{')
    .next()
    .unwrap_or("");
  if name.is_empty() {
    None
  } else {
    Some(name.to_string())
  }
}

fn svelte_hit(
  path: &Path,
  name: &str,
  kind: &str,
  line_start: usize,
  content: String,
) -> SymbolHit {
  SymbolHit {
    name: name.to_string(),
    kind: kind.to_string(),
    lang: "Svelte".to_string(),
    file_path: path.to_string_lossy().into_owned(),
    line_start: line_start as u32,
    content,
    ..Default::default()
  }
}

struct LocationResolution {
  symbol: String,
  definition: SymbolHit,
  anchor_path: PathBuf,
  anchor_line: u32,
}

fn resolve_location_symbol(
  symbol: &str,
  root: &Path,
  opts: &WalkOpts,
) -> Result<Option<LocationResolution>> {
  let Some((raw_path, line)) = parse_location_symbol(symbol) else {
    return Ok(None);
  };
  let mut path = PathBuf::from(raw_path);
  if path.is_relative() && root.is_dir() {
    path = root.join(path);
  }
  if !path.is_file() {
    return Ok(None);
  }
  let (mut hits, _) = crate::sensors::search_symbol_hits(
    &path,
    false,
    crate::sensors::SearchConstraints::default(),
    opts,
  )?;
  hits.retain(|hit| is_location_context_candidate(&hit.kind) && hit_contains_line(hit, line));
  hits.sort_by(|a, b| {
    a.line_start
      .cmp(&b.line_start)
      .then_with(|| declaration_rank(&a.kind).cmp(&declaration_rank(&b.kind)))
  });
  let Some(definition) = hits.pop() else {
    return Ok(None);
  };
  Ok(Some(LocationResolution {
    symbol: definition.name.clone(),
    definition,
    anchor_path: path,
    anchor_line: line,
  }))
}

fn source_context(path: &Path, root: &Path, line: u32, around: usize) -> Vec<LineContext> {
  let mut candidates = Vec::new();
  candidates.push(path.to_path_buf());
  if path.is_relative() {
    if root.is_dir() {
      candidates.push(root.join(path));
    } else if let Some(parent) = root.parent() {
      candidates.push(parent.join(path));
    }
  }
  let content = candidates
    .into_iter()
    .find_map(|candidate| std::fs::read_to_string(candidate).ok());
  let Some(content) = content else {
    return Vec::new();
  };
  let lines = content.lines().collect::<Vec<_>>();
  if line == 0 || lines.is_empty() {
    return Vec::new();
  }
  let target_idx = line.saturating_sub(1) as usize;
  if target_idx >= lines.len() {
    return Vec::new();
  }
  let start = target_idx.saturating_sub(around);
  let end = (target_idx + around + 1).min(lines.len());
  (start..end)
    .map(|idx| LineContext {
      line_number: idx + 1,
      line_content: lines[idx].to_string(),
    })
    .collect()
}

fn hit_contains_line(hit: &SymbolHit, line: u32) -> bool {
  let line_count = hit.content.lines().count().max(1) as u32;
  let line_end = hit.line_start.saturating_add(line_count.saturating_sub(1));
  hit.line_start <= line && line <= line_end
}

fn is_location_context_candidate(kind: &str) -> bool {
  is_function_kind(kind)
    || kind.contains("class")
    || kind.contains("struct")
    || kind.contains("interface")
    || kind.contains("trait")
    || kind.contains("enum")
    || kind.contains("type_alias")
    || kind.contains("const")
    || kind.contains("macro")
}

fn parse_location_symbol(symbol: &str) -> Option<(&str, u32)> {
  let (path, line) = symbol.rsplit_once(':')?;
  if path.is_empty() {
    return None;
  }
  let line = line.parse::<u32>().ok()?;
  if line == 0 {
    return None;
  }
  Some((path, line))
}

fn declaration_rank(kind: &str) -> u8 {
  if is_function_kind(kind) {
    2
  } else if kind.contains("class")
    || kind.contains("struct")
    || kind.contains("interface")
    || kind.contains("trait")
    || kind.contains("enum")
  {
    1
  } else {
    0
  }
}

struct ImportedDefinition {
  symbol: String,
  definition: SymbolHit,
  import_hit: SymbolHit,
}

fn find_one_hop_imported_definition(
  symbol: &str,
  root: &Path,
  opts: &WalkOpts,
) -> Result<Option<ImportedDefinition>> {
  for source in import_scan_files(root) {
    let content = match std::fs::read_to_string(&source) {
      Ok(content) => content,
      Err(_) => continue,
    };
    for edge in relative_import_edges(symbol, &source, &content) {
      for target in edge.targets {
        if let Some(definition) = find_definition(&edge.target_symbol, &target, opts)? {
          return Ok(Some(ImportedDefinition {
            symbol: edge.target_symbol,
            definition,
            import_hit: edge.hit,
          }));
        }
      }
    }
  }
  Ok(None)
}

fn import_scan_files(root: &Path) -> Vec<PathBuf> {
  if root.is_file() {
    return vec![root.to_path_buf()];
  }
  Vec::new()
}

struct ImportEdge {
  target_symbol: String,
  targets: Vec<PathBuf>,
  hit: SymbolHit,
}

fn relative_import_edges(symbol: &str, source: &Path, content: &str) -> Vec<ImportEdge> {
  let mut edges = Vec::new();
  for (idx, line) in content.lines().enumerate() {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
      continue;
    }
    if let Some(edge) = js_ts_import_edge(symbol, source, idx, line, trimmed) {
      edges.push(edge);
      continue;
    }
    if let Some(edge) = python_import_edge(symbol, source, idx, line, trimmed) {
      edges.push(edge);
      continue;
    }
    if let Some(edge) = rust_import_edge(symbol, source, idx, line, trimmed) {
      edges.push(edge);
    }
  }
  edges
}

fn js_ts_import_edge(
  symbol: &str,
  source: &Path,
  idx: usize,
  line: &str,
  trimmed: &str,
) -> Option<ImportEdge> {
  let from_pos = trimmed.rfind(" from ")?;
  let prefix = trimmed[..from_pos].trim();
  if !(prefix.starts_with("import ") || prefix.starts_with("export ")) {
    return None;
  }
  let spec = quoted_module_spec(&trimmed[from_pos + " from ".len()..])?;
  if !spec.starts_with('.') {
    return None;
  }
  let target_symbol = named_binding_target(symbol, prefix)?;
  Some(ImportEdge {
    target_symbol,
    targets: resolve_js_ts_targets(source, spec),
    hit: import_hit(symbol, "relative_import", source, idx, line),
  })
}

fn python_import_edge(
  symbol: &str,
  source: &Path,
  idx: usize,
  line: &str,
  trimmed: &str,
) -> Option<ImportEdge> {
  let rest = trimmed.strip_prefix("from ")?;
  let (module, names) = rest.split_once(" import ")?;
  if !module.starts_with('.') {
    return None;
  }
  let target_symbol = import_list_target(symbol, names)?;
  Some(ImportEdge {
    target_symbol,
    targets: resolve_python_targets(source, module, symbol),
    hit: import_hit(symbol, "relative_import", source, idx, line),
  })
}

fn rust_import_edge(
  symbol: &str,
  source: &Path,
  idx: usize,
  line: &str,
  trimmed: &str,
) -> Option<ImportEdge> {
  let rest = trimmed
    .strip_prefix("pub use ")
    .or_else(|| trimmed.strip_prefix("use "))?;
  let rest = rest.trim_end_matches(';').trim();
  let target_symbol = rust_path_target(symbol, rest)?;
  Some(ImportEdge {
    target_symbol,
    targets: resolve_rust_targets(source, rest, symbol),
    hit: import_hit(symbol, "relative_import", source, idx, line),
  })
}

fn quoted_module_spec(s: &str) -> Option<&str> {
  let s = s.trim().trim_end_matches(';').trim();
  let quote = s.chars().next()?;
  if quote != '"' && quote != '\'' {
    return None;
  }
  let rest = &s[1..];
  let end = rest.find(quote)?;
  Some(&rest[..end])
}

fn named_binding_target(symbol: &str, prefix: &str) -> Option<String> {
  if let Some(open) = prefix.find('{') {
    let close = prefix.rfind('}')?;
    return import_list_target(symbol, &prefix[open + 1..close]);
  }

  let local = prefix
    .strip_prefix("import ")
    .map(str::trim)
    .and_then(|s| s.split(',').next())
    .map(str::trim)?;
  if local == symbol {
    Some(symbol.to_string())
  } else {
    None
  }
}

fn import_list_target(symbol: &str, names: &str) -> Option<String> {
  for raw in names.split(',') {
    let part = raw.trim().trim_matches('{').trim_matches('}').trim();
    if part.is_empty() {
      continue;
    }
    let (remote, local) = if let Some((remote, local)) = part.split_once(" as ") {
      (remote.trim(), local.trim())
    } else {
      let mut words = part.split_whitespace();
      let remote = words.next().unwrap_or("");
      match (words.next(), words.next()) {
        (Some("as"), Some(local)) => (remote, local),
        _ => (remote, remote),
      }
    };
    if local == symbol || remote == symbol {
      return Some(remote.to_string());
    }
  }
  None
}

fn rust_path_target(symbol: &str, path: &str) -> Option<String> {
  let cleaned = path.trim();
  let candidate = cleaned
    .rsplit("::")
    .next()
    .unwrap_or(cleaned)
    .split_whitespace()
    .next()
    .unwrap_or("")
    .trim_matches('{')
    .trim_matches('}');
  if candidate == symbol && is_relative_rust_path(cleaned) {
    Some(symbol.to_string())
  } else {
    None
  }
}

fn is_relative_rust_path(path: &str) -> bool {
  path.starts_with("self::") || path.starts_with("super::")
}

fn resolve_js_ts_targets(source: &Path, spec: &str) -> Vec<PathBuf> {
  let Some(parent) = source.parent() else {
    return Vec::new();
  };
  let base = parent.join(spec);
  let mut candidates = Vec::new();
  push_existing(&mut candidates, &base);
  for ext in ["ts", "tsx", "js", "jsx", "mjs", "cjs"] {
    push_existing(&mut candidates, &base.with_extension(ext));
  }
  for ext in ["ts", "tsx", "js", "jsx"] {
    push_existing(&mut candidates, &base.join(format!("index.{ext}")));
  }
  candidates
}

fn resolve_python_targets(source: &Path, module: &str, symbol: &str) -> Vec<PathBuf> {
  let Some(mut base) = source.parent().map(Path::to_path_buf) else {
    return Vec::new();
  };
  let dots = module.chars().take_while(|c| *c == '.').count();
  for _ in 1..dots {
    base.pop();
  }
  let tail = module.trim_start_matches('.');
  if !tail.is_empty() {
    for part in tail.split('.') {
      base.push(part);
    }
  }

  let mut candidates = Vec::new();
  push_existing(&mut candidates, &base.with_extension("py"));
  push_existing(&mut candidates, &base.join("__init__.py"));
  if tail.is_empty() {
    push_existing(&mut candidates, &base.join(format!("{symbol}.py")));
  }
  candidates
}

fn resolve_rust_targets(source: &Path, path: &str, symbol: &str) -> Vec<PathBuf> {
  let Some(mut base) = source.parent().map(Path::to_path_buf) else {
    return Vec::new();
  };
  let mut segments = path.trim_end_matches(';').split("::").peekable();
  while let Some(segment) = segments.next() {
    match segment.trim() {
      "self" => {}
      "super" => {
        base.pop();
      }
      part if part == symbol => break,
      part if segments.peek().is_some() => base.push(part),
      _ => {}
    }
  }

  let mut candidates = Vec::new();
  push_existing(&mut candidates, &base.with_extension("rs"));
  push_existing(&mut candidates, &base.join("mod.rs"));
  candidates
}

fn push_existing(candidates: &mut Vec<PathBuf>, path: &Path) {
  if path.is_file() {
    let clean = lexical_clean(path);
    if !candidates.iter().any(|p| p == &clean) {
      candidates.push(clean);
    }
  }
}

fn lexical_clean(path: &Path) -> PathBuf {
  let mut out = PathBuf::new();
  for component in path.components() {
    match component {
      std::path::Component::CurDir => {}
      std::path::Component::ParentDir => {
        out.pop();
      }
      _ => out.push(component.as_os_str()),
    }
  }
  out
}

fn import_hit(symbol: &str, kind: &str, source: &Path, idx: usize, line: &str) -> SymbolHit {
  SymbolHit {
    name: symbol.to_string(),
    kind: kind.to_string(),
    lang: source
      .extension()
      .and_then(|ext| ext.to_str())
      .unwrap_or("")
      .to_string(),
    file_path: source.to_string_lossy().into_owned(),
    line_start: (idx + 1) as u32,
    content: line.to_string(),
    ..Default::default()
  }
}

fn has_meaningful_payload(env: &ContextResult) -> bool {
  env.definition.is_some()
    || !env.callers.is_empty()
    || !env.callees.is_empty()
    || !env.imports.is_empty()
    || !env.recent_history.is_empty()
    || !env.todos_inline.is_empty()
    || !env.file_outline.is_empty()
    || !env.usages.is_empty()
}

fn is_function_kind(kind: &str) -> bool {
  let k = kind;
  k.contains("function")
    || k.contains("method")
    || k == "function_item"
    || k == "method_declaration"
    || k == "method_definition"
    || k.starts_with("function_")
    || k.starts_with("method_")
}

/// Rough-cut imports extractor: scan the def file's first ~50 lines
/// for use/import/require/include. Avoids re-running the full
/// imports sensor (which is name-keyed, not file-keyed).
fn file_imports(def: &SymbolHit) -> Vec<SymbolHit> {
  use std::fs;
  let content = match fs::read_to_string(&def.file_path) {
    Ok(c) => c,
    Err(_) => return Vec::new(),
  };
  let mut hits: Vec<SymbolHit> = Vec::new();
  for (i, line) in content.lines().enumerate().take(80) {
    let l = line.trim_start();
    let starts = l.starts_with("use ")
      || l.starts_with("import ")
      || l.starts_with("from ")
      || l.starts_with("require ")
      || l.starts_with("require(")
      || l.starts_with("#include ")
      || l.starts_with("require_relative ")
      || l.starts_with("source ")
      || l.starts_with(". ");
    if starts {
      hits.push(SymbolHit {
        name: l
          .split(|c: char| c == ' ' || c == '\t' || c == ';' || c == '(' || c == '"' || c == '\'')
          .filter(|s| !s.is_empty())
          .nth(1)
          .unwrap_or("")
          .to_string(),
        kind: "import".to_string(),
        lang: def.lang.clone(),
        file_path: def.file_path.clone(),
        line_start: (i + 1) as u32,
        content: line.to_string(),
        ..Default::default()
      });
    }
  }
  hits
}

fn inline_todos(def: &SymbolHit) -> Vec<TodoEntry> {
  let re = regex::Regex::new(r"\b(TODO|FIXME|XXX|HACK)\b[:(]?\s*(.*)$").unwrap();
  let mut out = Vec::new();
  for (i, line) in def.content.lines().enumerate() {
    if let Some(cap) = re.captures(line) {
      out.push(TodoEntry {
        commit_hash: String::new(),
        author: String::new(),
        timestamp: String::new(),
        file_path: def.file_path.clone(),
        line_number: (def.line_start + i as u32),
        marker: cap.get(1).map(|m| m.as_str()).unwrap_or("TODO").to_string(),
        text: cap
          .get(2)
          .map(|m| m.as_str().trim())
          .unwrap_or("")
          .to_string(),
      });
    }
  }
  out
}

/// File outline — re-use the search sensor with --outline. Restrict
/// the walk to a single file by passing it as the root path.
fn file_outline(def: &SymbolHit, _opts: &WalkOpts) -> Result<Vec<SymbolHit>> {
  let local_opts = WalkOpts::default();
  let (hits, _diags) = crate::sensors::search_symbol_hits(
    Path::new(&def.file_path),
    true,
    crate::sensors::SearchConstraints::default(),
    &local_opts,
  )?;
  Ok(
    hits
      .into_iter()
      .map(|mut h| {
        h.content = crate::index::create_skeleton_from_text(&h.content);
        h
      })
      .collect(),
  )
}
