# lmprobe trace

Inject logs, run a command, then restore source files.

## Use cases

| Case | Invocation |
| --- | --- |
| Log/run/rollback in a clean repo | `lmprobe trace 'print($X)' -l python . --exec 'python3 app.py'` |
| Isolate HEAD and preserve patch | `lmprobe trace 'print($X)' -l python . --exec 'python3 app.py' --worktree --keep-patch` |
| Watch runtime values | `lmprobe trace 'print($X)' -l python . --exec 'python3 app.py' --watch x --json-trace --shape --len --id --repr-limit 80` |
| Custom injection template | `lmprobe trace 'print($X)' -l python . --exec 'python3 app.py' --template 'print("[TRACE]"); print($X)'` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe trace [OPTIONS] --exec <EXEC> <QUERY> [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `<QUERY>` | Required command input; see use cases above. |
| `[PATH]` | [default: .] |
| `--exec <EXEC>` | Shell command to run after instrumentation |
| `--template <TEMPLATE>` | Override the language-specific log injection template |
| `--watch <WATCH>` | Python/Bash/R watch variables. Requires `--json-trace` |
| `--json-trace` | Emit and parse bounded `[TRACE_JSON]` runtime evidence for `--watch` |
| `--shape` | Include `shape` metadata for watched values when available |
| `--len` | Include `len(...)` metadata for watched values when available |
| `--id` | Include object identity metadata for watched values |
| `--repr-limit <REPR_LIMIT>` | Bound watched value repr payloads [default: 120] |
| `--force` | Bypass the dirty-tree or non-Git safety check |
| `--worktree [<WORKTREE>]` | Run in a detached worktree at HEAD (optional location) |
| `--keep-patch [<KEEP_PATCH>]` | Save the instrumentation patch before rollback (optional destination) |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::Trace](../../crates/lmprobe/src/cli.rs) and `Commands::Trace`; [dispatch: Commands::Trace](../../crates/lmprobe/src/cli.rs).
- [Engine: sensors::trace_with_safety_and_watch](../../crates/lmprobe-engine/src/sensors.rs).
- [Result schema: TraceRunResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: trace](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Python/Bash watch uses structured [TRACE_JSON] evidence; R/r watch is line-based and requires an exact trimmed standalone source-line query. --watch requires --json-trace; shape/len/id/repr-limit are watch metadata controls, with language-specific availability. --worktree uses HEAD, not dirty local source; on execution failure its location may be preserved. Rollback/audit and D7 safety must survive failures. --template placeholders are implementation-defined; inspect the injection code before changing templates.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [trace_watch_json](../../crates/lmprobe/tests/trace_watch_json.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test trace_watch_json
```

Also check `lmprobe trace -h` and `lmprobe trace --help` after option changes.
