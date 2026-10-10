# lmprobe resource-sites

Find acquire/release anchors and local context.

## Use cases

| Case | Invocation |
| --- | --- |
| Acquire/release anchors | `lmprobe resource-sites . --acquire 'os.Open\|File::open' --release 'Close\|drop'` |
| Source and structural evidence | `lmprobe resource-sites src/ --acquire 'os.Create' --release 'Close' --around 3 --parents --body` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe resource-sites [OPTIONS] --acquire <ACQUIRE> --release <RELEASE> <PATH>
```

| Argument / option | Behavior |
| --- | --- |
| `<PATH>` | Scan root. |
| `--acquire <ACQUIRE>` | Regex for acquire-like anchors |
| `--release <RELEASE>` | Regex for release-like anchors |
| `--around <AROUND>` | Include N source lines around acquire anchors |
| `--parents` | Attach enclosing structural declaration chain |
| `--body` | Attach innermost enclosing structural body |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::ResourceSites](../../crates/lmprobe/src/cli.rs) and `Commands::ResourceSites`; [dispatch: Commands::ResourceSites](../../crates/lmprobe/src/cli.rs).
- [Engine: resource_sites::resource_sites](../../crates/lmprobe-engine/src/resource_sites.rs).
- [Result schema: ResourceSitesResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: resource_sites](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Reports local syntactic/lexical evidence, not ownership, control flow, authorization, compliance, or data-flow correctness. Preserve source anchors and diagnostics when extending coverage.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [resource_sites](../../crates/lmprobe/tests/resource_sites.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test resource_sites
```

Also check `lmprobe resource-sites -h` and `lmprobe resource-sites --help` after option changes.
