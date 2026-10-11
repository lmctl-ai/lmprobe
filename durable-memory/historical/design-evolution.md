# Historical design lessons

This chapter preserves technical reasoning from earlier design and validation
notes. It is not a current CLI reference, performance report, implementation plan,
or source of operating authority. Current contracts live in
[ARCHITECTURE.md](../../crates/lmprobe/ARCHITECTURE.md) and the
[command map](../commands/index.md).

## Discovery before parsing

Project markers and path/text candidate selection can narrow a large source tree
before structural parsing. AST validation then distinguishes code structure from
comments or textual resemblance. Metadata enrichment should apply to the selected
scope rather than justify eagerly indexing the entire workspace. A candidate
filter must not suppress legitimate matches; test it against broader scans.

## Live truth versus cached history

An early proposal used a persistent live symbol index. The shipped design instead
parses current source on demand: cached mutable declarations can become ghost
code after edits or branch changes. Immutable Git blobs are a different case and
can be cached by content identity, language and interpretation/version policy.
The current XDG cache contract supersedes old workspace-local cache proposals.

Historical evolution queries should distinguish file-changing commits from
symbol-level structural changes. Formatting-only changes can be useful provenance
without constituting a semantic milestone. Shallow/missing history and renamed
files require explicit coverage signals; do not turn absence into a correctness
or ownership verdict. See [history](../05_history.md) and [cache](../09_cache.md).

## Useful context without inflated certainty

Outlines, declarations and minimal structural context reduce the material a caller
must inspect. Ancestor/descendant checks on the same parsed tree provide more
precise evidence than independent text filters. Call relations and import edges
still carry language-specific/static limitations; they are not a universal type
checker or runtime call graph.

## Instrumentation as a reversible experiment

Temporary instrumentation needs an auditable source scope, explicit execution
command, refusal rules and tested recovery. Replacing a marker in an AST does not
make execution safe, and successful process completion does not prove complete
trace coverage. HEAD worktrees and live editing are distinct inputs and must not
be conflated. See [mutations](../06_mutations.md) and
[trace integration](../integration-guidance.md).

## Validation lessons

Early benchmark numbers and historical command syntax are not portable promises.
Some proposals referred to an upstream CLI package or flags that this workspace
does not expose. Verify the actual parser/help and current crate layout before
reusing a recipe. Keep full CLI/GraphQL envelope comparisons, focused temporary
fixtures, and explicit corpus/ignored-gate reporting; do not replace real checks
with a past success count. See [testing](../16_testing.md) and [performance](../15_perf.md).
