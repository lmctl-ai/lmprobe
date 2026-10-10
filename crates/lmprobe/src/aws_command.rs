//! JSON-first, allowlisted AWS CLI queries. No arbitrary command/flag passthrough.
use crate::aws_policy::{lookup, pagination, OperationSpec, OPERATIONS};
use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Args, Debug)]
pub struct AwsArgs {
  #[command(subcommand)]
  pub command: AwsCommand,
}

#[derive(Subcommand, Debug)]
pub enum AwsCommand {
  /// List the exact supported read-only operations; makes no AWS calls.
  #[command(after_help = "Example:\n  lmprobe aws catalog --json")]
  Catalog,
  /// Query an allowlisted AWS API using JSON parameters or a request file.
  #[command(
    after_help = "Examples:\n  lmprobe aws query sts get-caller-identity --json\n  lmprobe aws query ec2 describe-instances --params '{\"Filters\":[{\"Name\":\"instance-state-name\",\"Values\":[\"running\"]}]}' --region us-east-1 --json\n  lmprobe aws query --request-file request.json --outfile result.json\n\nRead-only operations only. Paginated calls default to 100 items; continuation and incomplete coverage remain visible. --select is a JSON Pointer applied after coverage checks. --infile remains GraphQL."
  )]
  Query(Box<QueryArgs>),
}

#[derive(Args, Debug)]
pub struct QueryArgs {
  /// AWS service name from the catalog (ec2, s3api, iam, etc.).
  #[arg(
    required_unless_present = "request_file",
    conflicts_with = "request_file",
    requires = "operation"
  )]
  pub service: Option<String>,
  /// Exact AWS CLI operation name, such as describe-instances.
  #[arg(
    required_unless_present = "request_file",
    conflicts_with = "request_file",
    requires = "service"
  )]
  pub operation: Option<String>,
  /// AWS API parameters as a JSON object using AWS field names.
  #[arg(long, conflicts_with = "request_file")]
  pub params: Option<String>,
  /// Versioned JSON request; - reads stdin. Cannot mix with query flags.
  #[arg(long)]
  pub request_file: Option<PathBuf>,
  /// Existing AWS CLI profile; no credentials are supplied through this wrapper.
  #[arg(long, conflicts_with = "request_file")]
  pub profile: Option<String>,
  /// Override the configured AWS region.
  #[arg(long, conflicts_with = "request_file")]
  pub region: Option<String>,
  /// Result cap for paginated operations (default 100; maximum 10000).
  #[arg(long, conflicts_with = "request_file")]
  pub max_items: Option<u32>,
  /// Native service page size; supported only for paginated operations.
  #[arg(long, conflicts_with = "request_file")]
  pub page_size: Option<u32>,
  /// Opaque AWS CLI token returned by an earlier capped query.
  #[arg(long, conflicts_with = "request_file")]
  pub starting_token: Option<String>,
  /// Fetch only the first service page; cannot mix with pagination controls.
  #[arg(long, conflicts_with_all=["request_file","max_items","page_size","starting_token"])]
  pub first_page: bool,
  /// Select a response subtree using JSON Pointer, e.g. /Reservations/0/Instances.
  #[arg(long, conflicts_with = "request_file")]
  pub select: Option<String>,
  /// Overall child-process deadline in seconds (default 60; maximum 300).
  #[arg(long, conflicts_with = "request_file")]
  pub timeout_seconds: Option<u64>,
}

fn request_schema() -> String {
  "lmprobe.aws.request.v1".into()
}
fn empty_parameters() -> Value {
  json!({})
}
fn default_timeout() -> u64 {
  60
}
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct AwsRequest {
  #[serde(default = "request_schema")]
  schema: String,
  service: String,
  operation: String,
  #[serde(default = "empty_parameters")]
  parameters: Value,
  profile: Option<String>,
  region: Option<String>,
  #[serde(default)]
  pagination: Pagination,
  select: Option<String>,
  #[serde(default = "default_timeout")]
  timeout_seconds: u64,
}
#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
struct Pagination {
  max_items: Option<u32>,
  page_size: Option<u32>,
  starting_token: Option<String>,
  #[serde(default)]
  first_page: bool,
}

