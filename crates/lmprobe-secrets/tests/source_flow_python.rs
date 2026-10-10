use lmprobe_secrets::{
  analyze_source, CredentialSourceEvidence, Secret, SecretsResult, SemanticConfidence,
  SinkEvidence, SinkEvidenceMode, SinkKind, SourceEvidence, SymbolReachabilityFinding,
  SINK_CATALOG_VERSION,
};

fn python(content: &str) -> lmprobe_secrets::SemanticScanResult {
  analyze_source("app.py", content, SinkEvidenceMode::Off)
}

#[test]
fn python_env_read_to_boto3_sink_emits_confirmed_flow() {
  let result = python(
    r#"import os
import boto3
secret = os.environ["AWS_SECRET_ACCESS_KEY"]
boto3.client("s3", aws_secret_access_key=secret)
"#,
  );

  assert_eq!(result.confirmed_flows.len(), 1, "{result:#?}");
  let flow = &result.confirmed_flows[0];
  assert_eq!(flow.confidence, SemanticConfidence::ConfirmedCredentialFlow);
  assert_eq!(flow.provider_id.as_deref(), Some("aws"));
  assert_eq!(
    flow.source.env_var_name.as_deref(),
    Some("AWS_SECRET_ACCESS_KEY")
  );
  assert_eq!(flow.sink.callee_text, "boto3.client");
  assert_eq!(flow.flow.len(), 4);
  assert!(result.symbol_reachability.is_empty());
}

#[test]
fn python_source_only_env_read_is_evidence_not_a_leak() {
  let result = python(
    r#"import os
os.environ["AWS_SECRET_ACCESS_KEY"]
"#,
  );

  assert_eq!(result.source_evidence.len(), 1, "{result:#?}");
  assert_eq!(
    result.source_evidence[0].env_var_name.as_deref(),
    Some("AWS_SECRET_ACCESS_KEY")
  );
  assert!(result.confirmed_flows.is_empty());
  assert!(result.symbol_reachability.is_empty());
}

#[test]
fn python_provider_mismatch_does_not_confirm_flow() {
  let result = python(
    r#"import os
import boto3
key = os.environ["STRIPE_API_KEY"]
boto3.client("s3", aws_secret_access_key=key)
"#,
  );

  assert_eq!(result.source_evidence.len(), 1, "{result:#?}");
  assert_eq!(
    result.source_evidence[0].provider_id.as_deref(),
    Some("stripe")
  );
  assert!(result.confirmed_flows.is_empty());
  assert!(result.symbol_reachability.is_empty());
}

#[test]
fn python_source_origin_dedup_keeps_distinct_env_reads_for_same_sink_line() {
  let result = python(
    r#"import os
import boto3
secret = os.environ["AWS_SECRET_ACCESS_KEY"]
token = os.environ["AWS_SESSION_TOKEN"]
boto3.client("s3", aws_secret_access_key=secret, aws_session_token=token)
"#,
  );

  assert_eq!(result.confirmed_flows.len(), 2, "{result:#?}");
  let mut envelope = SecretsResult::with_sink_catalog_version(Vec::new(), SINK_CATALOG_VERSION);
  envelope.confirmed_flows = result.confirmed_flows;
  envelope.rebuild_canonical_findings();
  assert_eq!(envelope.canonical_findings.len(), 2, "{envelope:#?}");
}

#[test]
fn python_source_origin_reassignment_kills_flow() {
  let result = python(
    r#"import os
import boto3
k = os.environ["AWS_SECRET_ACCESS_KEY"]
k = sanitize()
boto3.client("s3", aws_secret_access_key=k)
"#,
  );

  assert!(result.confirmed_flows.is_empty(), "{result:#?}");
}

#[test]
fn canonical_merge_keeps_literal_and_source_origins_distinct() {
  let sink = SinkEvidence {
    file_path: "app.py".into(),
    line: 10,
    column: 1,
    callee_text: "boto3.client".into(),
    arg_position: Some(1),
    arg_name: Some("aws_secret_access_key".into()),
    sink_kind: SinkKind::CredentialKwarg,
  };
  let mut envelope = SecretsResult::with_sink_catalog_version(Vec::new(), SINK_CATALOG_VERSION);
  envelope.semantic_findings.push(SymbolReachabilityFinding {
    detector: "symbol-reachability".into(),
    confidence: SemanticConfidence::BoundedSymbolFlow,
    provider_id: Some("aws".into()),
    provider_rule_ids: Vec::new(),
    credential_kind: None,
    sdk: None,
    provider_shape_id: None,
    source: SourceEvidence {
      file_path: "app.py".into(),
      line: 2,
      column: 1,
      variable: "literal".into(),
      literal: Secret::new("literal-secret"),
    },
    sink: sink.clone(),
    flow: Vec::new(),
  });
  envelope
    .confirmed_flows
    .push(lmprobe_secrets::ConfirmedCredentialFlowFinding {
      detector: "confirmed-credential-flow".into(),
      confidence: SemanticConfidence::ConfirmedCredentialFlow,
      provider_id: Some("aws".into()),
      source: CredentialSourceEvidence {
        file_path: "app.py".into(),
        line: 3,
        column: 1,
        source_id: "env-aws".into(),
        source_category: "env".into(),
        provider_id: Some("aws".into()),
        env_var_name: Some("AWS_SECRET_ACCESS_KEY".into()),
        key_name: None,
        sdk: "stdlib-env".into(),
        variable: "from_env".into(),
      },
      sink,
      intermediate_vars: vec!["from_env".into()],
      flow: Vec::new(),
    });
  envelope.rebuild_canonical_findings();
  assert_eq!(envelope.canonical_findings.len(), 2, "{envelope:#?}");
}

#[test]
fn java_system_get_property_source_is_p3_scope() {
  let result = analyze_source(
    "AwsEnvApp.java",
    r#"class AwsEnvApp {
  void run() { String key = System.getProperty("AWS_SECRET_ACCESS_KEY"); }
}"#,
    SinkEvidenceMode::Off,
  );
  assert_eq!(result.source_evidence.len(), 1, "{result:#?}");
  assert_eq!(
    result.source_evidence[0].env_var_name.as_deref(),
    Some("AWS_SECRET_ACCESS_KEY")
  );
  assert!(result.confirmed_flows.is_empty());
}
