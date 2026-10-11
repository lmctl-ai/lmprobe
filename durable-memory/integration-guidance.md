# Integration guidance for trace evidence

This note describes how another tool can consume `lmprobe.trace.v1` as
mechanical runtime evidence. It intentionally leaves interpretation, scoring,
and presentation to the consuming tool.

## Evidence boundary

The trace envelope records what a temporary instrumented run emitted. An event
is evidence that a watched site produced a value during that run; its presence
does not establish correctness, safety, ownership, policy compliance, or a
business conclusion. An absent event can mean that the site did not execute,
that the language or watch expression was unsupported, or that the runtime
payload could not be parsed. Always inspect `warnings` and `exit_status`.

The producer contract is the `TraceRunResult` type in
[`lmprobe-types`](../crates/lmprobe-types/src/schema.rs). Its stable v1 fields
are `schema`, `events`, `warnings`, and `exit_status`. Each event always has
`timestamp` and `output`; watch mode may add `var`, `type`, `id`, `len`,
`shape`, and `repr`. Optional fields describe captured metadata and should not
be treated as universally available across languages.

## Author-time and render-time separation

Use trace output while authoring or validating a trace-backed artifact. A
consumer should record enough provenance to reproduce the association between
source and evidence, such as the source-relative path, query, watched names,
language, command, and the source revision selected for the run. These are
consumer-side records; they are not additional fields in the current trace
envelope.

Render-time artifacts should contain the intended result or explanation, not
the raw trace payload, instrumentation templates, temporary worktree names,
or generator internals. This keeps the output stable and prevents a runtime
capture detail from becoming learner-facing content or a public API contract.

## Fidelity choices

Choose the smallest fidelity that serves the consumer:

- **Results-only** artifacts contain selected values, labels, or static frames.
  They are portable and do not require live re-evaluation.
- **Keyframe** artifacts preserve a sequence of selected states and their
  source anchors. A keyframe can be a compact state card or a richer static
  frame; the format is a consumer decision.
- **Full replay** artifacts retain enough source and runtime context to repeat
  the capture. They require stronger provenance and version checks and should
  not be assumed merely because a trace envelope was successful.

Manual or AI-authored keyframes are valid when a language has no stable
instrumentation path or when the desired presentation is richer than runtime
events. Label those artifacts as curated and keep their source anchor and
evidence provenance beside them. Do not imply that manual keyframes are
automatically replayable.

## Watch-mode integration

`trace --watch` requires `--json-trace`. Watch names are deliberately narrow:
Python and Bash use simple identifiers; R also permits dotted simple names.
Expression-level anchors are not guaranteed safe. Watch complete statements or
other anchors documented for the selected language.

Language support differs. Python exposes the richest optional metadata. Bash
currently supports `len` for scalars and arrays while omitting `shape` and
`id`. R uses conservative line matching and base-R serialization. A consumer
must tolerate omitted optional fields.

The current Bash and R emitters use bounded, language-side JSON escaping rather
than a general-purpose serializer. Backslash, quote, newline, carriage return,
and tab are covered; other control bytes may cause an individual event to be
dropped. Treat malformed event loss as a warning or incomplete capture, not as
proof that the watched value was never produced.

## Source scope and recovery

Before accepting a capture, verify that the instrumented files and query are
inside the intended source scope. Prefer `--worktree` for integrations: it
starts from `HEAD`, isolates temporary instrumentation, and leaves the user's
tree untouched. `--keep-patch` can preserve the generated patch for audit or
debugging. If execution fails, the worktree may be preserved for inspection.

In-place trace remains guarded by the mutation-safety rules in
[`06_mutations.md`](06_mutations.md): clean-tree checks, audit-before-mutation,
backup/restore, and rollback. Consumers should treat audit and rollback
diagnostics as part of the result and should never assume a successful process
means every event was captured.

## Consumer checklist

1. Require `schema == "lmprobe.trace.v1"`.
2. Validate that `events` is an array and each event has `timestamp` and
   `output`.
3. Preserve `warnings` and `exit_status` next to derived artifacts.
4. Record consumer-side provenance for source scope, query, watch names,
   language, command, and source revision.
5. Reject or flag incomplete captures before producing a replay-dependent
   artifact.
6. Keep rendered output results-only unless the consumer explicitly chooses a
   richer, documented fidelity.

For scanner-style evidence, apply the same boundary: local evidence is not a
verdict. See [`19_scanner_recipes.md`](19_scanner_recipes.md).
