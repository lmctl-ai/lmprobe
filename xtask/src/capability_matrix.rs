use anyhow::{bail, Context, Result};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

const CLI_PATH: &str = "crates/lmprobe/src/cli.rs";
const SCHEMA_PATH: &str = "crates/lmprobe-types/src/schema.rs";
const OVERVIEW_PATH: &str = "durable-memory/01_overview.md";
const MATRIX_START: &str = "<!-- capability-matrix:start -->";
const MATRIX_END: &str = "<!-- capability-matrix:end -->";

#[derive(Clone)]
struct CommandProfile {
  command: String,
  gql_field: String,
  schemas: Vec<String>,
  outputs: String,
  language_scope: String,
  rw_class: String,
}

pub fn run(check: bool) -> Result<()> {
  let cli = fs::read_to_string(CLI_PATH).with_context(|| format!("read {CLI_PATH}"))?;
  let schema = fs::read_to_string(SCHEMA_PATH).with_context(|| format!("read {SCHEMA_PATH}"))?;
  let schema_by_key = parse_schema_map(&schema)?;

  let history_rows = parse_history_rows(&cli, &schema_by_key)?;
  let query_methods = extract_graphql_method_names(&cli, "Query")?;
  let mutation_methods = extract_graphql_method_names(&cli, "Mutation")?;

  let query_set = query_methods.into_iter().collect::<BTreeSet<_>>();
  let mutation_set = mutation_methods.into_iter().collect::<BTreeSet<_>>();

  let top_level_rows = parse_top_level_rows(
    &cli,
    &schema_by_key,
    &history_rows,
    &query_set,
    &mutation_set,
  )?;
  let mut all_rows = top_level_rows.clone();
  all_rows.append(&mut history_rows.clone());

  let matrix = render_matrix(&top_level_rows, &history_rows);

  let mut content =
    fs::read_to_string(OVERVIEW_PATH).with_context(|| format!("read {OVERVIEW_PATH}"))?;
  content = update_overview_counts(&content, &all_rows)?;
  let updated = replace_between_markers(&content, &matrix)?;

  if check {
    let source =
      fs::read_to_string(OVERVIEW_PATH).with_context(|| format!("read {OVERVIEW_PATH}"))?;
    let expected = replace_between_markers(&source, &matrix)?;
    if updated != expected {
      bail!("capability matrix in {OVERVIEW_PATH} is stale. Run: cargo xtask capability-matrix");
    }
    println!("✔ capability matrix is up to date");
    return Ok(());
  }

  fs::write(OVERVIEW_PATH, updated).with_context(|| format!("write {OVERVIEW_PATH}"))?;
  println!("✔ capability matrix updated in {OVERVIEW_PATH}");
  Ok(())
}

fn render_matrix(top_level: &[CommandProfile], history: &[CommandProfile]) -> String {
  let mut out = String::new();
  out
    .push_str("Generated from CLI dispatch, GraphQL schema, and envelope schema declarations.\n\n");
  out.push_str("| Command | GraphQL field | Envelope schema | Output formats | Language scope | Read/Write |\n");
  out.push_str("| --- | --- | --- | --- | --- | --- |\n");

  for row in top_level {
    out.push_str(&render_row(row));
  }
  for row in history {
    out.push_str(&render_row(row));
  }

  out
}

fn render_row(row: &CommandProfile) -> String {
  let schemas = if row.schemas.is_empty() {
    "N/A".to_string()
  } else {
    row.schemas.join("<br>")
  };
  format!(
    "| {} | {} | {} | {} | {} | {} |\n",
    escape_cell(&row.command),
    escape_cell(&row.gql_field),
    escape_cell(&schemas),
    escape_cell(&row.outputs),
    escape_cell(&row.language_scope),
    escape_cell(&row.rw_class)
  )
}

