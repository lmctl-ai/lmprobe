# Common CLI options and development contracts

Most commands accept inherited flags before or after the command name.
`list-kinds` shadows --lang with its positional LANG; use `list-kinds python`.
Accepted does not imply that every dispatch path uses every flag.
Command-specific limitations are recorded in each command file.

## Shared options

| Argument / option | Behavior |
| --- | --- |
| `--exit-zero-on-no-match` | Return exit 0 instead of 1 when no results match |
| `--no-cache` | Bypass history cache where supported |
| `--workspace <WORKSPACE>` | Read across repositories listed in a roots: YAML file; mutations refuse this mode |
| `-l, --lang <LANG>` | Force the parser language; use --type to filter files by extension |
| `-G, --globs <GLOBS>` | Include file globs (repeatable or comma-separated; prefix ! to exclude) |
| `--exclude-glob <EXCLUDE_GLOBS>` | Exclude file globs (repeatable or comma-separated) |
| `--no-ignore` | Include files excluded by .gitignore and .ignore |
| `--no-default-excludes` | Include dependency, build, VCS, and cache directories |
| `-j, --threads <THREADS>` | Worker thread count (default: num_cpus) |
| `-A, --after <AFTER>` | Lines after grep/ref hits; grep JSON includes structured context |
| `-B, --before <BEFORE>` | Lines before grep/ref hits; grep JSON includes structured context |
| `-C, --context <CONTEXT>` | Lines around grep/ref hits; grep JSON includes structured context |
| `-q, --quiet` | Suppress non-error stderr (warnings stay in the envelope) |
| `-v, --verbose` | Also print incidental and non-error JSON diagnostics to stderr |
| `--limit <LIMIT>` | Cap the primary result list; composite sub-lists may need their own limit |
| `--max-filesize <MAX_FILESIZE>` | Skip larger files (bytes); report FILE_TOO_LARGE [default: 5242880] |
| `-i, --ignore-case` | Case-insensitive matching for textual filters (grep, name) |
| `-t, --type <TYPE>` | Filter files by language extension without forcing a parser |
| `--json` | One lmprobe.result.v1 program result with common status/payload/diagnostics, including errors. |
| `--outfile <FILE>` | Program JSON written atomically to FILE; quiet stdout on successful write; - means stdout. |
| `--infile <FILE>` / `--intfile <FILE>` | Full GraphQL document; implies query/program JSON; - means stdin. |
| `--format <FORMAT>` | auto, markdown, json, ndjson, sarif, github, mermaid, dot. Auto selects markdown on a terminal and NDJSON when piped. |
| `-h, --help` | Print help (see a summary with '-h') |
| `-V, --version` | Print version |

See [program-json.md](../program-json.md) for the program contract, error routing,
advanced expression files, and outfile mutation/alias protections.

## Operational details

- No arguments: display help on stdout and exit 0. `-h` is compact;
  `--help` includes defaults and value descriptions. Missing required command
  inputs remain parse errors (exit 2).
- Most scan paths default to `.`. `grep`/`find` support repeatable `--path`;
  config-keys/py-slice-candidates take multiple positional roots. Other commands
  use the positionals shown in their help.
- `-l` forces a parser even when extensions/shebangs disagree. `--type` filters
  extensions. With no forced parser, detection uses extension then shebang.
- Ignored files and dependency/build/VCS/cache directories are skipped by
  default. `--no-ignore` and `--no-default-excludes` control different layers.
  Globs are repeatable or comma-separated; exclusion globs share the override
  builder with includes. Large skipped files generate FILE_TOO_LARGE.
- Auto output: markdown on a terminal, NDJSON on pipes. Prefer explicit
  `--format json` for one envelope with diagnostics. Find JSON adds files snapshots while keeping legacy paths; find NDJSON keeps
  path-string rows and carries typed files in the summary for piped actions.
  Raw `find --paths-only`
  overrides format. Mermaid/DOT require supported graph renderers; other verbs
  return FORMAT_NOT_APPLICABLE. `query` supports JSON/NDJSON.
- Standard exits: 0 matched, 1 no match, 2 error, 3 partial coverage.
  `--exit-zero-on-no-match` changes only no-match handling, not partial/errors.
  Secrets gitleaks compatibility uses its own finding/no-finding conventions.
- Quiet suppresses non-error stderr; warnings remain in envelopes. Verbose
  also mirrors incidental/non-error JSON diagnostics to stderr.
- Limit caps supported primary result lists, not every list inside composite
  envelopes. `context --recent-history-limit` controls its history slice.
- `--workspace` fans supported reads across YAML `roots:` with origin_root tags.
  Mutation paths refuse it. The daemon currently uses a single-root schema.
- `--no-cache` is forwarded by evolution and selected history paths; some
  history commands still use their shared cache. Do not claim universal cache
  bypass without checking dispatch_history.

## Where to change shared behavior

- [CommonArgs, Cli, run, dispatch_history, exit_code_for](../../crates/lmprobe/src/cli.rs)
- [WalkOpts, apply_walk_opts, walk_pre_check](../../crates/lmprobe-engine/src/sensors.rs)
- [OutputFormat and renderers](../../crates/lmprobe/src/output.rs)
- [Envelope types and Diagnostic](../../crates/lmprobe-types/src/schema.rs)
- [Workspace fan-out](../../crates/lmprobe/src/workspace.rs)

## Verification pitfalls

`cargo test -p lmprobe --locked` runs unit tests and many focused integration
suites, not just smoke. Deep/performance tests are explicitly ignored by default.
Some smoke/deep/performance cases require the external `$LMPROBE_QA_CORPUS`
corpus and may skip when absent; a green run alone does not prove those cases
ran. Timing gates run alone with `--test-threads=1`. See [testing](../16_testing.md).

CLI example tests parse commands without running their operations. This is
intentional for mutation, network, and daemon examples. Practical tests need
fixtures, isolated Git repos and XDG_CACHE_HOME, and appropriate language runtimes.

For local use, build `cargo build -p lmprobe --locked --release` and link or copy
`target/release/lmprobe` onto your `PATH`; rebuild to refresh it. Production
publishing is separate and must not be triggered by local verification.
