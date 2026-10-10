use lmprobe_secrets::{
  analyze_source, analyze_sources_with_module_graph, FlowStepKind, SinkEvidenceMode,
};
use std::path::PathBuf;

fn confirmed_provider(path: &str, code: &str, provider: &str) {
  let result = analyze_source(path, code, SinkEvidenceMode::Off);
  assert!(
    result
      .confirmed_flows
      .iter()
      .any(|flow| flow.provider_id.as_deref() == Some(provider)),
    "expected confirmed {provider} flow:\n{result:#?}"
  );
}

fn assert_dotenv_source_only(path: &str, code: &str) {
  let result = analyze_source(path, code, SinkEvidenceMode::Off);
  assert!(
    result
      .source_evidence
      .iter()
      .any(|source| source.source_id == "dotenv-loader"),
    "expected dotenv-loader source evidence:\n{result:#?}"
  );
  assert!(
    result.confirmed_flows.is_empty(),
    "dotenv side-effect calls must not produce confirmed flows:\n{result:#?}"
  );
}

#[test]
fn python_dotenv_loader_emits_source_evidence_only() {
  assert_dotenv_source_only(
    "app.py",
    r#"
from dotenv import load_dotenv
load_dotenv()
"#,
  );
}

#[test]
fn javascript_dotenv_loader_emits_source_evidence_only() {
  assert_dotenv_source_only(
    "app.js",
    r#"
require("dotenv").config();
"#,
  );
}

#[test]
fn go_dotenv_loader_emits_source_evidence_only() {
  assert_dotenv_source_only(
    "app.go",
    r#"
package main
import "github.com/joho/godotenv"
func main() {
  godotenv.Load()
}
"#,
  );
}

#[test]
fn java_dotenv_loader_emits_source_evidence_only() {
  assert_dotenv_source_only(
    "App.java",
    r#"
import io.github.cdimascio.dotenv.Dotenv;
class App {
  void configure() {
    Dotenv.load();
  }
}
"#,
  );
}

#[test]
fn javascript_source_origin_reaches_provider_sink() {
  confirmed_provider(
    "app.js",
    r#"
const secret = process.env.STRIPE_API_KEY;
const stripeClient = require("stripe")(secret);
"#,
    "stripe",
  );
}

#[test]
fn go_source_origin_reaches_provider_sink() {
  confirmed_provider(
    "app.go",
    r#"
package main
import (
  "os"
  "github.com/aws/aws-sdk-go-v2/credentials"
)
func main() {
  id := os.Getenv("AWS_ACCESS_KEY_ID")
  secret := os.Getenv("AWS_SECRET_ACCESS_KEY")
  _ = credentials.NewStaticCredentialsProvider(id, secret, "")
}
"#,
    "aws",
  );
}

#[test]
fn go_grouped_var_source_bindings_reach_provider_sink() {
  confirmed_provider(
    "app.go",
    r#"
package main
import (
  "os"
  "github.com/aws/aws-sdk-go-v2/credentials"
)
func main() {
  var (
    id = os.Getenv("AWS_ACCESS_KEY_ID")
    secret = os.Getenv("AWS_SECRET_ACCESS_KEY")
  )
  _ = credentials.NewStaticCredentialsProvider(id, secret, "")
}
"#,
    "aws",
  );
}

#[test]
fn java_env_and_system_property_sources_reach_provider_sink() {
  let result = analyze_source(
    "AwsEnvApp.java",
    r#"
import com.amazonaws.auth.BasicAWSCredentials;
class AwsEnvApp {
  void configure() {
    String id = System.getProperty("AWS_ACCESS_KEY_ID");
    String secret = System.getenv("AWS_SECRET_ACCESS_KEY");
    new BasicAWSCredentials(id, secret);
  }
}
"#,
    SinkEvidenceMode::Off,
  );
  assert_eq!(
    result
      .confirmed_flows
      .iter()
      .filter(|flow| flow.provider_id.as_deref() == Some("aws"))
      .count(),
    2,
    "expected System.getProperty and System.getenv confirmed flows:\n{result:#?}"
  );
}