impl QueryArgs {
  fn request(&self) -> Result<AwsRequest> {
    if let Some(path) = &self.request_file {
      let source = if path == Path::new("-") {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text)?;
        text
      } else {
        fs::read_to_string(path)?
      };
      return serde_json::from_str(&source)
        .context("AWS_REQUEST_INVALID: expected a versioned JSON request");
    }
    Ok(AwsRequest {
      schema: request_schema(),
      service: self.service.clone().context("missing service")?,
      operation: self.operation.clone().context("missing operation")?,
      parameters: self
        .params
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .context("AWS_PARAMETERS_INVALID: expected JSON object")?
        .unwrap_or_else(empty_parameters),
      profile: self.profile.clone(),
      region: self.region.clone(),
      pagination: Pagination {
        max_items: self.max_items,
        page_size: self.page_size,
        starting_token: self.starting_token.clone(),
        first_page: self.first_page,
      },
      select: self.select.clone(),
      timeout_seconds: self.timeout_seconds.unwrap_or(60),
    })
  }
}

impl AwsRequest {
  fn validate(&self) -> Result<&'static OperationSpec> {
    if self.schema != "lmprobe.aws.request.v1" {
      bail!("AWS_REQUEST_SCHEMA: unsupported request version");
    }
    let spec = lookup(&self.service, &self.operation)
      .context("AWS_OPERATION_REFUSED: operation is not in the read-only catalog")?;
    if spec.paginated && pagination(spec).is_none() {
      bail!("AWS_PAGINATION_UNSUPPORTED: no reviewed paginator model");
    }
    if !self.parameters.is_object() {
      bail!("AWS_PARAMETERS_INVALID: parameters must be an object");
    }
    if self.timeout_seconds == 0 || self.timeout_seconds > 300 {
      bail!("AWS_TIMEOUT_INVALID: timeout_seconds must be 1..=300");
    }
    for value in [&self.profile, &self.region, &self.pagination.starting_token]
      .into_iter()
      .flatten()
    {
      if value.is_empty() || value.contains('\0') {
        bail!("AWS_PARAMETER_INVALID: empty or NUL-containing option value");
      }
    }
    let page = &self.pagination;
    if page
      .max_items
      .is_some_and(|value| value == 0 || value > 10000)
      || page
        .page_size
        .is_some_and(|value| value == 0 || value > 10000)
    {
      bail!("AWS_PAGINATION_INVALID: item and page sizes must be 1..=10000");
    }
    let controls =
      page.max_items.is_some() || page.page_size.is_some() || page.starting_token.is_some();
    if !spec.paginated && (controls || page.first_page) {
      bail!("AWS_PAGINATION_UNSUPPORTED: operation is not paginated");
    }
    if page.first_page && controls {
      bail!("AWS_PAGINATION_INVALID: first_page cannot combine with pagination controls");
    }
    if let Some(pointer) = &self.select {
      if !pointer.is_empty() && !pointer.starts_with('/') {
        bail!("AWS_SELECT_INVALID: select must be a JSON Pointer");
      }
      let mut chars = pointer.chars();
      while let Some(c) = chars.next() {
        if c == '~' && !matches!(chars.next(), Some('0' | '1')) {
          bail!("AWS_SELECT_INVALID: invalid JSON Pointer escape");
        }
      }
    }
    Ok(spec)
  }
  fn argv(&self, spec: &OperationSpec) -> Vec<String> {
    let mut args = vec![
      self.service.clone(),
      self.operation.clone(),
      "--output=json".into(),
      "--no-cli-pager".into(),
      "--no-cli-auto-prompt".into(),
      "--color=off".into(),
      "--cli-connect-timeout=10".into(),
      "--cli-read-timeout=30".into(),
      format!("--cli-input-json={}", self.parameters),
    ];
    if let Some(profile) = &self.profile {
      args.push(format!("--profile={profile}"));
    }
    if let Some(region) = &self.region {
      args.push(format!("--region={region}"));
    }
    if spec.paginated {
      if self.pagination.first_page {
        args.push("--no-paginate".into());
      } else {
        args.push(format!(
          "--max-items={}",
          self.pagination.max_items.unwrap_or(100)
        ));
        if let Some(size) = self.pagination.page_size {
          args.push(format!("--page-size={size}"));
        }
        if let Some(token) = &self.pagination.starting_token {
          args.push(format!("--starting-token={token}"));
        }
      }
    }
    args
  }
}

