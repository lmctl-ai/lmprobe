//! Native lmcode host for lmprobe's operation library.
//!
//! The host callbacks receive JSON text, dispatch the existing lmprobe CLI operation,
//! and return its versioned result document as JSON text for `json_parse`.

mod child;
mod generate;
mod overlay;

use dslvm::stdlib::{failure_frame, InvocationResults, ResultContract};
use dslvm::{Context, Error, Program, Registry, Value};
use generate::{Binding, OpPlan, ParamSpec, ParamType, Slot, ValueKind};
use overlay::OverlayAccess;
use serde_json::{Map, Value as Json};
use std::io::{self, Write};
use std::rc::Rc;

pub const SCHEMA: &[u8] = include_bytes!("../../lmcode/host.schema.json");
const USAGE: &str = "usage: --version | --lmcode-schema [--json] | --lmcode-signatures | [--lmcode|--first|--last] FILE ...";

fn framed_failure(phase: &str, error: Error) -> i32 {
  let mut out = io::stdout().lock();
  failure_frame(&mut out, phase, &error).unwrap_or(4)
}

fn plans() -> Result<Vec<OpPlan>, Error> {
  let overlay = overlay::parse(overlay::OVERLAY_TOML)
    .map_err(|_| Error::host("invalid lmprobe operation catalog"))?;
  generate::plans(&crate::cli::command(), &overlay)
    .map_err(|_| Error::host("cannot generate lmprobe operation catalog"))
}

fn host_functions() -> Result<Registry, Error> {
  let mut registry = Registry::new();
  let schema: Json = serde_json::from_slice(SCHEMA)
    .map_err(|_| Error::host("invalid embedded lmprobe host schema"))?;
  let functions = schema
    .get("functions")
    .and_then(Json::as_object)
    .ok_or_else(|| Error::host("lmprobe host schema has no functions"))?;
  let plans = plans()?;
  let mut expected: Vec<String> = plans
    .iter()
    .map(|plan| format!("probe_{}", plan.name))
    .collect();
  expected.sort();
  let mut actual: Vec<String> = functions.keys().cloned().collect();
  actual.sort();
  if actual != expected {
    return Err(Error::host(
      "lmprobe operation catalog and lmcode schema differ",
    ));
  }
  for plan in plans {
    if plan.description.trim().is_empty() {
      return Err(Error::host(format!(
        "{} has no operation description",
        plan.name
      )));
    }
    let operation = plan.name.clone();
    let plan = Rc::new(plan);
    let run_plan = plan.clone();
    let signature_name = format!("probe_{operation}");
    registry.function_with_dry(
      &signature_name,
      &["string"],
      "string",
      move |_: &mut Context, args: &[Value]| {
        let input = args[0]
          .as_str()
          .map_err(|_| Error::new("CLI_USAGE", "operation parameters must be JSON text"))?;
        run_operation(&run_plan, input)
      },
      dslvm::dry(|_: &mut Context, _: &[Value]| Ok(Value::string("null"))),
    )?;
  }
  Ok(registry)
}

fn operation_error(code: &str, message: impl Into<String>) -> Error {
  Error::new(code, message)
}

fn param_matches(spec: &ParamSpec, value: &Json) -> bool {
  let scalar_matches = |kind: ParamType, value: &Json| match kind {
    ParamType::String => value.is_string(),
    ParamType::Number => value.as_number().is_some(),
    ParamType::Boolean => value.is_boolean(),
    ParamType::Array => false,
  };
  if spec.kind == ParamType::Array {
    let Some(values) = value.as_array() else {
      return false;
    };
    let item_kind = match spec.items {
      Some(generate::ItemType::String) => ParamType::String,
      Some(generate::ItemType::Number) => ParamType::Number,
      Some(generate::ItemType::Boolean) => ParamType::Boolean,
      None => return false,
    };
    values.iter().all(|value| {
      scalar_matches(item_kind, value)
        && spec
          .enum_values
          .as_ref()
          .is_none_or(|choices| choices.contains(value))
    })
  } else {
    scalar_matches(spec.kind, value)
      && spec
        .enum_values
        .as_ref()
        .is_none_or(|choices| choices.contains(value))
  }
}