fn parse_top_level_rows(
  cli: &str,
  schema_by_key: &BTreeMap<String, String>,
  history_rows: &[CommandProfile],
  query_set: &BTreeSet<String>,
  mutation_set: &BTreeSet<String>,
) -> Result<Vec<CommandProfile>> {
  let block = extract_block_after_marker(cli, "match cli.command {")?;
  let arms = extract_variant_bodies(&block, "Commands::")?;
  let mut rows = Vec::new();
  for arm in arms {
    let command_key = to_kebab_case(&arm.0);
    let gql_field = resolve_gql_field(&command_key, query_set, mutation_set);
    let output_formats = infer_output_formats(&command_key, &arm.1);
    let language_scope = infer_language_scope(&command_key, &arm.1);

    let mut schemas = extract_and_map_schema_keys(&arm.1, schema_by_key);
    if schemas.is_empty() || matches!(command_key.as_str(), "git" | "aws") {
      schemas = fallback_schemas(&command_key, history_rows);
    }
    if schemas.is_empty() {
      schemas = vec!["N/A".to_string()];
    }

    let rw_class = if command_key == "serve" {
      "Interface".to_string()
    } else if command_key == "find" {
      "Read / Write (--action delete --force)".to_string()
    } else if matches!(command_key.as_str(), "delete" | "edit")
      || mutation_set.contains(&to_snake_case(&command_key))
    {
      "Write".to_string()
    } else {
      "Read".to_string()
    };

    rows.push(CommandProfile {
      command: format!("lmprobe {command_key}"),
      gql_field,
      schemas,
      outputs: output_formats,
      language_scope,
      rw_class,
    });
  }
  Ok(rows)
}

fn parse_history_rows(
  cli: &str,
  schema_by_key: &BTreeMap<String, String>,
) -> Result<Vec<CommandProfile>> {
  let block = extract_block_after_marker(cli, "fn dispatch_history(")?;
  let arms = extract_variant_bodies(&block, "HistoryCmd::")?;

  let mut rows = Vec::new();
  for arm in arms {
    let command_key = to_kebab_case(&arm.0);
    let command = format!("lmprobe history {command_key}");
    let gql_field = format!("history.{}", to_lower_camel_case(&command_key));
    let schemas = extract_and_map_schema_keys(&arm.1, schema_by_key);
    let outputs = infer_output_formats(&command_key, &arm.1);
    let language_scope = "Git history".to_string();
    rows.push(CommandProfile {
      command,
      gql_field,
      schemas,
      outputs,
      language_scope,
      rw_class: "Read".to_string(),
    });
  }

  Ok(rows)
}

fn fallback_schemas(command_key: &str, history_rows: &[CommandProfile]) -> Vec<String> {
  if command_key == "aws" {
    return vec![
      "lmprobe.aws_catalog.v1".into(),
      "lmprobe.aws_query.v1".into(),
    ];
  }
  if command_key == "query" {
    return vec!["lmprobe.query.v1".to_string()];
  }
  if command_key == "delete" {
    return vec!["lmprobe.delete.v1".to_string()];
  }
  if command_key == "edit" {
    return vec!["lmprobe.edit.v1".to_string()];
  }
  if command_key == "git" {
    return [
      "status",
      "diff",
      "log",
      "show",
      "branch",
      "remote",
      "rev_parse",
      "ls_files",
      "blame",
    ]
    .iter()
    .map(|verb| format!("lmprobe.git_{verb}.v1"))
    .collect();
  }
  if command_key == "serve" || command_key == "help" {
    return vec!["N/A".to_string()];
  }
  if command_key == "secrets-recipes" {
    return vec![
      "lmprobe.secrets_recipes.v1".to_string(),
      "lmprobe.secrets_recipes.show.v1".to_string(),
    ];
  }
  if command_key == "secrets-sources" {
    return vec![
      "lmprobe.secrets_sources.v1".to_string(),
      "lmprobe.secrets_sources.show.v1".to_string(),
    ];
  }
  if command_key == "history" {
    let mut out = BTreeSet::new();
    for row in history_rows {
      for schema in &row.schemas {
        out.insert(schema.clone());
      }
    }
    return out.into_iter().collect();
  }
  Vec::new()
}

fn extract_and_map_schema_keys(arm: &str, schema_by_key: &BTreeMap<String, String>) -> Vec<String> {
  let keys = extract_render_schema_keys(arm);
  map_schema_keys(&keys, schema_by_key)
}

