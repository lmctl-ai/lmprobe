# lmprobe help

Clap supplies this navigation command; it does not call an engine sensor.

## Use cases

```bash
lmprobe
lmprobe help
lmprobe help search
lmprobe help history blame
lmprobe search -h
lmprobe search --help
```

## Inputs and options

`lmprobe help [COMMAND]...` takes a sequence of nested command names.
Root flags `-h/--help` and `-V/--version` are in [common.md](common.md).
Help exits 0; an unknown command or missing required operational argument is
an error. Bare invocation does not scan files, load workspace configuration,
or initialize caches.

## Implementation entry points

[Cli derive and run](../../crates/lmprobe/src/cli.rs) use clap CommandFactory.
The explicit no-argument branch prints root short help and returns success.
Clap handles `help`, `-h`, and `--help` before dispatch. No result envelope.

## Verification

[help integration tests](../../crates/lmprobe/tests/help.rs) cover successful
stdout help and missing-argument errors. The binary help_tests walk the full
clap command tree, parse advertised examples without executing operations, and
check command-memory coverage.

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test help
```