pub fn input_paths(args: &AwsArgs) -> Vec<PathBuf> {
  let mut paths = Vec::new();
  if let AwsCommand::Query(query) = &args.command {
    paths.extend(
      query
        .request_file
        .iter()
        .filter(|path| path.as_path() != Path::new("-"))
        .cloned(),
    );
  }
  paths
}
pub fn protect_configuration() -> Result<()> {
  if crate::program_output::target().is_none() {
    return Ok(());
  }
  let result = (|| -> Result<()> {
    let home = std::env::var_os("HOME")
      .or_else(|| std::env::var_os("USERPROFILE"))
      .map(PathBuf::from)
      .context("AWS_CONFIG_RESOLUTION_FAILED: cannot resolve home directory")?;
    let config = auth_override_path("AWS_CONFIG_FILE", home.join(".aws/config"), &home)?;
    let credentials = auth_override_path(
      "AWS_SHARED_CREDENTIALS_FILE",
      home.join(".aws/credentials"),
      &home,
    )?;
    let mut paths = vec![
      config.clone(),
      credentials.clone(),
      home.join(".boto"),
      PathBuf::from("/etc/boto.cfg"),
    ];
    for name in [
      "AWS_WEB_IDENTITY_TOKEN_FILE",
      "AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE",
      "AWS_CREDENTIAL_FILE",
      "BOTO_CONFIG",
      "AWS_CA_BUNDLE",
    ] {
      if let Some(value) = std::env::var_os(name) {
        let text = value
          .to_str()
          .context("AWS_CONFIG_RESOLUTION_FAILED: non-UTF-8 authentication path")?;
        paths.push(PathBuf::from(text));
        paths.push(expand_auth_path(text, &home)?);
      }
    }
    // Protect known filenames before reading profile metadata, so malformed
    // config can never be overwritten by its own resolution error.
    crate::program_output::protect(paths).map_err(anyhow::Error::msg)?;
    for input in [&config, &credentials] {
      match fs::read_to_string(input) {
        Ok(text) => {
          let mut continued_auth_key = false;
          let mut previous_indent = 0;
          for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with(['#', ';']) {
              continue;
            }
            let indent = line.len() - line.trim_start().len();
            if continued_auth_key && indent > previous_indent {
              bail!("AWS_CONFIG_RESOLUTION_FAILED: multiline authentication path is unsupported");
            }
            continued_auth_key = false;
            if let Some(index) = trimmed.find(['=', ':']) {
              let key = trimmed[..index].trim();
              let value = trimmed[index + 1..].trim();
              if key.eq_ignore_ascii_case("web_identity_token_file")
                || key.eq_ignore_ascii_case("ca_bundle")
              {
                let raw = PathBuf::from(value);
                let expanded = expand_auth_path(value, &home)?;
                crate::program_output::protect([raw, expanded]).map_err(anyhow::Error::msg)?;
                continued_auth_key = true;
                previous_indent = indent;
              }
            }
          }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
      }
    }
    let root = home.join(".aws");
    let root = root
      .canonicalize()
      .or_else(|_| home.canonicalize().map(|home| home.join(".aws")))?;
    if crate::program_output::target().is_some_and(|target| target.starts_with(root)) {
      bail!("OUTFILE_SOURCE_CONFLICT: AWS outfile overlaps configuration or credential cache");
    }
    Ok(())
  })();
  if result.is_err() {
    crate::program_output::cancel_file();
  }
  result
}

fn expand_auth_path(input: &str, home: &Path) -> Result<PathBuf> {
  if input.is_empty() || input.contains('\0') {
    bail!("AWS_CONFIG_RESOLUTION_FAILED: invalid authentication path");
  }
  let variable = regex::Regex::new(r"\$\{([^}]+)\}|\$([A-Za-z_][A-Za-z0-9_]*)").unwrap();
  for capture in variable.captures_iter(input) {
    let name = capture.get(1).or_else(|| capture.get(2)).unwrap().as_str();
    if std::env::var_os(name).is_some_and(|value| value.to_str().is_none()) {
      bail!("AWS_CONFIG_RESOLUTION_FAILED: non-UTF-8 expanded authentication path");
    }
  }
  let expanded = variable.replace_all(input, |captures: &regex::Captures<'_>| {
    let name = captures
      .get(1)
      .or_else(|| captures.get(2))
      .unwrap()
      .as_str();
    std::env::var(name).unwrap_or_else(|_| captures[0].to_string())
  });
  let path = if expanded == "~" {
    home.to_path_buf()
  } else if let Some(tail) = expanded
    .strip_prefix("~/")
    .or_else(|| expanded.strip_prefix("~\\"))
  {
    home.join(tail)
  } else if expanded.starts_with('~') {
    bail!("AWS_CONFIG_RESOLUTION_FAILED: other-user home expansion is unsupported");
  } else {
    PathBuf::from(expanded.as_ref())
  };
  Ok(path)
}