fn resolve_gql_field(
  command_key: &str,
  query_set: &BTreeSet<String>,
  mutation_set: &BTreeSet<String>,
) -> String {
  if command_key == "serve" {
    return "N/A".to_string();
  }
  if command_key == "history" {
    return "history".to_string();
  }
  if command_key == "query" {
    return "custom query".to_string();
  }
  let snake = to_snake_case(command_key);
  if query_set.contains(&snake) || mutation_set.contains(&snake) {
    to_lower_camel_case(command_key)
  } else {
    "—".to_string()
  }
}

fn infer_output_formats(command_key: &str, arm: &str) -> String {
  if command_key == "query" {
    "JSON, NDJSON".to_string()
  } else if command_key == "serve" {
    "N/A (GraphQL HTTP service)".to_string()
  } else if arm.contains("render_by_format_with_graph(") {
    "Markdown, JSON, NDJSON, Sarif, Github, Mermaid, Dot".to_string()
  } else if arm.contains("render_by_format(") {
    "Markdown, JSON, NDJSON, Sarif, Github".to_string()
  } else if arm.contains("render_recipe_rows_table")
    || arm.contains("render_source_recipe_rows_table")
    || arm.contains("render_envelope_json")
  {
    "JSON, Markdown (for list output)".to_string()
  } else {
    "JSON".to_string()
  }
}

fn infer_language_scope(command_key: &str, arm: &str) -> String {
  if command_key == "aws" {
    "Read-only AWS APIs".into()
  } else if command_key == "edit" {
    "UTF-8 text files".to_string()
  } else if command_key == "git" {
    "Git repository records".to_string()
  } else if command_key == "find" || command_key == "delete" {
    "filesystem paths and metadata".to_string()
  } else if command_key == "query" {
    "in-process GraphQL".to_string()
  } else if command_key == "serve" {
    "GraphQL service".to_string()
  } else if command_key == "pr-triage" {
    "GitHub API".to_string()
  } else if command_key.starts_with("history") {
    "Git history".to_string()
  } else if command_key == "list-kinds" {
    "portable-kind taxonomy".to_string()
  } else if command_key.contains("secrets-recipes") || command_key.contains("secrets-sources") {
    "secret provider recipe catalog".to_string()
  } else if arm.contains("crate::api_facts") {
    "source code API facts".to_string()
  } else if arm.contains("fix_") || command_key == "fix" || command_key == "trace" {
    if command_key == "fix" || command_key == "trace" {
      "Git working tree (source mutation)".to_string()
    } else {
      "Git working tree".to_string()
    }
  } else if command_key == "config-keys"
    || command_key == "resource-sites"
    || command_key == "async-leaks"
    || command_key == "crypto-sites"
  {
    "multi-language source / AST scans".to_string()
  } else {
    "multi-language source / AST scans".to_string()
  }
}

fn update_overview_counts(content: &str, rows: &[CommandProfile]) -> Result<String> {
  let total = rows.len();
  let interface = rows
    .iter()
    .filter(|row| row.rw_class == "Interface")
    .count();
  let resultful = total.saturating_sub(interface);
  let interface_label = if interface == 1 {
    "1 interface command".to_string()
  } else {
    format!("{interface} interface commands")
  };
  let one_line_re =
    Regex::new(r"A one-shot CLI that exposes (?:27 verbs|\d+ commands) over a single binary\.")
      .context("compile overview count regex")?;
  let bullet_re = Regex::new(
    r"- (?:27 verbs|\d+ commands plus \d+ interface commands?) \(\[03_sensors_search\.md\]\(03_sensors_search\.md\), \[04_sensors_graph\.md\]\(04_sensors_graph\.md\), \[05_history\.md\]\(05_history\.md\), \[06_mutations\.md\]\(06_mutations\.md\)\)\.",
  )
  .context("compile overview bullet regex")?;
  let inventory_re = Regex::new(
    r"(?m)^(?:27 result-producing verbs \+ `serve` \(interface only\)|\d+ result-producing commands \+ `serve` .*)\. Grouped:$",
  )
  .context("compile inventory count regex")?;

  let mut out = one_line_re
    .replace(
      content,
      format!("A one-shot CLI that exposes {total} commands over a single binary."),
    )
    .into_owned();
  out = bullet_re
    .replace(
      &out,
      format!(
        "- {resultful} commands plus {interface_label} ([03_sensors_search.md](03_sensors_search.md), [04_sensors_graph.md](04_sensors_graph.md), [05_history.md](05_history.md), [06_mutations.md](06_mutations.md))."
      ),
    )
    .into_owned();
  out = inventory_re
    .replace(
      &out,
      format!("{resultful} result-producing commands + `serve` (interface only). Grouped:"),
    )
    .into_owned();
  out = out.replace(
    "| Group | Verbs | Chapter |",
    "| Command | Verbs | Chapter |",
  );
  Ok(out)
}

