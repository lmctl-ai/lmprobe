# lmprobe aws query

Query an exact allowlisted AWS service operation with JSON parameters.

| Case | Invocation |
| --- | --- |
| Identity | `lmprobe aws query sts get-caller-identity --json` |
| EC2 filters | `lmprobe aws query ec2 describe-instances --params '{"Filters":[{"Name":"instance-state-name","Values":["running"]}]}' --region us-east-1 --json` |
| S3 object prefix | `lmprobe aws query s3api list-objects-v2 --params '{"Bucket":"example-bucket","Prefix":"logs/"}' --max-items 100 --json` |
| Request document | `lmprobe aws query --request-file request.json --outfile result.json` |

SERVICE and OPERATION are required together unless --request-file is supplied.
--params is a JSON object with native AWS API field names; default {}. --profile
and --region select existing AWS configuration. --max-items caps paginated results
(default 100); --page-size sets native page size; each integer must be 1..10000.
--starting-token accepts an earlier opaque CLI token. --first-page disables native
automatic pagination and conflicts with other pagination controls. All pagination
controls are refused for non-paginated operations. --select is a JSON Pointer,
not JMESPath, applied after pagination/failure checks and redaction. Empty pointer
selects the whole result; missing pointer errors. --timeout-seconds is 1..300,
default 60. --request-file - reads stdin. Request-file mode cannot mix with the
named query flags or positional service/operation. --help explains inherited flags;
[common.md](common.md) and [aws.md](aws.md) describe transport/safety boundaries.

## Versioned request file

The [JSON Schema](../schemas/lmprobe.aws.request.v1.schema.json) defines this
document's shape. Runtime catalog lookup still enforces the exact read-only
service/operation pair; native AWS models validate service parameters.

```json
{
  "schema":"lmprobe.aws.request.v1",
  "service":"ec2",
  "operation":"describe-instances",
  "parameters":{"Filters":[{"Name":"instance-state-name","Values":["running"]}]},
  "profile":"default",
  "region":"us-east-1",
  "pagination":{"max_items":100,"page_size":100},
  "select":"/Reservations",
  "timeout_seconds":60
}
```

Schema may be omitted (defaults v1), parameters default {}, pagination defaults
to the operation's normal bounded query, profile/region/select default unset.
Unknown keys and unsupported schema versions are rejected. Optional pagination
fields: max_items, page_size, starting_token, first_page. JSON server-side filters
support nested arrays/maps; no shell expansion or file:// interpolation occurs.
This JSON contract addresses complex inputs without introducing another query
language or exposing arbitrary AWS flags.

## Result and coverage

Raw lmprobe.aws_query.v1 carries data (native nested JSON, optionally selected),
service, operation, explicit profile/region/select, complete, partial, page,
redactions, aws_exit_code, exit_status and warnings. Program JSON uses hierarchical
payload.data and moves operation/coverage fields into metadata.

Page metadata is per-operation. Automatic pagination exposes its encoded CLI
NextToken as resume_token for --starting-token. First-page mode exposes native
output tokens and native_input_key/native_output_key, with resume_token:null;
continue by supplying that service input field in parameters. An echoed input
token (S3 list-objects-v2 ContinuationToken) is not a next-page signal. Reviewed
more-results fields such as IsTruncated determine completion. started_from_token
marks a resumed request. Pagination cap counts native paginator result items (EC2
reservations rather than flattened instances, for example).

Continuation tokens/IsTruncated and per-resource failures are detected before
--select can hide them. They set partial:true, complete:false, diagnostics and exit 3
in every output format. Complete means the requested segment has no continuation or
resource failures, not a global inventory outside filters/starting-token scope.
An explicitly selected empty array is no_match (exit 1, overridable via
--exit-zero-on-no-match); empty response objects may be valid API results and remain ok.
Intentional environment redaction is informational, not partial inventory coverage.
Failures never degrade to empty success arrays; the AWS process code is metadata.
aws_failures retains redacted per-resource failure detail, including when selection
fails. A missing selection adds its diagnostic without dropping upstream coverage,
failure or redaction diagnostics; error exit 2 takes precedence over partial.

Developer entry points: QueryArgs/AwsRequest/Pagination and request.validate/argv
in crates/lmprobe/src/aws_command.rs; exact gate in aws_policy::lookup. Native
credentials/config paths and request-file inputs are protected before parsing or
execution errors can route output. Tests: aws_command units and tests/aws_json.rs.