fn parse_params(plan: &OpPlan, text: &str) -> Result<(Map<String, Json>, bool, bool), Error> {
  let value: Json = serde_json::from_str(text)
    .map_err(|_| operation_error("CLI_USAGE", "operation parameters must be a JSON object"))?;
  let mut params = value
    .as_object()
    .cloned()
    .ok_or_else(|| operation_error("CLI_USAGE", "operation parameters must be a JSON object"))?;
  let readonly = match params.remove("readonly") {
    None => false,
    Some(Json::Bool(value)) => value,
    Some(_) => return Err(operation_error("CLI_USAGE", "readonly must be boolean")),
  };
  let confirm = match params.remove("confirm") {
    None => false,
    Some(Json::Bool(value)) => value,
    Some(_) => return Err(operation_error("CLI_USAGE", "confirm must be boolean")),
  };
  if params.contains_key("dryrun") || params.contains_key("dry-run") {
    return Err(operation_error(
      "CLI_USAGE",
      "dry mode is invocation metadata; remove the legacy dryrun parameter",
    ));
  }
  for (key, value) in &params {
    let spec = plan
      .params
      .iter()
      .find(|spec| spec.name == *key)
      .ok_or_else(|| operation_error("CLI_USAGE", format!("unknown parameter {key}")))?;
    if !value.is_null() && !param_matches(spec, value) {
      return Err(operation_error(
        "CLI_USAGE",
        format!("parameter {key} has the wrong type or value"),
      ));
    }
  }
  for spec in &plan.params {
    if matches!(
      spec.occurs,
      generate::Occurs::One | generate::Occurs::OneOrMore
    ) && params.get(&spec.name).is_none_or(|value| value.is_null())
      && spec.default.is_none()
    {
      return Err(operation_error(
        "CLI_USAGE",
        format!("missing required parameter {}", spec.name),
      ));
    }
  }
  Ok((params, readonly, confirm))
}

fn json_display(value: &Json) -> String {
  match value {
    Json::String(value) => value.clone(),
    Json::Null => "null".into(),
    Json::Bool(value) => value.to_string(),
    Json::Number(value) => value.to_string(),
    Json::Array(_) | Json::Object(_) => serde_json::to_string(value).unwrap_or_default(),
  }
}

fn legacy_argv(plan: &OpPlan, params: &Map<String, Json>) -> Result<Vec<String>, Error> {
  let mut argv = Vec::new();
  argv
    .try_reserve(plan.words.len().saturating_add(plan.fixed_args.len()))
    .map_err(|_| Error::host("lmprobe argument allocation failed"))?;
  let mut positionals: Vec<(&Binding, Vec<String>)> = Vec::new();
  for (level, word) in plan.words.iter().enumerate() {
    argv.push(word.clone());
    for binding in plan
      .bindings
      .iter()
      .filter(|binding| binding.level == level)
    {
      let Some(value) = params.get(&binding.param) else {
        continue;
      };
      if value.is_null() || binding.default.as_ref() == Some(value) {
        continue;
      }
      let values: Vec<&Json> = match value {
        Json::Array(items) if binding.multi => items.iter().collect(),
        other => vec![other],
      };
      if matches!(binding.slot, Slot::Positional { .. }) {
        positionals.push((binding, values.into_iter().map(json_display).collect()));
        continue;
      }
      let flag = match &binding.slot {
        Slot::Long(long) => format!("--{long}"),
        Slot::Short(short) => format!("-{short}"),
        Slot::Positional { .. } => unreachable!(),
      };
      match binding.kind {
        ValueKind::Flag => {
          if value == &Json::Bool(true) {
            argv.push(flag);
          }
        }
        ValueKind::Count => {
          let count = value
            .as_u64()
            .ok_or_else(|| operation_error("CLI_USAGE", "count must be a nonnegative integer"))?;
          let count = usize::try_from(count).map_err(|_| {
            operation_error("CLI_USAGE", "count is not representable on this platform")
          })?;
          argv
            .try_reserve(count)
            .map_err(|_| Error::host("lmprobe argument allocation failed"))?;
          argv.extend(std::iter::repeat_n(flag, count));
        }
        ValueKind::Value => {
          for value in values {
            match binding.slot {
              Slot::Long(_) => argv.push(format!("{flag}={}", json_display(value))),
              _ => {
                argv.push(flag.clone());
                argv.push(json_display(value));
              }
            }
          }
        }
      }
    }
  }
  argv.extend(plan.fixed_args.iter().cloned());
  positionals.sort_by_key(|(binding, _)| match binding.slot {
    Slot::Positional { index, .. } => index,
    _ => 0,
  });
  if positionals.iter().any(|(binding, values)| {
    matches!(binding.slot, Slot::Positional { last: true, .. })
      || values.iter().any(|v| v.starts_with('-'))
  }) && !positionals.is_empty()
  {
    argv.push("--".into());
  }
  for (_, values) in positionals {
    argv.extend(values);
  }
  Ok(argv)
}