fn map_schema_keys(keys: &[String], schema_by_key: &BTreeMap<String, String>) -> Vec<String> {
  let mut out = BTreeSet::new();
  for key in keys {
    let mut candidate = key.replace('-', "_");
    if let Some(schema) = schema_by_key.get(&candidate) {
      out.insert(schema.clone());
      continue;
    }
    if !candidate.starts_with("lmprobe.") {
      candidate = format!("lmprobe.{candidate}.v1");
    }
    out.insert(candidate);
  }
  out.into_iter().collect()
}

fn parse_schema_map(schema: &str) -> Result<BTreeMap<String, String>> {
  let mut map = BTreeMap::new();
  let macro_re =
    Regex::new(r#"envelope(?:_with_scan_stats)?!\(\s*[A-Za-z][A-Za-z0-9_]*\s*,\s*"([^"]+)""#)
      .context("compile schema macro regex")?;
  let impl_re = Regex::new(r#"(?s)impl\s+([A-Za-z][A-Za-z0-9_]*)\s*\{.*?pub const SCHEMA:\s*&'static\s+str\s*=\s*"([^"]+)""#)
    .context("compile schema const regex")?;

  for cap in macro_re.captures_iter(schema) {
    let schema_name = &cap[1];
    if let Some((_, key)) = schema_name.split_once(".v") {
      if let Some(base) = schema_name.strip_prefix("lmprobe.") {
        map.insert(
          base[..base.len() - (key.len() + 1)].to_string(),
          schema_name.to_string(),
        );
      }
    }
  }

  for cap in impl_re.captures_iter(schema) {
    let schema_name = &cap[2];
    if let Some((_, key)) = schema_name.split_once(".v") {
      if let Some(base) = schema_name.strip_prefix("lmprobe.") {
        map.insert(
          base[..base.len() - (key.len() + 1)].to_string(),
          schema_name.to_string(),
        );
      }
    }
  }

  Ok(map)
}

fn extract_graphql_method_names(cli: &str, scope: &str) -> Result<Vec<String>> {
  let block = extract_impl_block(cli, scope)?;
  let re = Regex::new(r#"(?m)^\s*async fn\s+([a-z][a-z0-9_]*)\s*\("#)
    .context("compile gql method regex")?;
  let mut out = Vec::new();
  for cap in re.captures_iter(&block) {
    out.push(cap[1].to_string());
  }
  Ok(out)
}

fn extract_impl_block(cli: &str, scope: &str) -> Result<String> {
  extract_block_after_marker(cli, &format!("impl {scope}"))
}

fn extract_variant_bodies(block: &str, marker: &str) -> Result<Vec<(String, String)>> {
  let pattern = format!(
    r"(?m)^\s*{}([A-Za-z][A-Za-z0-9_]*)\s*(?:\{{|\([^)]*\)\s*=>\s*\{{)",
    regex::escape(marker)
  );
  let re = Regex::new(&pattern).context("compile variant regex")?;
  let mut out = Vec::new();
  let lines: Vec<&str> = block.lines().collect();
  let mut idx = 0;

  while idx < lines.len() {
    let line = lines[idx];
    if let Some(caps) = re.captures(line) {
      let variant = caps[1].to_string();
      let marker_text = format!("{marker}{variant}");
      let mut body = String::new();
      let mut brace_depth = 0i32;
      let mut seen = false;

      while idx < lines.len() {
        let line = lines[idx];
        let opens = line.matches('{').count() as i32;
        let closes = line.matches('}').count() as i32;

        if !seen && line.contains(&marker_text) {
          seen = true;
          brace_depth += opens;
          brace_depth -= closes;
          body.push_str(line);
          body.push('\n');
          if brace_depth <= 0 {
            break;
          }
          idx += 1;
          continue;
        }

        if seen {
          body.push_str(line);
          body.push('\n');
          brace_depth += opens;
          brace_depth -= closes;
          if brace_depth <= 0 {
            break;
          }
        }
        idx += 1;
      }

      out.push((variant, body));
      idx += 1;
      continue;
    }

    idx += 1;
  }

  if out.is_empty() {
    bail!("no matching {marker} arms found");
  }
  Ok(out)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn command_inventory_includes_tuple_variants_and_their_write_class() {
    let cli = include_str!("../../crates/lmprobe/src/cli.rs");
    let rows = parse_top_level_rows(
      cli,
      &BTreeMap::new(),
      &[],
      &BTreeSet::new(),
      &BTreeSet::new(),
    )
    .unwrap();
    let edit = rows
      .iter()
      .find(|row| row.command == "lmprobe edit")
      .expect("edit inventory");
    assert_eq!(edit.rw_class, "Write");
    assert_eq!(edit.schemas, ["lmprobe.edit.v1"]);
    let git = rows
      .iter()
      .find(|row| row.command == "lmprobe git")
      .expect("git inventory");
    assert_eq!(git.rw_class, "Read");
    assert_eq!(git.schemas.len(), 9);
    assert!(git
      .schemas
      .iter()
      .any(|schema| schema == "lmprobe.git_status.v1"));
  }
}

fn extract_render_schema_keys(block: &str) -> Vec<String> {
  let re =
    Regex::new(r#"render_by_format(?:_with_graph)?\s*\([^,]+,[^,]+,\s*"([A-Za-z][a-z0-9_-]*)""#)
      .expect("render schema regex is valid");
  let mut set = BTreeSet::new();
  for cap in re.captures_iter(block) {
    set.insert(cap[1].to_string());
  }
  set.into_iter().collect()
}

fn extract_block_after_marker(source: &str, marker: &str) -> Result<String> {
  let marker_idx = source
    .find(marker)
    .with_context(|| format!("marker '{marker}' not found"))?;
  let bytes = source.as_bytes();
  let mut brace_idx = None;

  for i in marker_idx..bytes.len() {
    if bytes[i] == b'{' {
      brace_idx = Some(i);
      break;
    }
  }

  let brace_idx = brace_idx.context("missing '{'")?;
  let mut depth = 0i32;
  let mut end = brace_idx;

  for i in brace_idx..bytes.len() {
    if bytes[i] == b'{' {
      depth += 1;
    } else if bytes[i] == b'}' {
      depth -= 1;
      if depth == 0 {
        end = i + 1;
        break;
      }
    }
  }

  if depth != 0 {
    bail!("unbalanced braces while parsing '{marker}' block");
  }

  Ok(source[brace_idx..end].to_string())
}

fn replace_between_markers(content: &str, replacement: &str) -> Result<String> {
  let start = content
    .find(MATRIX_START)
    .context("missing matrix start marker")?;
  let end = content
    .find(MATRIX_END)
    .context("missing matrix end marker")?;
  if end <= start {
    bail!("marker order invalid");
  }
  let suffix = content[end + MATRIX_END.len()..].trim_start_matches('\n');
  Ok(format!(
    "{}\n{}\n{}\n\n{}",
    &content[..=start + MATRIX_START.len() - 1],
    replacement,
    MATRIX_END,
    suffix,
  ))
}

fn to_kebab_case(value: &str) -> String {
  let mut out = String::new();
  for (idx, ch) in value.chars().enumerate() {
    if ch.is_ascii_uppercase() {
      if idx > 0 {
        out.push('-');
      }
      out.push(ch.to_ascii_lowercase());
    } else {
      out.push(ch);
    }
  }
  out
}

fn to_snake_case(value: &str) -> String {
  value.replace('-', "_")
}

fn to_lower_camel_case(value: &str) -> String {
  let mut parts = value.split('-');
  let mut out = parts.next().unwrap_or_default().to_string();
  for part in parts {
    let mut chars = part.chars();
    if let Some(first) = chars.next() {
      out.push(first.to_ascii_uppercase());
      out.extend(chars);
    }
  }
  out
}

fn escape_cell(value: &str) -> String {
  value.replace('|', "\\|")
}