#[test]
fn javascript_block_scoped_source_binding_does_not_leak_to_sibling_block() {
  let result = analyze_source(
    "app.js",
    r#"
{
  let secret = process.env.STRIPE_API_KEY;
}
const stripeClient = require("stripe")(secret);
"#,
    SinkEvidenceMode::Off,
  );
  assert!(
    result.confirmed_flows.is_empty(),
    "sibling let source must not leak:\n{result:#?}"
  );
}

#[test]
fn java_local_source_binding_shadows_class_field_source_binding() {
  let result = analyze_source(
    "AwsEnvApp.java",
    r#"
import com.amazonaws.auth.BasicAWSCredentials;
class AwsEnvApp {
  String secret = System.getenv("AWS_SECRET_ACCESS_KEY");
  void configure() {
    String id = System.getProperty("AWS_ACCESS_KEY_ID");
    String secret = System.getenv("STRIPE_API_KEY");
    new BasicAWSCredentials(id, secret);
  }
}
"#,
    SinkEvidenceMode::Off,
  );
  assert_eq!(
    result
      .confirmed_flows
      .iter()
      .filter(|flow| flow.provider_id.as_deref() == Some("aws"))
      .count(),
    1,
    "local Stripe source should shadow AWS field for the secret arg:\n{result:#?}"
  );
}

#[test]
fn provider_mismatch_source_origin_does_not_emit_symbol_reachability() {
  let result = analyze_source(
    "app.js",
    r#"
const secret = process.env.STRIPE_API_KEY;
const client = new S3Client({ credentials: fromStatic({ accessKeyId: id, secretAccessKey: secret }) });
"#,
    SinkEvidenceMode::Off,
  );
  assert_eq!(
    result.source_evidence.len(),
    1,
    "source should be recorded:\n{result:#?}"
  );
  assert!(
    result.confirmed_flows.is_empty(),
    "mismatched provider must not confirm:\n{result:#?}"
  );
  assert!(
    result.symbol_reachability.is_empty(),
    "source-origin mismatch must not become literal reachability:\n{result:#?}"
  );
}

#[test]
fn cross_file_javascript_source_origin_reaches_imported_provider_sink() {
  let files = vec![
    (
      PathBuf::from("/repo/secrets.js"),
      r#"export const KEY = process.env.STRIPE_API_KEY;"#.to_string(),
    ),
    (
      PathBuf::from("/repo/app.js"),
      r#"import { KEY } from "./secrets";
const stripeClient = require("stripe")(KEY);
"#
      .to_string(),
    ),
  ];
  let result = analyze_sources_with_module_graph(&files, SinkEvidenceMode::Off);
  let flow = result
    .confirmed_flows
    .iter()
    .find(|flow| flow.provider_id.as_deref() == Some("stripe"))
    .unwrap_or_else(|| panic!("expected cross-file Stripe flow:\n{result:#?}"));
  assert_eq!(
    flow.flow.iter().map(|step| step.kind).collect::<Vec<_>>(),
    vec![
      FlowStepKind::Source,
      FlowStepKind::Export,
      FlowStepKind::Import,
      FlowStepKind::Sink,
    ]
  );
}

#[test]
fn cross_file_python_source_origin_reaches_imported_provider_sink() {
  let files = vec![
    (
      PathBuf::from("/repo/secrets.py"),
      "import os\nSECRET = os.environ[\"AWS_SECRET_ACCESS_KEY\"]\n".to_string(),
    ),
    (
      PathBuf::from("/repo/app.py"),
      "import boto3\nfrom secrets import SECRET\nboto3.client(\"s3\", aws_secret_access_key=SECRET)\n"
        .to_string(),
    ),
  ];
  let result = analyze_sources_with_module_graph(&files, SinkEvidenceMode::Off);
  let flow = result
    .confirmed_flows
    .iter()
    .find(|flow| flow.provider_id.as_deref() == Some("aws"))
    .unwrap_or_else(|| panic!("expected cross-file AWS flow:\n{result:#?}"));
  assert_eq!(flow.flow.len(), 4);
  assert_eq!(flow.flow[0].kind, FlowStepKind::Source);
  assert_eq!(flow.flow[1].kind, FlowStepKind::Export);
  assert_eq!(flow.flow[2].kind, FlowStepKind::Import);
  assert_eq!(flow.flow[3].kind, FlowStepKind::Sink);
}
