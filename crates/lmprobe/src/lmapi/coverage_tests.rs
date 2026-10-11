//! Per-operation coverage of the lmapi surface without spawning anything: for every published
//! operation, params generated from its published schema must resolve to the same params and the
//! same legacy argv in the flags form and the JSON form, and that argv must parse with lmprobe's
//! own clap parser.

use super::generate::{OpPlan, Slot, ValueKind};
use super::{legacy_argv, plans, spec};
use lmrust::{
  encode_argv, resolve_input, EncodeArgvOptions, Occurs, ParamSpec, ParamType, Record, ResolveArgs,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// The number of published operations; changing it is a contract change (update the docs table).
const PUBLISHED_OPS: usize = 63;

fn required(p: &ParamSpec) -> bool {
  matches!(p.occurs, None | Some(Occurs::One) | Some(Occurs::OneOrMore))
}

/// One sample element for `p` (`variant` picks a second distinct value for arrays).
fn sample(p: &ParamSpec, elem: ParamType, variant: usize) -> Value {
  if let Some(values) = p.enum_values.as_ref().filter(|v| !v.is_empty()) {
    return values[variant.min(values.len() - 1)].clone();
  }
  match elem {
    ParamType::Number => json!(3 + variant),
    ParamType::Boolean => json!(true),
    _ => json!(format!("s{variant}-{}", p.name.to_lowercase())),
  }
}

fn value_for(p: &ParamSpec) -> Value {
  if p.kind == ParamType::Array {
    let elem = match p.items.map(|i| i.as_param_type()) {
      Some(t) => t,
      None => ParamType::String,
    };
    json!([sample(p, elem, 0), sample(p, elem, 1)])
  } else {
    sample(p, p.kind, 0)
  }
}

/// Clap requirements the schema cannot express (`required_unless_present`): the published params
/// are optional, so a minimal call adds the first alternative. Without it the legacy parser
/// refuses the call (VALIDATION_ERROR at runtime, see tests/lmapi_ops.rs).
/// `edit` has a required ArgGroup (one edit operation).
const ONE_OF: [(&str, &[&str]); 4] = [
  ("aws_query", &["service", "operation"]),
  ("query_graphql", &["query"]),
  ("edit_preview", &["pattern", "replacement"]),
  ("edit_text", &["pattern", "replacement"]),
];

/// Params for `plan`: only the required ones, or every published param.
fn params_for(plan: &OpPlan, full: bool) -> Record {
  let alternative: &[&str] = ONE_OF
    .iter()
    .find(|(op, _)| *op == plan.name)
    .map(|(_, names)| *names)
    .unwrap_or(&[]);
  plan
    .schema
    .params
    .iter()
    .filter(|p| full || required(p) || alternative.contains(&p.name.as_str()))
    .map(|p| (p.name.clone(), value_for(p)))
    .collect()
}

/// The runner's globals (building the spec walks clap, so it is built once).
fn globals() -> &'static [ParamSpec] {
  static GLOBALS: std::sync::OnceLock<Vec<ParamSpec>> = std::sync::OnceLock::new();
  GLOBALS.get_or_init(|| spec().unwrap().global_specs())
}

fn resolve(plan: &OpPlan, argv: &[String], json: Option<&str>) -> Record {
  resolve_input(ResolveArgs {
    schema: &plan.schema,
    argv,
    globals: globals(),
    json_document: json,
    graphql_document: None,
  })
  .unwrap_or_else(|e| panic!("{}: {argv:?} / {json:?}: {e:?}", plan.name))
  .params
}

/// Legacy argv of `params` through the flags form (positional and named) and the JSON form.
fn argv_through_forms(plan: &OpPlan, params: &Record) -> Vec<String> {
  let as_json = serde_json::to_string(params).unwrap();
  let from_json = resolve(plan, &[], Some(&as_json));
  for positional in [true, false] {
    let flags = flags_form(plan, params, positional);
    let from_flags = resolve(plan, &flags, None);
    assert_eq!(
      from_flags, from_json,
      "{}: flags {flags:?} and JSON resolve differently",
      plan.name
    );
    assert_eq!(
      legacy_argv(plan, &from_flags),
      legacy_argv(plan, &from_json),
      "{}",
      plan.name
    );
  }
  legacy_argv(plan, &from_json)
}

