# lmprobe aws catalog

List exact supported service/operation pairs without launching AWS.

| Case | Invocation |
| --- | --- |
| Program inventory | `lmprobe aws catalog --json` |

No local flags; --help and inherited options are in [common.md](common.md).
Raw schema lmprobe.aws_catalog.v1 has rows[{service,operation,paginated,pagination}],
exit_status and warnings. Program JSON uses a rows payload, stable for empty data.
This operation does not need credentials or an AWS executable.
Pagination metadata gives the reviewed native input/output token keys and
optional more-results flag. It is null for non-paginated operations.

Implementation: aws_policy::OPERATIONS, aws_command::run, Commands::Aws dispatch.
See [aws.md](aws.md) for safety and supported-service scope. Changes to this list
require primary AWS API verification, deny tests, and paginator-model verification.
Do not infer read-only behavior from a get/list/describe name or IAM admin rights.

Verification: cargo test -p lmprobe --locked --bin lmprobe aws_policy and --test aws_json.
