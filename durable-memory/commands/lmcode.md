# lmcode host

lmprobe exposes its operation library through the native lmcode host protocol.
The Rust SDK dependency is pinned by full commit SHA in `crates/lmprobe/Cargo.toml`.
The host schema is `crates/lmprobe/lmcode/host.schema.json`; operation names and
parameter metadata are in `operations.json`, generated from the clap tree and
`overlay.toml`.

## Host commands

- `lmprobe --lmcode-schema --json` prints the host schema.
- `lmprobe --lmcode-signatures` prints the callable signatures.
- `lmprobe --lmcode FILE` runs a compiled program and writes a terminal
  `LMCODE_RETURN` frame. `--first` and `--last` select the invocation result
  range for multi-program calls.

The host exports 63 functions named `probe_<operation>`. Each accepts a JSON
string and returns the operation's JSON result as a string. A program can call
`json_parse` on the result to use it as a structured value. The dry callback
returns JSON null and does not start an operation child. Dry is controlled by
the invocation manifest; the legacy `dryrun` parameter is rejected.

Operation inputs retain the published parameter names and types in
`operations.json`. `readonly` is an invocation policy input, `confirm` is
required for dangerous writes, and `LMPROBE_READONLY` can enforce a read-only
floor. The native CLI remains available with its existing commands and behavior.

## Verification

`cargo test -p lmprobe --test lmcode_host --test lmcode_ops --test lmcode_policy --test lmcode_hostile --test lmcode_selfexec --locked`
exercises the schema, host frames, all operations, dry mode, policy and child
process behavior. The push/PR workflow also installs the pinned compiler package
locally and runs these tests with `LMSCRIPT_BIN` set.
