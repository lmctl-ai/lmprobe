# Program JSON and expression files

Program clients use `--json`, `--outfile`, or `--infile`. All select the
versioned `lmprobe.result.v1` transport. Existing `--format json` remains the
legacy sensor envelope, including PR metadata's own status object.

```bash
lmprobe find '\.py$' . --json
lmprobe --infile expression.graphql --outfile result.json
lmprobe query --infile expression.graphql --var root=. --operation-name Inspect
```

`--intfile` aliases `--infile`; requested single-dash `-infile`/`-intfile` are
accepted too. An infile without a command implies query, including query-local
variables and operation options. Files contain a full UTF-8 GraphQL document,
not just CLI filters: comments, variables, aliases, fragments, directives,
multiple operations, nested resolver composition, and existing mutations all
remain available. File contents are interpreted literally, never decoded again
as @FILE or stdin. `--infile -` reads stdin; `--outfile -` writes stdout.

Machine-readable [JSON Schema](schemas/lmprobe.result.v1.schema.json) defines
the envelope and discriminated payload shapes.

## Stable result contract

```json
{
  "schema": "lmprobe.result.v1",
  "source_schema": "lmprobe.find.v1",
  "status": "ok",
  "payload_format": "rows",
  "payload": {
    "columns": ["path", "size_bytes"],
    "rows": [{"path": "src/app.py", "size_bytes": 120}]
  },
  "metadata": {"paths": ["src/app.py"]},
  "diagnostics": []
}
```

The source schema names the command-specific record contract. No source fields
are discarded: primary data moves into payload, remaining data into metadata,
and warnings into diagnostics. PR-triage's PR status stays in its hierarchical
data; wrapper status always means invocation outcome. Types stay JSON types,
including numbers, booleans, nulls, nested row values, and redacted secret data.

| Field | Contract |
| --- | --- |
| schema | Always lmprobe.result.v1; independent of the sensor version. |
| source_schema | Original command/transport schema, including lmprobe.cli_error.v1 for early failures. |
| status | ok, no_match, partial, error; precedence error > partial > no_match > ok. |
| payload_format | list, rows, hierarchical; selected by source schema, not result count. |
| payload | list: {items: [primitive values]}; rows: {columns: [field names], rows: [objects]}; hierarchical: {data: JSON}. |
| metadata | Remaining command-specific data, e.g. paths, counts, action outcomes, audit paths, GraphQL errors. |
| diagnostics | Diagnostic objects: code, message, optional nullable hint and lang. |

Rows are objects, not positional arrays. Columns are the sorted union of observed
row keys, so an empty result has columns=[] and rows=[]; source_schema still
identifies the record type. Empty/nonempty results retain the same payload_format.
Lists carry scalar documents/text for recipe-show/help/version. Discover carries
a project tree; context/impact/secrets/PR/query carry hierarchical data.

Text `edit` uses source_schema `lmprobe.edit.v1` and rows with path, before,
after, applied. Metadata carries mode and audit_path; unchanged files are omitted.
An incomplete apply returns error and keeps successful per-file applied flags
and its recovery location. Edit scripts use `--script`; `--infile` remains GraphQL.

Git inspection uses `lmprobe.git_VERB.v1` source schemas: status/diff/log/branch/
remote/ls_files/blame are rows; show/rev_parse are hierarchical data. Every result
carries repository/head/worktree state in metadata. Clean status is ok with empty
rows. Invalid UTF-8 data uses `{bytes_hex:HEX}` values, preserving identity rather
than lossy filenames. See [Git](commands/git.md) for fields and outfile boundaries.

AWS uses lmprobe.aws_catalog.v1 rows and lmprobe.aws_query.v1 hierarchical data.
Operation, AWS exit code, pagination tokens, completeness and redaction counts
are metadata. Partial coverage survives selection; --request-file accepts the
versioned JSON AWS request while --infile remains GraphQL. See [AWS](commands/aws.md).

