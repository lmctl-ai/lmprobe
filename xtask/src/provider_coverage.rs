use anyhow::{bail, Context, Result};
use std::collections::BTreeSet;
use std::fs;
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table};

const PROVIDER_DB: &str = "crates/lmprobe-secrets/providers/default.toml";
const DOC_TARGETS: &[&str] = &[
  "durable-memory/21_secrets.md",
  "crates/lmprobe-secrets/providers/CONTRIBUTING.md",
];
const START: &str = "<!-- provider-coverage:start -->";
const END: &str = "<!-- provider-coverage:end -->";

pub fn run(check: bool) -> Result<()> {
  let table = render_table()?;
  let mut drift = Vec::new();
  for target in DOC_TARGETS {
    let content = fs::read_to_string(target).with_context(|| format!("read {target}"))?;
    let updated = replace_between_markers(&content, &table)
      .with_context(|| format!("update provider coverage table in {target}"))?;
    if updated != content {
      if check {
        drift.push(*target);
      } else {
        fs::write(target, updated).with_context(|| format!("write {target}"))?;
      }
    }
  }
  if !drift.is_empty() {
    bail!(
      "provider coverage table is stale in: {}. Run `cargo xtask provider-coverage`.",
      drift.join(", ")
    );
  }
  if check {
    println!("✔ provider coverage tables are up to date");
  } else {
    println!("✔ provider coverage tables updated");
  }
  Ok(())
}

fn render_table() -> Result<String> {
  let content = fs::read_to_string(PROVIDER_DB).with_context(|| format!("read {PROVIDER_DB}"))?;
  let doc = content
    .parse::<DocumentMut>()
    .with_context(|| format!("parse {PROVIDER_DB}"))?;
  let providers = doc["providers"]
    .as_array_of_tables()
    .context("providers must be an array of tables")?;

  let mut out = String::new();
  out.push_str("| Provider | Languages | SDKs | Gitleaks rule IDs |\n");
  out.push_str("| --- | --- | --- | --- |\n");
  for provider in providers {
    let id = string_field(provider, "id")?;
    let rule_ids = string_array(provider.get("rule_ids"), "rule_ids")?;
    let languages = provider
      .get("languages")
      .and_then(Item::as_array_of_tables)
      .context("provider languages must be an array of tables")?;
    let language_list = language_list(languages)?;
    let sdk_list = sdk_list(languages)?;
    out.push_str(&format!(
      "| `{}` | {} | {} | {} |\n",
      escape_cell(id),
      language_list
        .iter()
        .map(|language| format!("`{}`", escape_cell(language)))
        .collect::<Vec<_>>()
        .join(", "),
      sdk_list
        .iter()
        .map(|sdk| format!("`{}`", escape_cell(sdk)))
        .collect::<Vec<_>>()
        .join(", "),
      rule_ids
        .iter()
        .map(|rule| format!("`{}`", escape_cell(rule)))
        .collect::<Vec<_>>()
        .join(", ")
    ));
  }
  Ok(out)
}

fn language_list(languages: &ArrayOfTables) -> Result<Vec<String>> {
  let mut set = BTreeSet::new();
  for language in languages {
    set.insert(string_field(language, "language")?.to_string());
  }
  Ok(set.into_iter().collect())
}

fn sdk_list(languages: &ArrayOfTables) -> Result<Vec<String>> {
  let mut sdks = Vec::new();
  for language in languages {
    let lang = string_field(language, "language")?;
    let sdk = string_field(language, "sdk")?;
    sdks.push(format!("{lang}/{sdk}"));
  }
  Ok(sdks)
}

fn string_field<'a>(table: &'a Table, key: &str) -> Result<&'a str> {
  table
    .get(key)
    .and_then(Item::as_str)
    .with_context(|| format!("{key} must be a string"))
}

fn string_array(item: Option<&Item>, key: &str) -> Result<Vec<String>> {
  item
    .and_then(Item::as_array)
    .with_context(|| format!("{key} must be an array"))?
    .iter()
    .map(|value| {
      value
        .as_str()
        .map(str::to_string)
        .with_context(|| format!("{key} values must be strings"))
    })
    .collect()
}

fn replace_between_markers(content: &str, table: &str) -> Result<String> {
  let start_idx = content
    .find(START)
    .with_context(|| format!("missing marker {START}"))?;
  let after_start = start_idx + START.len();
  let end_rel = content[after_start..]
    .find(END)
    .with_context(|| format!("missing marker {END}"))?;
  let end_idx = after_start + end_rel;
  let after_end = end_idx + END.len();
  Ok(format!(
    "{}\n\n{}\n\n{}{}",
    &content[..after_start],
    table.trim_end(),
    END,
    &content[after_end..]
  ))
}

fn escape_cell(value: &str) -> String {
  value.replace('|', "\\|")
}