fn mentions_mutation(document: &str) -> bool {
  document
    .lines()
    .map(|line| line.split('#').next().unwrap_or_default())
    .any(|line| {
      line
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .any(|word| word == "mutation")
    })
}

fn run_operation(plan: &OpPlan, text: &str) -> Result<Value, Error> {
  let (params, readonly, confirm) = parse_params(plan, text)?;
  let dynamic_write = if plan.access == OverlayAccess::Dynamic {
    let query = params
      .get("query")
      .and_then(Json::as_str)
      .unwrap_or_default();
    let query = query
      .strip_prefix('@')
      .map(|path| std::fs::read_to_string(path).unwrap_or_default())
      .unwrap_or_else(|| query.to_string());
    mentions_mutation(&query)
  } else {
    false
  };
  let write = plan.access == OverlayAccess::Write || dynamic_write;
  let env_readonly = std::env::var("LMPROBE_READONLY").ok().is_some_and(|value| {
    matches!(
      value.trim().to_ascii_lowercase().as_str(),
      "1" | "true" | "yes" | "on"
    )
  });
  if write && (readonly || env_readonly) {
    return Err(operation_error(
      "FORBIDDEN",
      format!("{} is refused by read-only policy", plan.name),
    ));
  }
  if write && plan.dangerous && !confirm {
    return Err(operation_error(
      "CONFIRM_REQUIRED",
      format!("{} requires confirm=true", plan.name),
    ));
  }
  let argv = legacy_argv(plan, &params)?;
  let stdin = plan
    .stdin_param
    .as_ref()
    .and_then(|name| params.get(name))
    .map(|value| {
      let lines: Vec<String> = match value {
        Json::Array(values) => values.iter().map(json_display).collect(),
        other => vec![json_display(other)],
      };
      lines
        .iter()
        .map(|line| format!("{line}\n"))
        .collect::<String>()
    });
  let run = child::run_legacy(&argv, stdin).map_err(|error| match error {
    child::SpawnError::Input(message) => operation_error("CLI_USAGE", message),
    child::SpawnError::Internal(message) => operation_error("APP_FAIL", message),
  })?;
  let Some(result) = run.result else {
    let message = run
      .stderr
      .trim()
      .lines()
      .last()
      .unwrap_or("lmprobe operation produced no result");
    return Err(operation_error(
      "APP_FAIL",
      format!("{message} (legacy exit code {:?})", run.exit_code),
    ));
  };
  if result
    .get("status")
    .and_then(Json::as_str)
    .is_some_and(|status| matches!(status, "ok" | "no_match" | "partial"))
  {
    Ok(Value::string(
      serde_json::to_string(&result).unwrap_or_default(),
    ))
  } else {
    let diagnostics = result
      .get("diagnostics")
      .and_then(Json::as_array)
      .map(Vec::as_slice)
      .unwrap_or_default();
    let diag = diagnostics
      .iter()
      .find(|diag| diag.get("severity").and_then(Json::as_str) == Some("error"))
      .or_else(|| diagnostics.first());
    let code = diag
      .and_then(|diag| diag.get("code"))
      .and_then(Json::as_str)
      .unwrap_or("COMMAND_FAILED");
    let message = diag
      .and_then(|diag| diag.get("message"))
      .and_then(Json::as_str)
      .unwrap_or("lmprobe reported an error");
    Err(operation_error(code, message.to_string()))
  }
}