Status comes from explicit source outcome and coverage flags/diagnostics, never
from arbitrary data fields or payload size. DELETE_PREVIEW and intentionally
ignored files are warnings, not partial coverage. --quiet suppresses stderr
mirroring, not JSON diagnostics. Standard exit codes are 0/1/3/2 for
ok/no_match/partial/error; --exit-zero-on-no-match changes the process code only.
Explicit secrets --gitleaks-compat retains its compatibility exit convention;
use the JSON status rather than guessing status from the process exit code.

GraphQL collects status/partial/diagnostics in request-scoped state before
selection-set projection, including SymbolHit children. A query selecting only
paths has the same outcome as one selecting warnings/partial. Returned data and
GraphQL errors (paths, locations, codes) remain available independently.

## File output and failures

--outfile implies program JSON and leaves stdout empty on a successful write.
It writes successful, no-match, partial, and command-error results. Existing
writable regular files are atomically replaced after complete serialization,
write, and sync. Parent directories must already exist. Directory, symlink,
read-only, input-alias, and known mutation-target collisions are refused.

Destination preflight is read-only: output temp files are created only after a
result is ready. This avoids changing selection metadata or dirtying Git before
fix/trace safety checks. Metadata/access checks cannot guarantee that a later
write/rename succeeds (ACLs, disk capacity, concurrent changes). A late commit
failure preserves the previous output when rename has not happened, cleans its
owned temp, emits one error result to stdout with OUTFILE_WRITE_FAILED and
metadata.operation_status, and exits 2. Already completed operations are not
repeated or rolled back by output handling.

Parser/help/version completion conservatively checks explicitly mentioned input
paths too, so a typo or --help cannot cause input-file replacement. Workspace
configurations and untrace audit inputs/targets are protected, including
outside-scope audit targets and aliases. Parser guards understand attached
short input values (e.g. -rrule.yaml). Untrace fallback protection checks its
actual marker-bearing files regardless of extension, not just source languages.

Unsafe input/mutation overlaps cancel file routing and return structured error
JSON on stdout, preserving the source file. Mutation batches/GraphQL documents
retain their existing nontransactional semantics; earlier completed mutations
are not undone by a later resolver/output failure. Trace rejects an existing
recognized source file under its scope as an outfile.

Program JSON conflicts with raw --paths-only or explicit non-JSON formats.
Streaming serve cannot produce a one-shot result and is rejected before startup;
its HTTP GraphQL responses remain JSON. Help/version/parser/runtime errors also
use program JSON when requested. Ordinary behavior without these new options is
preserved, including legacy --format json and automatic Markdown/NDJSON.

## Pipelines and advanced expression example

```bash
lmprobe find '\.log$' . --older-than 1w --json | lmprobe delete --json
lmprobe find '\.log$' . --older-than 1w --json | lmprobe delete --json --force
```

The delete consumer explicitly unwraps find program rows, preserves original
snapshots/source paths, and applies the same completeness/identity rules. It
rejects other source schemas, failed/partial results, and inconsistent rows.

An expression.graphql may contain operations unavailable as individual flags:

```graphql
query Inspect($root: String!) {
  repos: find(
    pattern: ".*", path: $root
    filter: { kind: DIRECTORY, gitRemote: { owner: "acme", repo: "widget" } }
    prune: { hasEntry: ".git" }
  ) { paths }
  projects: discover(path: $root) { projects { name children { name } } }
}
```

## Implementation and verification

- [schema.rs](../crates/lmprobe-types/src/schema.rs): ProgramResult/ProgramStatus/PayloadFormat, QueryResult.partial, child-resolver context.
- [program_output.rs](../crates/lmprobe/src/program_output.rs): schema-based wrapper, status precedence, destination protection, atomic completion.
- [cli.rs](../crates/lmprobe/src/cli.rs): invocation detection, global flags, literal infile, runtime errors, request-scoped GraphQL progress.
- [file_actions.rs](../crates/lmprobe-engine/src/file_actions.rs): explicit typed find program-input adapter.
- [program_json tests](../crates/lmprobe/tests/program_json.rs): shapes, errors, projection, advanced input, quiet/atomic file output, collisions, no dirty-tree interference, and typed pipelines.

```bash
cargo test -p lmprobe --locked --test program_json
cargo test -p lmprobe --locked --bin lmprobe
cargo test -p lmprobe --locked
```
