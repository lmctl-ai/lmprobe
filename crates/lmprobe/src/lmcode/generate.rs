//! Generate CLI bindings for the lmcode operation catalog from the clap command tree.
//!
//! For each overlay op the clap command at `command` is walked (after `Command::build`, so
//! global args are propagated to leaves): every visible argument becomes a parameter with its
//! long/short/aliases, occurrence (`1 ? + *` from required/num_args/action), type (from the value
//! parser: numbers, booleans, value enums), default and help text. The resulting
//! [`OpPlan`] also remembers how to turn validated params back into the legacy argv.

use super::overlay::{Overlay, OverlayAccess, OverlayOp};
use clap::builder::ArgAction;
use clap::{Arg, Command};
use serde_json::{Number, Value};
use std::any::TypeId;

/// How one published param maps back to the legacy command line.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Slot {
  /// `--long=value` (or `--long` for booleans).
  Long(String),
  /// `-s value` (or `-s` for booleans) when the arg has no long name.
  Short(char),
  /// A positional value; `last` positionals must follow `--`.
  Positional { index: usize, last: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ValueKind {
  Flag,
  Count,
  Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParamType {
  String,
  Number,
  Boolean,
  Array,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ItemType {
  String,
  Number,
  Boolean,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Occurs {
  One,
  Optional,
  OneOrMore,
  ZeroOrMore,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParamSpec {
  pub name: String,
  pub kind: ParamType,
  pub occurs: Occurs,
  pub items: Option<ItemType>,
  pub enum_values: Option<Vec<Value>>,
  pub positional: Option<bool>,
  pub aliases: Option<Vec<String>>,
  pub description: Option<String>,
  pub default: Option<Value>,
}
impl ParamSpec {
  fn new(name: String, kind: ParamType) -> Self {
    Self {
      name,
      kind,
      occurs: Occurs::One,
      items: None,
      enum_values: None,
      positional: None,
      aliases: None,
      description: None,
      default: None,
    }
  }
  fn occurs(mut self, occurs: Occurs) -> Self {
    self.occurs = occurs;
    self
  }
  fn items(mut self, items: ItemType) -> Self {
    self.items = Some(items);
    self
  }
  fn describe(mut self, description: String) -> Self {
    self.description = Some(description);
    self
  }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Binding {
  /// Published param name.
  pub param: String,
  /// Index into `OpPlan::words` of the command level the arg belongs to.
  pub level: usize,
  pub slot: Slot,
  pub kind: ValueKind,
  pub multi: bool,
  pub default: Option<Value>,
}

/// One published operation and how to run it through the legacy CLI.
#[derive(Debug, Clone)]
pub(crate) struct OpPlan {
  pub name: String,
  /// Legacy command words, e.g. `["git", "status"]`.
  pub words: Vec<String>,
  pub access: OverlayAccess,
  pub dangerous: bool,
  pub description: String,
  pub params: Vec<ParamSpec>,
  pub bindings: Vec<Binding>,
  pub fixed_args: Vec<String>,
  /// Param whose list value is fed on stdin, one entry per line.
  pub stdin_param: Option<String>,
}

/// kebab-case or snake_case to camelCase (`exit-zero-on-no-match` -> `exitZeroOnNoMatch`).
pub(crate) fn camel(name: &str) -> String {
  let mut out = String::with_capacity(name.len());
  let mut upper = false;
  for c in name.chars() {
    if c == '-' || c == '_' {
      upper = !out.is_empty();
    } else if upper {
      out.extend(c.to_uppercase());
      upper = false;
    } else {
      out.push(c);
    }
  }
  out
}

/// Every leaf command path of the tree (`["git", "status"]`).
#[cfg(test)]
pub(crate) fn leaf_paths(root: &Command) -> Vec<Vec<String>> {
  fn walk(cmd: &Command, prefix: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    let subs: Vec<&Command> = cmd.get_subcommands().collect();
    if subs.is_empty() {
      out.push(prefix.clone());
      return;
    }
    for sub in subs {
      prefix.push(sub.get_name().to_string());
      walk(sub, prefix, out);
      prefix.pop();
    }
  }
  let mut out = Vec::new();
  walk(root, &mut Vec::new(), &mut out);
  out
}

fn find_path<'a>(root: &'a Command, words: &[String]) -> Option<Vec<&'a Command>> {
  let mut chain = vec![root];
  for word in words {
    let next = chain
      .last()?
      .get_subcommands()
      .find(|c| c.get_name() == word)?;
    chain.push(next);
  }
  Some(chain)
}

fn is_type<T: 'static>(arg: &Arg) -> bool {
  arg.get_value_parser().type_id() == TypeId::of::<T>()
}

fn is_number(arg: &Arg) -> bool {
  is_type::<u8>(arg)
    || is_type::<u16>(arg)
    || is_type::<u32>(arg)
    || is_type::<u64>(arg)
    || is_type::<usize>(arg)
    || is_type::<i8>(arg)
    || is_type::<i16>(arg)
    || is_type::<i32>(arg)
    || is_type::<i64>(arg)
    || is_type::<isize>(arg)
    || is_type::<f32>(arg)
    || is_type::<f64>(arg)
}

fn convert(text: &str, kind: ParamType) -> Value {
  match kind {
    ParamType::Number => text
      .parse::<f64>()
      .ok()
      .and_then(Number::from_f64)
      .map(Value::Number)
      .unwrap_or_else(|| Value::String(text.to_string())),
    ParamType::Boolean => Value::Bool(text == "true"),
    ParamType::String | ParamType::Array => Value::String(text.to_string()),
  }
}

fn help_text(arg: &Arg) -> Option<String> {
  arg
    .get_help()
    .or_else(|| arg.get_long_help())
    .map(|h| {
      h.to_string()
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ")
    })
    .filter(|h| !h.trim().is_empty())
}

/// Build the param and binding for one clap arg; `None` for args that are never published.
fn param_for(arg: &Arg, level: usize) -> Option<(ParamSpec, Binding)> {
  let action = arg.get_action();
  let kind = match action {
    ArgAction::SetTrue | ArgAction::SetFalse => ValueKind::Flag,
    ArgAction::Count => ValueKind::Count,
    ArgAction::Set | ArgAction::Append => ValueKind::Value,
    _ => return None, // help, version
  };
  let positional = arg.is_positional();
  let name = match (positional, arg.get_long()) {
    (false, Some(long)) => camel(long),
    _ => camel(arg.get_id().as_str()),
  };
  let slot = if positional {
    Slot::Positional {
      index: arg.get_index().unwrap_or(0),
      last: arg.is_last_set(),
    }
  } else if let Some(long) = arg.get_long() {
    Slot::Long(long.to_string())
  } else {
    Slot::Short(arg.get_short()?)
  };
  let max_values = arg.get_num_args().map(|r| r.max_values()).unwrap_or(1);
  let multi = kind == ValueKind::Value && (matches!(action, ArgAction::Append) || max_values > 1);
  let enum_values: Vec<Value> = if kind == ValueKind::Value && !is_type::<bool>(arg) {
    arg
      .get_possible_values()
      .into_iter()
      .filter(|v| !v.is_hide_set())
      .map(|v| Value::String(v.get_name().to_string()))
      .collect()
  } else {
    Vec::new()
  };
  let elem = match kind {
    ValueKind::Flag => ParamType::Boolean,
    ValueKind::Count => ParamType::Number,
    ValueKind::Value if is_type::<bool>(arg) => ParamType::Boolean,
    ValueKind::Value if is_number(arg) => ParamType::Number,
    ValueKind::Value => ParamType::String,
  };
  let required = arg.is_required_set();
  let occurs = match (multi, required, &kind) {
    (_, _, ValueKind::Flag | ValueKind::Count) => Occurs::Optional,
    (true, true, _) => Occurs::OneOrMore,
    (true, false, _) => Occurs::ZeroOrMore,
    (false, true, _) => Occurs::One,
    (false, false, _) => Occurs::Optional,
  };
  let mut spec = ParamSpec::new(name.clone(), elem).occurs(occurs);
  if multi {
    spec.kind = ParamType::Array;
    spec.items = Some(match elem {
      ParamType::Number => ItemType::Number,
      ParamType::Boolean => ItemType::Boolean,
      _ => ItemType::String,
    });
  }
  if !enum_values.is_empty() {
    spec.enum_values = Some(enum_values);
  }
  if positional {
    spec.positional = Some(true);
  }
  let mut aliases: Vec<String> = Vec::new();
  if !positional {
    if let Some(short) = arg.get_short() {
      if arg.get_long().is_some() {
        aliases.push(short.to_string());
      }
    }
    for alias in arg.get_visible_aliases().unwrap_or_default() {
      aliases.push(alias.to_string());
    }
    for alias in arg.get_visible_short_aliases().unwrap_or_default() {
      aliases.push(alias.to_string());
    }
  }
  if !aliases.is_empty() {
    spec.aliases = Some(aliases);
  }
  spec.description = help_text(arg);
  let default = if kind == ValueKind::Value && !arg.get_default_values().is_empty() {
    let texts: Vec<String> = arg
      .get_default_values()
      .iter()
      .map(|v| v.to_string_lossy().into_owned())
      .collect();
    if multi {
      let delimiter = arg.get_value_delimiter();
      let items: Vec<Value> = texts
        .iter()
        .flat_map(|t| match delimiter {
          Some(d) => t.split(d).map(str::to_string).collect::<Vec<_>>(),
          None => vec![t.clone()],
        })
        .map(|t| convert(&t, elem))
        .collect();
      Some(Value::Array(items))
    } else {
      texts.first().map(|t| convert(t, elem))
    }
  } else {
    None
  };
  spec.default = default.clone();
  Some((
    spec,
    Binding {
      param: name,
      level,
      slot,
      kind,
      multi,
      default,
    },
  ))
}

/// Plans for every overlay op, in overlay order.
pub(crate) fn plans(root: &Command, overlay: &Overlay) -> Result<Vec<OpPlan>, String> {
  let mut root = root.clone();
  root.build();
  for (arg, ops) in &overlay.only_for {
    if let Some(name) = ops
      .iter()
      .find(|n| !overlay.ops.iter().any(|op| &op.name == *n))
    {
      return Err(format!("only_for.{arg}: unknown op {name}"));
    }
  }
  for op in &overlay.ops {
    let first = op.command.split_whitespace().next().unwrap_or_default();
    if overlay
      .excluded
      .iter()
      .any(|e| e == &op.command || e == first)
    {
      return Err(format!(
        "op {}: command `{}` is excluded",
        op.name, op.command
      ));
    }
  }
  overlay
    .ops
    .iter()
    .map(|op| plan(&root, overlay, op))
    .collect()
}

fn plan(root: &Command, overlay: &Overlay, op: &OverlayOp) -> Result<OpPlan, String> {
  let words: Vec<String> = op.command.split_whitespace().map(str::to_string).collect();
  let chain = find_path(root, &words)
    .ok_or_else(|| format!("op {}: no clap command `{}`", op.name, op.command))?;
  let leaf = *chain.last().expect("chain has the root");
  if leaf.get_subcommands().next().is_some() {
    return Err(format!(
      "op {}: `{}` is a command group, not a leaf",
      op.name, op.command
    ));
  }
  let root_globals: Vec<&str> = chain[0]
    .get_arguments()
    .map(|a| a.get_id().as_str())
    .collect();
  let excluded = |id: &str| {
    overlay
      .exclude_params
      .iter()
      .chain(&op.exclude_params)
      .any(|e| e == id)
      || (!op.common_options && root_globals.contains(&id))
      || overlay
        .only_for
        .get(id)
        .is_some_and(|ops| !ops.contains(&op.name))
  };
  let mut params: Vec<ParamSpec> = Vec::new();
  let mut bindings: Vec<Binding> = Vec::new();
  let mut fixed_args = op.fixed_args.clone();
  // chain[0] is the root (its args are global and reappear on the leaf).
  for (depth, cmd) in chain.iter().enumerate().skip(1) {
    let level = depth - 1;
    let is_leaf = depth == chain.len() - 1;
    let mut args: Vec<&Arg> = cmd.get_arguments().collect();
    // Positionals in index order after the options, as clap lists them.
    args.sort_by_key(|a| (a.is_positional(), a.get_index().unwrap_or(0)));
    for arg in args {
      if arg.is_hide_set() || excluded(arg.get_id().as_str()) || (arg.is_global_set() && !is_leaf) {
        continue;
      }
      // A required on/off flag is a marker the legacy command always needs (`callsites
      // --args`): a published boolean could only ever be `true`, so it is a fixed arg instead.
      if is_leaf && arg.is_required_set() && matches!(arg.get_action(), ArgAction::SetTrue) {
        if let Some(long) = arg.get_long() {
          fixed_args.push(format!("--{long}"));
          continue;
        }
      }
      let Some((spec, binding)) = param_for(arg, level) else {
        continue;
      };
      if params.iter().any(|p| p.name == spec.name) {
        continue; // a local arg shadows a propagated global of the same name
      }
      params.push(spec);
      bindings.push(binding);
    }
  }
  if let Some(stdin) = &op.stdin_param {
    params.push(
      ParamSpec::new(stdin.name.clone(), ParamType::Array)
        .items(ItemType::String)
        .occurs(Occurs::OneOrMore)
        .describe(stdin.description.clone()),
    );
  }
  let description = op
    .description
    .clone()
    .or_else(|| leaf.get_about().map(|a| a.to_string()))
    .unwrap_or_else(|| format!("lmprobe {}", op.command));
  Ok(OpPlan {
    name: op.name.clone(),
    words,
    access: op.access,
    dangerous: op.dangerous,
    description,
    params,
    bindings,
    fixed_args,
    stdin_param: op.stdin_param.as_ref().map(|s| s.name.clone()),
  })
}

/// Clap leaf commands neither published by an op nor listed in `excluded`.
#[cfg(test)]
pub(crate) fn unmapped_commands(root: &Command, overlay: &Overlay) -> Vec<String> {
  let mut root = root.clone();
  root.build();
  leaf_paths(&root)
    .into_iter()
    .map(|p| p.join(" "))
    .filter(|path| {
      let first = path.split_whitespace().next().unwrap_or_default();
      let is_help = path.split_whitespace().any(|w| w == "help");
      !is_help
        && !overlay.excluded.iter().any(|e| e == path || e == first)
        && !overlay.ops.iter().any(|op| &op.command == path)
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn shipped_overlay_maps_every_command_and_has_unique_operation_names() {
    let overlay = super::super::overlay::parse(super::super::overlay::OVERLAY_TOML).unwrap();
    let planned = plans(&crate::cli::command(), &overlay).unwrap();
    assert_eq!(planned.len(), 63);
    let mut names: Vec<_> = planned.iter().map(|p| p.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), planned.len());
    assert!(unmapped_commands(&crate::cli::command(), &overlay).is_empty());
  }

  #[test]
  fn required_and_default_arguments_are_retained_for_host_validation() {
    let overlay = super::super::overlay::parse(super::super::overlay::OVERLAY_TOML).unwrap();
    let planned = plans(&crate::cli::command(), &overlay).unwrap();
    let grep = planned
      .iter()
      .find(|plan| plan.name == "grep_text")
      .unwrap();
    assert!(grep
      .params
      .iter()
      .any(|p| p.name == "pattern" && p.occurs == Occurs::One));
    assert!(grep.params.iter().any(|p| p.name == "path"));
    let find = planned
      .iter()
      .find(|plan| plan.name == "find_paths")
      .unwrap();
    assert!(find
      .params
      .iter()
      .any(|p| p.name == "display" && p.default.is_some()));
  }
}
