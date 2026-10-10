//! Explicit allowlist for the read-only AWS CLI surface.
//!
//! This module deliberately contains no command execution or credential
//! handling.  The caller must use [`lookup`] before constructing an AWS CLI
//! invocation; an absent entry means the operation is outside lmprobe's
//! supported read-only surface.

/// Metadata for one AWS CLI operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationSpec {
  /// AWS CLI service name (for example, `ec2`).
  pub service: &'static str,
  /// AWS CLI operation name (for example, `describe-instances`).
  pub operation: &'static str,
  /// Whether the AWS CLI operation has a paginator in the AWS service model.
  pub paginated: bool,
}

/// The deliberately small, exact read-only AWS operation surface.
pub const OPERATIONS: &[OperationSpec] = &[
  op("sts", "get-caller-identity", false),
  op("ec2", "describe-instances", true),
  op("ec2", "describe-vpcs", true),
  op("ec2", "describe-subnets", true),
  op("ec2", "describe-security-groups", true),
  op("ec2", "describe-volumes", true),
  op("ec2", "describe-images", true),
  op("ec2", "describe-regions", false),
  op("s3api", "list-buckets", true),
  op("s3api", "list-objects-v2", true),
  op("s3api", "head-object", false),
  op("s3api", "get-bucket-location", false),
  op("s3api", "get-bucket-versioning", false),
  op("s3api", "get-bucket-tagging", false),
  op("s3api", "get-public-access-block", false),
  op("iam", "list-users", true),
  op("iam", "list-roles", true),
  op("iam", "list-groups", true),
  op("iam", "list-policies", true),
  op("iam", "list-attached-role-policies", true),
  op("iam", "list-attached-user-policies", true),
  op("iam", "get-role", false),
  op("iam", "get-user", false),
  op("iam", "get-policy", false),
  op("iam", "get-policy-version", false),
  op("lambda", "list-functions", true),
  op("ecs", "list-clusters", true),
  op("ecs", "list-services", true),
  op("ecs", "describe-clusters", false),
  op("ecs", "describe-services", false),
  op("ecs", "list-tasks", true),
  op("ecs", "describe-tasks", false),
  op("rds", "describe-db-instances", true),
  op("rds", "describe-db-clusters", true),
  op("logs", "describe-log-groups", true),
  op("logs", "describe-log-streams", true),
  op("cloudwatch", "list-metrics", true),
  op("cloudwatch", "describe-alarms", true),
  op("dynamodb", "list-tables", true),
  op("dynamodb", "describe-table", false),
  op("cloudformation", "list-stacks", true),
];

const fn op(service: &'static str, operation: &'static str, paginated: bool) -> OperationSpec {
  OperationSpec {
    service,
    operation,
    paginated,
  }
}

/// Return the exact allowlisted operation, if it is supported.
pub fn lookup(service: &str, operation: &str) -> Option<&'static OperationSpec> {
  OPERATIONS
    .iter()
    .find(|spec| spec.service == service && spec.operation == operation)
}

#[derive(Debug, Clone, Copy)]
pub struct PaginationSpec {
  pub input_token: &'static str,
  pub output_token: &'static str,
  pub more_results: Option<&'static str>,
}

/// Exact fields from the installed AWS service paginator models, not a global
/// token-name heuristic: S3's ContinuationToken is an echo for list-objects-v2.
pub fn pagination(spec: &OperationSpec) -> Option<PaginationSpec> {
  if !spec.paginated {
    return None;
  }
  let (input, output, more) = match (spec.service, spec.operation) {
    (
      "ec2",
      "describe-instances"
      | "describe-vpcs"
      | "describe-subnets"
      | "describe-security-groups"
      | "describe-volumes"
      | "describe-images",
    )
    | ("cloudwatch", "list-metrics" | "describe-alarms")
    | ("cloudformation", "list-stacks") => ("NextToken", "NextToken", None),
    ("s3api", "list-buckets") => ("ContinuationToken", "ContinuationToken", None),
    ("s3api", "list-objects-v2") => (
      "ContinuationToken",
      "NextContinuationToken",
      Some("IsTruncated"),
    ),
    (
      "iam",
      "list-users"
      | "list-roles"
      | "list-groups"
      | "list-policies"
      | "list-attached-role-policies"
      | "list-attached-user-policies",
    ) => ("Marker", "Marker", Some("IsTruncated")),
    ("lambda", "list-functions") => ("Marker", "NextMarker", None),
    ("ecs", "list-clusters" | "list-services" | "list-tasks")
    | ("logs", "describe-log-groups" | "describe-log-streams") => ("nextToken", "nextToken", None),
    ("rds", "describe-db-instances" | "describe-db-clusters") => ("Marker", "Marker", None),
    ("dynamodb", "list-tables") => ("ExclusiveStartTableName", "LastEvaluatedTableName", None),
    _ => return None,
  };
  Some(PaginationSpec {
    input_token: input,
    output_token: output,
    more_results: more,
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::collections::HashSet;

  #[test]
  fn allows_representative_read_operations() {
    assert!(!lookup("sts", "get-caller-identity").unwrap().paginated);
    assert!(lookup("ec2", "describe-instances").unwrap().paginated);
    assert!(!lookup("s3api", "head-object").unwrap().paginated);
    assert!(!lookup("iam", "get-policy-version").unwrap().paginated);
    assert!(lookup("cloudformation", "list-stacks").unwrap().paginated);
  }

  #[test]
  fn rejects_mutating_or_secret_bearing_operations() {
    for (service, operation) in [
      ("ec2", "run-instances"),
      ("s3api", "put-object"),
      ("s3api", "get-object"),
      ("lambda", "invoke"),
      ("lambda", "get-function"),
      ("secretsmanager", "get-secret-value"),
      ("ssm", "get-parameters"),
      ("sts", "assume-role"),
      ("logs", "start-query"),
      ("athena", "start-query-execution"),
      ("iam", "create-user"),
    ] {
      assert!(
        lookup(service, operation).is_none(),
        "{service} {operation}"
      );
    }
  }

  #[test]
  fn matching_is_exact_and_does_not_allow_injection_or_aliases() {
    for (service, operation) in [
      ("EC2", "describe-instances"),
      ("ec2", "describe-instance"),
      ("ec2", "describe-instances --dry-run"),
      ("ec2;rm -rf /", "describe-instances"),
      ("ec2", "describe-instances\0--no-paginate"),
      (" ec2", "describe-instances"),
    ] {
      assert!(lookup(service, operation).is_none());
    }
  }

  #[test]
  fn operation_keys_are_unique() {
    let keys = OPERATIONS
      .iter()
      .map(|spec| (spec.service, spec.operation))
      .collect::<HashSet<_>>();
    assert_eq!(keys.len(), OPERATIONS.len());
    assert!((30..=45).contains(&OPERATIONS.len()));
  }
}