/// The flags form of `params`: named flags, then (with `positional`) positional values after
/// `--` in schema order, as the runner assigns them.
fn flags_form(plan: &OpPlan, params: &Record, positional: bool) -> Vec<String> {
  let is_positional = |name: &str| {
    plan
      .schema
      .params
      .iter()
      .any(|p| p.name == name && p.positional == Some(true))
  };
  if !positional {
    return encode_argv(
      &plan.schema,
      params,
      EncodeArgvOptions { positional: false },
    );
  }
  let named: Record = params
    .iter()
    .filter(|(k, _)| !is_positional(k))
    .map(|(k, v)| (k.clone(), v.clone()))
    .collect();
  let mut argv = encode_argv(
    &plan.schema,
    &named,
    EncodeArgvOptions { positional: false },
  );
  let trailing: Vec<String> = plan
    .schema
    .params
    .iter()
    .filter(|p| p.positional == Some(true))
    .filter_map(|p| params.get(&p.name))
    .flat_map(|v| match v {
      Value::Array(items) => items.iter().map(lmrust::js::to_display).collect(),
      other => vec![lmrust::js::to_display(other)],
    })
    .collect();
  if !trailing.is_empty() {
    argv.push("--".into());
    argv.extend(trailing);
  }
  argv
}

fn clap_parses(argv: &[String]) -> Result<(), clap::Error> {
  crate::cli::command()
    .try_get_matches_from(
      std::iter::once("lmprobe".to_string())
        .chain(["--outfile".to_string(), "/dev/null".to_string()])
        .chain(argv.iter().cloned()),
    )
    .map(|_| ())
}

#[test]
fn every_operation_is_planned_once() {
  let plans = plans().unwrap();
  assert_eq!(plans.len(), PUBLISHED_OPS);
  let mut names: Vec<&str> = plans.iter().map(|p| p.name.as_str()).collect();
  names.sort_unstable();
  names.dedup();
  assert_eq!(names.len(), PUBLISHED_OPS, "duplicate op names");
}

