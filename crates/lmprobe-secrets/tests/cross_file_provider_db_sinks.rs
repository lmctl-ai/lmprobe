use std::path::PathBuf;

use lmprobe_secrets::{analyze_sources_with_module_graph, FlowStepKind, SinkEvidenceMode};

#[test]
fn python_cross_file_provider_db_sink_preserves_provider_metadata() {
  let files = vec![
    (
      PathBuf::from("/repo/secrets.py"),
      "AWS_SECRET = \"cross_file_aws_secret\"\n".to_string(),
    ),
    (
      PathBuf::from("/repo/app.py"),
      r#"
import boto3
from secrets import AWS_SECRET

boto3.client("s3", aws_secret_access_key=AWS_SECRET)
"#
      .to_string(),
    ),
  ];

  let result = analyze_sources_with_module_graph(&files, SinkEvidenceMode::Off);
  assert_eq!(result.symbol_reachability.len(), 1);
  let finding = &result.symbol_reachability[0];
  assert_eq!(finding.provider_id.as_deref(), Some("aws"));
  assert_eq!(finding.sdk.as_deref(), Some("boto3-v1"));
  assert!(finding
    .provider_shape_id
    .as_deref()
    .is_some_and(|shape_id| { shape_id.starts_with("aws/python/boto3-v1/") }));
  assert_eq!(finding.flow.len(), 4);
  assert_eq!(finding.flow[0].kind, FlowStepKind::Definition);
  assert_eq!(finding.flow[1].kind, FlowStepKind::Export);
  assert_eq!(finding.flow[2].kind, FlowStepKind::Import);
  assert_eq!(finding.flow[3].kind, FlowStepKind::Sink);
}
