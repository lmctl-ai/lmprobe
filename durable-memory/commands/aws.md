# lmprobe aws

Read-only AWS CLI queries with JSON inputs and structured outputs. Requires a
trusted installed/configured AWS CLI for queries; catalog is entirely local.

| Case | Invocation |
| --- | --- |
| Identify current AWS principal | `lmprobe aws query sts get-caller-identity --json` |
| List supported operations | `lmprobe aws catalog --json` |
| Complex request | `lmprobe aws query --request-file request.json --outfile result.json` |

Subcommands: [catalog](aws-catalog.md), [query](aws-query.md). --help shows usage;
[common.md](common.md) describes global options. --json emits lmprobe.result.v1;
--outfile implies that transport and quiet stdout. --format json stays the raw
sensor envelope. --infile is still a full GraphQL document; AWS uses --request-file.
No AWS GraphQL resolver is added. --workspace is refused for both subcommands.
Filesystem traversal/language/glob/cache/context/limit flags do not affect AWS.

The service/operation pair must match the reviewed exact allowlist in
`crates/lmprobe/src/aws_policy.rs`. No prefix matching, arbitrary flags, command
passthrough, mutation opt-in, credential issuance, secret extraction, code
invocation, or query-job creation is exposed. Parameters go into one serialized
--cli-input-json argument and cannot replace the operation or CLI transport.
Existing AWS profiles and normal authentication are delegated to the AWS CLI.
`LMPROBE_AWS` overrides the trusted executable for testing/deployment; it is not
a request parameter and must never come from an untrusted caller.

Operations cover STS identity, EC2 metadata, S3 API metadata/object listings,
IAM identities/policies, Lambda inventory, ECS inventory, RDS inventory,
CloudWatch/log inventory, DynamoDB metadata and CloudFormation stack summaries.
The initial paginator flags were checked against installed AWS CLI 2.33.26
service/paginator models; do not guess pagination from operation-name prefixes.
S3 list-buckets is paginated in current models; EC2 describe-regions is not.
The catalog is an explicit supported subset, not all AWS read APIs.

## Verified state (2026-10-06)

41 operations and their paginator support were compared to installed AWS CLI
models. The documented JSON request validates against the
[versioned request schema](../schemas/lmprobe.aws.request.v1.schema.json).
Eleven mocked AWS integration tests plus policy/request unit cases passed;
the combined CLI/engine suite passed 384 tests (4 ignored). Tester passed 58
baseline and 12 focused mock scenarios; Reviewer approved the final guards.
Live STS, capped S3 and filtered EC2 reads and installed-binary JSON/outfile
contracts were verified. Mutation refusal was tested only with mocks, never by
trying a denied action against the admin role. Linux/Unix process-group behavior
was tested; non-Linux behavior and production publication were not verified.

Review corrections to preserve: auth-file protection includes environment token
files and profiles merged from both config and shared credentials; input paths
must be expanded without silent fallback. Native first-page tokens differ from
CLI paginator resume tokens, and S3 echoed input tokens are not continuation.
Missing projections retain upstream diagnostics and redacted failure details.

## Safety, process, and output boundaries

- Validation happens before launching AWS. AWS query calls use JSON output,
  no interactive pager or auto-prompt, null stdin, connect/read timeouts 10/30s,
  and two retry attempts. Default process deadline is 60s (1..300 allowed).
- Unix processes run in their own group; timeout kills only that group, including
  descendants holding capture pipes. Capture limits are 16 MiB stdout / 1 MiB
  stderr; capture drains excess without retaining it. Errors/timeouts/invalid
  JSON/overflow return structured diagnostics and exit 2.
- Profile and region option values remain one argv entry, including whitespace;
  no shell is launched. Native AWS schema validation checks the API parameters.
- Outfile cannot alias request inputs, expanded AWS config/credential paths,
  environment token files, or profile web_identity_token_file/ca_bundle inputs;
  the default .aws configuration/cache tree is protected too. This applies to
  help/parser errors. Unresolvable auth-input paths fail closed for file output.
  Native credential-cache behavior remains
  AWS CLI-owned; lmprobe does not persist credentials or a resource index.
- Response Environment fields are redacted to [REDACTED] before selection/output;
  redactions and an informational diagnostic make this visible. Other response
  fields and AWS nested JSON structures remain available to callers.
- Credential values are never supplied in request documents; profile names may
  be selected through the documented profile field. Avoid
  storing access keys, session tokens, or secrets in parameter values/logs.

## Developer map

`crates/lmprobe/src/aws_policy.rs`: reviewed service/operation catalog and unit tests.
`crates/lmprobe/src/aws_command.rs`: request schema, validation, argv construction,
bounded process/capture, pagination, redaction and unit tests.
`crates/lmprobe/src/cli.rs`: dispatch, input/output alias guard, coverage exit codes.
`crates/lmprobe/src/program_output.rs`: catalog rows/query hierarchy adapters.
`crates/lmprobe/tests/aws_json.rs`: mocked CLI safety/transport regressions.

Run cargo test -p lmprobe --locked --bin lmprobe aws and --test aws_json.
Live verification must use only cataloged queries; never try a denied action
against the admin role to test the guard. Mock subprocess call markers prove refusal.

Primary references: [AWS CLI JSON/output](https://docs.aws.amazon.com/cli/latest/userguide/cli-usage-output.html),
[pagination](https://docs.aws.amazon.com/cli/latest/userguide/cli-usage-pagination.html),
[JSON parameters](https://docs.aws.amazon.com/cli/latest/userguide/cli-usage-parameters-json.html).