fn auth_override_path(name: &str, default: PathBuf, home: &Path) -> Result<PathBuf> {
  match std::env::var_os(name) {
    None => Ok(default),
    Some(value) => expand_auth_path(
      value
        .to_str()
        .context("AWS_CONFIG_RESOLUTION_FAILED: non-UTF-8 authentication path")?,
      home,
    ),
  }
}
fn warning(code: &str, message: impl ToString) -> Value {
  json!({"code":code,"message":message.to_string(),"hint":null,"lang":null})
}

pub fn run(args: &AwsArgs) -> Result<Value> {
  protect_configuration()?;
  crate::program_output::protect(input_paths(args)).map_err(anyhow::Error::msg)?;
  if matches!(&args.command, AwsCommand::Catalog) {
    let rows:Vec<_>=OPERATIONS.iter().map(|spec|json!({"service":spec.service,"operation":spec.operation,"paginated":spec.paginated,"pagination":pagination(spec).map(|p|json!({"input_token":p.input_token,"output_token":p.output_token,"more_results":p.more_results}))})).collect();
    return Ok(
      json!({"schema":"lmprobe.aws_catalog.v1","exit_status":"matched","warnings":[],"rows":rows}),
    );
  }
  let AwsCommand::Query(query) = &args.command else {
    unreachable!()
  };
  let request = query.request()?;
  let spec = request.validate()?;
  let executable = std::env::var_os("LMPROBE_AWS").unwrap_or_else(|| "aws".into());
  let mut command = Command::new(executable);
  command
    .args(request.argv(spec))
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .env("AWS_PAGER", "")
    .env("AWS_CLI_AUTO_PROMPT", "off")
    .env("AWS_MAX_ATTEMPTS", "2");
  #[cfg(unix)]
  {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
  }
  let mut result = json!({"schema":"lmprobe.aws_query.v1","exit_status":"error","warnings":[],"data":null,"service":request.service,"operation":request.operation,"profile":request.profile,"region":request.region,"select":request.select,"complete":false,"partial":false,"page":{},"redactions":0});
  let mut child = match command.spawn() {
    Ok(child) => child,
    Err(error) => {
      result["warnings"] = json!([warning("AWS_CLI_UNAVAILABLE", error)]);
      return Ok(result);
    }
  };
  let stdout = capture(child.stdout.take().unwrap(), 16 * 1024 * 1024);
  let stderr = capture(child.stderr.take().unwrap(), 1024 * 1024);
  let deadline = Instant::now() + Duration::from_secs(request.timeout_seconds);
  let mut timed_out = false;
  let mut child_status = None;
  let status = loop {
    if child_status.is_none() {
      child_status = match child.try_wait() {
        Ok(status) => status,
        Err(error) => {
          stop_child(&mut child);
          return Err(error.into());
        }
      };
    }
    if let Some(status) = child_status.filter(|_| stdout.is_finished() && stderr.is_finished()) {
      break status;
    }
    if Instant::now() >= deadline {
      timed_out = true;
      stop_child(&mut child);
      break child.wait()?;
    }
    std::thread::sleep(Duration::from_millis(10));
  };
  result["aws_exit_code"] = json!(status.code());
  if timed_out {
    result["warnings"] = json!([warning(
      "AWS_QUERY_TIMEOUT",
      "AWS query exceeded the configured process deadline"
    )]);
    return Ok(result);
  }
  let (stdout, stdout_overflow) = stdout
    .join()
    .map_err(|_| anyhow::anyhow!("AWS_OUTPUT_CAPTURE_FAILED"))??;
  let (stderr, stderr_overflow) = stderr
    .join()
    .map_err(|_| anyhow::anyhow!("AWS_ERROR_CAPTURE_FAILED"))??;
  if stdout_overflow || stderr_overflow {
    result["warnings"] = json!([warning(
      "AWS_OUTPUT_TOO_LARGE",
      "AWS output exceeded capture limits"
    )]);
    return Ok(result);
  }
  if !status.success() {
    result["warnings"] = json!([warning(
      "AWS_CLI_ERROR",
      String::from_utf8_lossy(&stderr).trim()
    )]);
    return Ok(result);
  }
  let mut data: Value = match serde_json::from_slice(&stdout) {
    Ok(data) => data,
    Err(_) => {
      result["warnings"] = json!([warning(
        "AWS_JSON_INVALID",
        "AWS CLI did not return valid JSON"
      )]);
      return Ok(result);
    }
  };
  let mut warnings = Vec::new();
  let mut tokens = serde_json::Map::new();
  let native = pagination(spec);
  let cli_token = (!request.pagination.first_page && spec.paginated)
    .then(|| data.get("NextToken"))
    .flatten()
    .filter(|value| value.as_str().is_some_and(|s| !s.is_empty()))
    .cloned();
  let mut more_results = false;
  if request.pagination.first_page {
    if let Some(p) = native {
      let indicator = p
        .more_results
        .and_then(|key| data.get(key))
        .and_then(Value::as_bool);
      if let Some(value) = data
        .get(p.output_token)
        .filter(|value| value.as_str().is_some_and(|s| !s.is_empty()) && indicator != Some(false))
      {
        tokens.insert(p.output_token.into(), value.clone());
      }
      more_results = indicator == Some(true);
    }
  } else if let Some(value) = &cli_token {
    tokens.insert("NextToken".into(), value.clone());
  }
  let truncated = !tokens.is_empty() || more_results;
  let failures = data
    .get("failures")
    .or_else(|| data.get("Failures"))
    .and_then(Value::as_array)
    .is_some_and(|failures| !failures.is_empty());
  if truncated {
    warnings.push(warning(
      "AWS_PAGE_INCOMPLETE",
      "Result has continuation data; coverage is incomplete",
    ));
  }
  if failures {
    warnings.push(warning(
      "AWS_PARTIAL_FAILURE",
      "AWS returned per-resource failures alongside the result",
    ));
  }
  let redactions = redact(&mut data);
  result["aws_failures"] = data
    .get("failures")
    .or_else(|| data.get("Failures"))
    .cloned()
    .unwrap_or_else(|| json!([]));
  if redactions > 0 {
    warnings.push(warning(
      "AWS_VALUES_REDACTED",
      "Sensitive environment values were redacted",
    ));
  }
  result["page"] = json!({"tokens":tokens,"resume_token":cli_token,"native_input_key":native.map(|p|p.input_token),"native_output_key":native.map(|p|p.output_token),"first_page":request.pagination.first_page,"started_from_token":request.pagination.starting_token.is_some(),"max_items":if spec.paginated && !request.pagination.first_page {Some(request.pagination.max_items.unwrap_or(100))} else {None}});
  result["partial"] = json!(truncated || failures);
  result["complete"] = json!(!truncated && !failures);
  result["redactions"] = json!(redactions);
  if let Some(pointer) = &request.select {
    data = match data.pointer(pointer) {
      Some(value) => value.clone(),
      None => {
        warnings.push(warning(
          "AWS_SELECT_NOT_FOUND",
          "JSON Pointer does not exist in AWS response",
        ));
        result["warnings"] = json!(warnings);
        return Ok(result);
      }
    };
  }
  result["exit_status"] = json!(if data.as_array().is_some_and(Vec::is_empty) {
    "no_match"
  } else {
    "matched"
  });
  result["data"] = data;
  result["warnings"] = json!(warnings);
  Ok(result)
}