#[test]
fn minimal_params_build_the_same_parseable_argv_in_every_form() {
  let mut failures = Vec::new();
  for plan in plans().unwrap() {
    let params = params_for(&plan, false);
    let argv = argv_through_forms(&plan, &params);
    assert_eq!(argv[..plan.words.len()], plan.words[..], "{}", plan.name);
    for fixed in &plan.fixed_args {
      assert!(argv.contains(fixed), "{}: {fixed} missing", plan.name);
    }
    if let Err(e) = clap_parses(&argv) {
      failures.push(format!("{}: {argv:?}\n{e}", plan.name));
    }
  }
  assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_published_param_builds_the_same_argv_in_every_form() {
  let mut rejected = BTreeMap::new();
  for plan in plans().unwrap() {
    let params = params_for(&plan, true);
    let argv = argv_through_forms(&plan, &params);
    // Every non-default value reaches the legacy command line in its slot's spelling.
    for b in &plan.bindings {
      let value = &params[&b.param];
      if b.default.as_ref() == Some(value) {
        continue;
      }
      let first = match value {
        Value::Array(items) => &items[0],
        other => other,
      };
      let text = lmrust::js::to_display(first);
      let found = match (&b.slot, &b.kind) {
        (Slot::Positional { .. }, _) => argv.contains(&text),
        (Slot::Long(long), ValueKind::Value) => argv.contains(&format!("--{long}={text}")),
        (Slot::Long(long), _) => argv.contains(&format!("--{long}")),
        (Slot::Short(c), ValueKind::Value) => argv
          .windows(2)
          .any(|w| w[0] == format!("-{c}") && w[1] == text),
        (Slot::Short(c), _) => argv.contains(&format!("-{c}")),
      };
      assert!(found, "{}: {} ({text}) not in {argv:?}", plan.name, b.param);
    }
    // Clap may refuse combinations it declares conflicting; every other refusal is a bug.
    if let Err(e) = clap_parses(&argv) {
      use clap::error::ErrorKind::*;
      assert!(
        matches!(e.kind(), ArgumentConflict | InvalidValue | ValueValidation),
        "{}: {argv:?}\n{e}",
        plan.name
      );
      rejected.insert(plan.name.clone(), e.kind());
    }
  }
  // Every refusal is expected: conflicting options published side by side (clap
  // `conflicts_with`), or a sample string a custom value parser rejects (find `--older-than`).
  // A new entry means the generator publishes something the legacy parser cannot take.
  use clap::error::ErrorKind::*;
  let expected: BTreeMap<String, clap::error::ErrorKind> = [
    ("aws_query", ArgumentConflict),
    ("context_bundle", ArgumentConflict),
    ("edit_preview", ArgumentConflict),
    ("edit_text", ArgumentConflict),
    ("find_delete", ValueValidation),
    ("find_paths", ValueValidation),
    ("impact_of", ArgumentConflict),
    ("query_graphql", ArgumentConflict),
    ("search_symbols", ArgumentConflict),
  ]
  .into_iter()
  .map(|(k, v)| (k.to_string(), v))
  .collect();
  assert_eq!(rejected, expected);
}

#[test]
fn no_two_operations_build_the_same_legacy_argv() {
  let mut seen: BTreeMap<Vec<String>, String> = BTreeMap::new();
  for plan in plans().unwrap() {
    let argv = legacy_argv(&plan, &params_for(&plan, false));
    if let Some(other) = seen.insert(argv.clone(), plan.name.clone()) {
      panic!("{other} and {} both run {argv:?}", plan.name);
    }
  }
}

#[test]
fn every_operation_documents_description_access_and_returns() {
  let doc = serde_json::to_value(super::document().unwrap()).unwrap();
  for op in doc["operations"].as_array().unwrap() {
    let name = &op["name"];
    assert!(
      op["description"]
        .as_str()
        .is_some_and(|d| !d.trim().is_empty()),
      "{name}"
    );
    assert!(
      ["read", "write", "dynamic"].contains(&op["access"].as_str().unwrap_or_default()),
      "{name}"
    );
    assert_eq!(op["returns"], "object", "{name}");
    for p in op["params"].as_array().unwrap() {
      assert!(p["name"].as_str().is_some_and(|n| !n.is_empty()), "{name}");
    }
  }
}

#[test]
fn the_document_is_deterministic_and_in_overlay_order() {
  let a = serde_json::to_string(&super::document().unwrap()).unwrap();
  let b = serde_json::to_string(&super::document().unwrap()).unwrap();
  assert_eq!(a, b);
  let overlay = super::overlay::parse(super::overlay::OVERLAY_TOML).unwrap();
  let doc: Value = serde_json::from_str(&a).unwrap();
  let names: Vec<&str> = doc["operations"]
    .as_array()
    .unwrap()
    .iter()
    .map(|o| o["name"].as_str().unwrap())
    .collect();
  let expected: Vec<&str> = overlay.ops.iter().map(|o| o.name.as_str()).collect();
  assert_eq!(names, expected);
}

/// Legacy commands published by more than one op: the variants must differ in what they run.
const VARIANTS: [(&str, [&str; 2]); 3] = [
  ("edit", ["edit_preview", "edit_text"]),
  ("fix", ["fix_preview", "fix_ast"]),
  ("find", ["find_paths", "find_delete"]),
];

#[test]
fn every_legacy_command_is_published_once_or_as_declared_variants() {
  let overlay = super::overlay::parse(super::overlay::OVERLAY_TOML).unwrap();
  let mut by_command: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
  for op in &overlay.ops {
    by_command
      .entry(op.command.as_str())
      .or_default()
      .push(op.name.as_str());
  }
  for (command, ops) in &by_command {
    if ops.len() == 1 {
      continue;
    }
    let declared = VARIANTS
      .iter()
      .find(|(c, _)| c == command)
      .unwrap_or_else(|| panic!("`{command}` is published by {ops:?} without a declared variant"));
    assert_eq!(ops[..], declared.1[..], "{command}");
  }
  let all = plans().unwrap();
  for (command, [a, b]) in VARIANTS {
    let pa = all.iter().find(|p| p.name == a).unwrap();
    let pb = all.iter().find(|p| p.name == b).unwrap();
    assert_ne!(pa.fixed_args, pb.fixed_args, "{command}");
    assert_ne!(
      pa.access, pb.access,
      "{command}: a preview and its write differ in access"
    );
  }
  // And the legacy tree is fully accounted for (published or excluded).
  assert!(super::generate::unmapped_commands(&crate::cli::command(), &overlay).is_empty());
}

#[test]
fn the_document_stays_within_its_size_budget() {
  let text = serde_json::to_string_pretty(&super::document().unwrap()).unwrap();
  assert!(
    text.len() < 256 * 1024,
    "the published document is {} bytes; trim common options before raising the budget",
    text.len()
  );
}

/// lmrust bug: `encode_argv` with `positional: true` emits positional values in the params
/// map's iteration order, while the runner assigns positionals in schema order; with sorted
/// JSON objects (`default-features = false`) `{"symbol", "path"}` becomes `-- <path> <symbol>`
/// and the values swap.
#[test]
#[ignore = "bug: lmrust encode_argv orders positionals by params-map key order, not schema order"]
fn lmrust_encode_argv_keeps_positional_schema_order() {
  let plan = plans()
    .unwrap()
    .into_iter()
    .find(|p| p.name == "impact_of")
    .unwrap();
  let params: Record = serde_json::from_value(json!({"symbol": "Sym", "path": "src"})).unwrap();
  let flags = encode_argv(
    &plan.schema,
    &params,
    EncodeArgvOptions { positional: true },
  );
  let resolved = resolve(&plan, &flags, None);
  assert_eq!(
    (&resolved["symbol"], &resolved["path"]),
    (&json!("Sym"), &json!("src")),
    "{flags:?}"
  );
}
