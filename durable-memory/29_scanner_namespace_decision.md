# 29 — Scanner namespace decision

This note closes the scanner namespace planning item for the current 0.x
surface. It does not authorize a CLI rename.

## Decision

Keep the existing direct evidence-command style as the stable user-facing
surface for the current release line:

- `resource-sites`
- `guard-sites`
- `async-leaks`
- `crypto-sites`
- `config-keys`
- `openapi-facts`
- `route-anchors`
- `http-client-calls`
- `secrets`
- `secrets-recipes`
- `secrets-sources`
- `py-slice-candidates`
- `callsites`
- `diff-context`

Do not introduce `lmprobe scan <kind>` as a parallel namespace in this slice.
New evidence scanners should continue to follow the existing direct-command
pattern unless a later design explicitly changes the major surface.

## Rationale

The direct commands already have documented envelopes, GraphQL resolvers where
applicable, examples, and downstream recipes. Adding a `scan` namespace now
would create two names for the same evidence class before there is a concrete
consumer need that offsets the migration cost.

The stronger boundary is semantic, not lexical:

- Scanner commands collect evidence only.
- They do not assert findings, severities, vulnerabilities, or policy verdicts.
- AI agents and downstream products own interpretation after inspecting the
  returned anchors and diagnostics.

That boundary is documented in `19_scanner_recipes.md`.

## Interpretation boundaries

### lmprobe-owned

- Command invocation, path walking, language parsing, and local evidence
  extraction.
- Versioned envelope names, warning codes, and `exit_status` semantics.
- GraphQL resolver parity when a scanner is exposed through `query`.
- Safety class documentation for read/write behavior.

### Downstream-owned

- Narrative, lesson, or remediation interpretation.
- Severity, policy, vulnerability, correctness, or business-impact claims.
- Aggregation across code evidence and non-code evidence.
- Release pinning and compatibility testing in consuming repositories.

## Migration table

| Current command | Future command | Alias window | Deprecation date | Minimum version | Status |
| --- | --- | --- | --- | --- | --- |
| `resource-sites` | unchanged | none | none | current workspace version | Canonical direct command |
| `guard-sites` | unchanged | none | none | current workspace version | Canonical direct command |
| `async-leaks` | unchanged | none | none | current workspace version | Canonical direct command |
| `crypto-sites` | unchanged | none | none | current workspace version | Canonical direct command |
| `config-keys` | unchanged | none | none | current workspace version | Canonical direct command |
| `openapi-facts` | unchanged | none | none | current workspace version | Canonical direct command |
| `route-anchors` | unchanged | none | none | current workspace version | Canonical direct command |
| `http-client-calls` | unchanged | none | none | current workspace version | Canonical direct command |
| `secrets` | unchanged | none | none | current workspace version | Canonical direct command |
| `secrets-recipes` | unchanged | none | none | current workspace version | Canonical direct command |
| `secrets-sources` | unchanged | none | none | current workspace version | Canonical direct command |
| `py-slice-candidates` | unchanged | none | none | current workspace version | Canonical direct command |
| `callsites` | unchanged | none | none | current workspace version | Canonical direct command |
| `diff-context` | unchanged | none | none | current workspace version | Canonical direct command |
| `scan <kind>` | not introduced | none | none | n/a | Reserved for a future major-surface decision |

## Compatibility statement

Third-party integrations may keep using the direct scanner commands
listed above. The project should not remove, rename, or require a `scan`
namespace for those commands without a new migration design that includes:

- updated CLI `--help`, durable-memory, README/site, and GraphQL recipe docs;
- an alias/deprecation schedule tied to the single workspace version source;
- dry-run validation against downstream scripts that call current commands;
- a rollback plan that restores direct-command invocation without schema
  changes.

## Rules for future scanner expansion

1. Add new evidence scanners as direct commands unless a future major-surface
   migration is approved first.
2. Add or update the scanner row in `19_scanner_recipes.md`.
3. Document the envelope namespace and GraphQL resolver parity, if applicable.
4. Preserve the "evidence, not verdict" language in command help and docs.
5. Do not add GraphQL schema changes solely to support a lexical namespace
   experiment.

## Reconsideration trigger

Reopen the `scan` namespace question only if at least one of these becomes
true:

- scanner command count or help discoverability becomes a measured usability
  blocker;
- a downstream integration needs one polymorphic scanner entry point rather
  than GraphQL aliases or direct commands;
- a scheduled major-surface review is already planned and can carry the
  migration cost.