fn capture(
  reader: impl Read + Send + 'static,
  limit: usize,
) -> std::thread::JoinHandle<std::io::Result<(Vec<u8>, bool)>> {
  std::thread::spawn(move || {
    let mut reader = reader;
    let mut output = Vec::new();
    let mut overflow = false;
    let mut buffer = [0; 8192];
    loop {
      let count = reader.read(&mut buffer)?;
      if count == 0 {
        break;
      }
      let keep = count.min(limit.saturating_sub(output.len()));
      output.extend_from_slice(&buffer[..keep]);
      overflow |= keep < count;
    }
    Ok((output, overflow))
  })
}
fn stop_child(child: &mut std::process::Child) {
  #[cfg(unix)]
  {
    extern "C" {
      fn kill(pid: i32, signal: i32) -> i32;
    }
    if let Ok(pid) = i32::try_from(child.id()) {
      // The child is in its own process group; signal only that group, including
      // descendants holding capture pipes open. Never signal our process group.
      unsafe {
        kill(-pid, 9);
      }
    }
  }
  let _ = child.kill();
}
fn redact(data: &mut Value) -> usize {
  match data {
    Value::Object(object) => {
      let mut count = 0;
      for (key, value) in object.iter_mut() {
        if key.eq_ignore_ascii_case("environment") {
          if !value.is_null() {
            *value = json!("[REDACTED]");
            count += 1;
          }
        } else {
          count += redact(value);
        }
      }
      count
    }
    Value::Array(values) => values.iter_mut().map(redact).sum(),
    _ => 0,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  fn request(value: Value) -> AwsRequest {
    serde_json::from_value(value).unwrap()
  }
  #[test]
  fn defaults_and_parameter_payload_are_passed_without_shell_or_file_expansion() {
    let r = request(
      json!({"service":"ec2","operation":"describe-instances","parameters":{"Filters":[{"Name":"tag:Name","Values":["$(touch /tmp/no); file://secret"]}]}}),
    );
    let args = r.argv(r.validate().unwrap());
    assert_eq!(&args[..2], ["ec2", "describe-instances"]);
    assert!(args.iter().any(|arg| arg == "--max-items=100"));
    assert!(args
      .iter()
      .any(|arg| arg.starts_with("--cli-input-json={") && arg.contains("file://secret")));
    assert!(!args.iter().any(|arg| arg == "--query" || arg == "--debug"));
  }
  #[test]
  fn request_version_unknown_fields_and_mutations_are_refused() {
    assert!(serde_json::from_value::<AwsRequest>(
      json!({"service":"ec2","operation":"describe-instances","endpoint_url":"https://bad"})
    )
    .is_err());
    for value in [
      json!({"schema":"lmprobe.aws.request.v2","service":"ec2","operation":"describe-instances"}),
      json!({"service":"ec2","operation":"terminate-instances"}),
      json!({"service":"sts","operation":"assume-role"}),
      json!({"service":"ec2","operation":"describe-instances","parameters":[]}),
    ] {
      assert!(request(value).validate().is_err());
    }
  }
  #[test]
  fn pagination_rules_and_deadline_are_validated_before_spawning() {
    for extra in [
      json!({"max_items":0}),
      json!({"page_size":10001}),
      json!({"starting_token":""}),
      json!({"first_page":true,"max_items":1}),
    ] {
      assert!(request(
        json!({"service":"ec2","operation":"describe-instances","pagination":extra})
      )
      .validate()
      .is_err());
    }
    assert!(request(
      json!({"service":"sts","operation":"get-caller-identity","pagination":{"max_items":1}})
    )
    .validate()
    .is_err());
    for timeout in [0, 301] {
      assert!(request(
        json!({"service":"sts","operation":"get-caller-identity","timeout_seconds":timeout})
      )
      .validate()
      .is_err());
    }
  }
  #[test]
  fn option_values_cannot_become_new_cli_flags() {
    let r = request(
      json!({"service":"sts","operation":"get-caller-identity","profile":"--debug","region":"us-east-1 --endpoint-url=http://bad"}),
    );
    let args = r.argv(r.validate().unwrap());
    assert!(args.contains(&"--profile=--debug".into()));
    assert!(args.contains(&"--region=us-east-1 --endpoint-url=http://bad".into()));
    assert!(!args.contains(&"--debug".into()));
  }
  #[test]
  fn pointer_escaping_and_environment_redaction_preserve_metadata() {
    for pointer in ["not/pointer", "/~2bad", "/~"] {
      assert!(
        request(json!({"service":"sts","operation":"get-caller-identity","select":pointer}))
          .validate()
          .is_err()
      );
    }
    assert!(request(
      json!({"service":"sts","operation":"get-caller-identity","select":"/a~1b/~0key"})
    )
    .validate()
    .is_ok());
    let mut data = json!({"Functions":[{"FunctionName":"test","Environment":{"Variables":{"TOKEN":"secret"}}}],"nextToken":"resume"});
    assert_eq!(redact(&mut data), 1);
    assert_eq!(data["Functions"][0]["Environment"], "[REDACTED]");
    assert_eq!(data["nextToken"], "resume");
  }
  #[test]
  fn capture_drains_overflow_without_retaining_unbounded_output() {
    let (data, overflow) = capture(std::io::Cursor::new(vec![b'x'; 32768]), 1024)
      .join()
      .unwrap()
      .unwrap();
    assert_eq!(data.len(), 1024);
    assert!(overflow);
    let (data, overflow) = capture(std::io::Cursor::new(b"abc".to_vec()), 1024)
      .join()
      .unwrap()
      .unwrap();
    assert_eq!(data, b"abc");
    assert!(!overflow);
  }
}
