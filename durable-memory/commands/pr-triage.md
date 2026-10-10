# lmprobe pr-triage

Show GitHub pull request checks, reviews, and comments.

## Use cases

| Case | Invocation |
| --- | --- |
| After latest author comment | `lmprobe pr-triage https://github.com/owner/repo/pull/123` |
| All fetched review/comment evidence | `lmprobe pr-triage --since all --compact https://github.com/owner/repo/pull/123` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe pr-triage [OPTIONS] <URL>
```

| Argument / option | Behavior |
| --- | --- |
| `<URL>` | GitHub pull request URL: https://github.com/<owner>/<repo>/pull/<number> |
| `--since <SINCE>` | Include review/comment evidence since this cutoff Possible values: - last-author-comment: Include PR comments/reviews after the author's latest issue comment - all:                 Include all fetched PR comments/reviews [default: last-author-comment] |
| `--compact` | Shorten evidence excerpts for terminal use |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::PrTriage](../../crates/lmprobe/src/cli.rs) and `Commands::PrTriage`; [dispatch: Commands::PrTriage](../../crates/lmprobe/src/cli.rs).
- [Engine: pr_triage::pr_triage](../../crates/lmprobe-engine/src/pr_triage.rs).
- [Result schema: PrTriageResult](../../crates/lmprobe-types/src/schema.rs).
- No direct same-name GraphQL resolver; inspect query composition before extending surface parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Calls `gh pr view`; requires GitHub authentication/access and may perform network I/O. Reports fetched metadata, not a merge/reviewer recommendation.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [pr_triage](../../crates/lmprobe/tests/pr_triage.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test pr_triage
```

Also check `lmprobe pr-triage -h` and `lmprobe pr-triage --help` after option changes.