fn write_schema() -> Result<i32, Error> {
  io::stdout()
    .lock()
    .write_all(SCHEMA)
    .map_err(|_| Error::host("cannot write lmprobe lmcode schema"))?;
  Ok(0)
}

fn write_signatures() -> Result<i32, Error> {
  let registry = host_functions()?;
  let mut out = io::stdout().lock();
  for signature in registry.signatures() {
    writeln!(out, "{signature}").map_err(|_| Error::host("cannot write lmprobe signatures"))?;
  }
  Ok(0)
}

fn execute(args: &[String]) -> i32 {
  if args.len() == 1 && args[0] == "--lmcode-schema" {
    return write_schema().unwrap_or_else(|e| framed_failure("app", e));
  }
  if args.len() == 2 && args[0] == "--lmcode-schema" && args[1] == "--json" {
    return write_schema().unwrap_or_else(|e| framed_failure("app", e));
  }
  if args.len() == 1 && args[0] == "--lmcode-signatures" {
    return write_signatures().unwrap_or_else(|e| framed_failure("app", e));
  }

  let mut files = Vec::new();
  let mut priorities = Vec::new();
  let mut i = 0;
  while i < args.len() {
    if i + 1 >= args.len() {
      return framed_failure("cli", Error::new("CLI_USAGE", USAGE));
    }
    let priority = match args[i].as_str() {
      "--lmcode" => 1,
      "--first" => 0,
      "--last" => 2,
      _ => return framed_failure("cli", Error::new("CLI_USAGE", USAGE)),
    };
    files.push(args[i + 1].clone());
    priorities.push(priority);
    i += 2;
  }
  if files.is_empty() {
    return framed_failure("cli", Error::new("CLI_USAGE", USAGE));
  }

  let mut results = match InvocationResults::from_command(&files) {
    Ok(results) => results,
    Err(error) => return framed_failure("cli", error),
  };
  let registry = match host_functions() {
    Ok(registry) => registry,
    Err(error) => return framed_failure("app", error),
  };
  let contract = match ResultContract::from_schema(SCHEMA) {
    Ok(contract) => contract,
    Err(error) => return framed_failure("app", error),
  };
  let mut context = Context::new();
  let mut execution: Vec<usize> = (0..files.len()).collect();
  execution.sort_by_key(|index| priorities[*index]);

  for position in execution {
    let registration = results.registrations()[position].clone();
    let mut phase = "app";
    let result = (|| -> Result<(), Error> {
      let bytes = std::fs::read(&registration.file)
        .map_err(|_| Error::new("APP_FAIL", "cannot read lmprobe lmcode program"))?;
      phase = "load";
      let program = Program::load(&bytes, SCHEMA, &registry).map_err(|error| {
        let message = match error.code.as_str() {
          "LOAD_SCHEMA_MISMATCH" => "lmprobe lmcode schema mismatch",
          "LOAD_UNBOUND" => "lmprobe lmcode import mismatch",
          _ => "invalid lmprobe lmcode program",
        };
        Error::new(&error.code, message)
      })?;
      results.select(registration.index, &mut context, contract.clone())?;
      phase = "run";
      program.run(&mut context, &registry)?;
      results.complete(registration.index)
    })();
    if let Err(error) = result {
      let _ = results.fail(Some(registration.index), phase, error);
      break;
    }
  }
  results.write_frame(&mut io::stdout().lock())
}

/// Return Some(exit) only for an explicit host or host-inspection invocation.
pub fn maybe_run(raw_args: &[std::ffi::OsString]) -> Option<i32> {
  let first = raw_args.first()?.to_str()?;
  if ![
    "--lmcode",
    "--first",
    "--last",
    "--lmcode-schema",
    "--lmcode-signatures",
  ]
  .contains(&first)
  {
    return None;
  }
  let args: Option<Vec<String>> = raw_args
    .iter()
    .map(|arg| arg.to_str().map(str::to_owned))
    .collect();
  Some(match args {
    Some(args) => execute(&args),
    None => framed_failure(
      "cli",
      Error::new("CLI_USAGE", "lmcode arguments must be UTF-8"),
    ),
  })
}
