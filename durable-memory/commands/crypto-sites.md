# lmprobe crypto-sites

Find Go crypto algorithm uses and nearby FIPS evidence.

## Use cases

| Case | Invocation |
| --- | --- |
| Algorithm anchors | `lmprobe crypto-sites . --algorithm sha1 --algorithm md5` |
| Nearby FIPS-related text | `lmprobe crypto-sites . --algorithm sha1 --fips-check --around 4 --parents --body` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe crypto-sites [OPTIONS] --algorithm <ALGORITHMS> <PATH>
```

| Argument / option | Behavior |
| --- | --- |
| `<PATH>` | Scan root. |
| `--algorithm <ALGORITHMS>` | Algorithm name to anchor, e.g. `sha1`; repeat for multiple names |
| `--fips-check` | Attach nearby FIPS / boringcrypto / GODEBUG / GOFIPS evidence lines |
| `--around <AROUND>` | Include N source lines around algorithm anchors |
| `--parents` | Attach enclosing structural declaration chain |
| `--body` | Attach innermost enclosing structural body |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::CryptoSites](../../crates/lmprobe/src/cli.rs) and `Commands::CryptoSites`; [dispatch: Commands::CryptoSites](../../crates/lmprobe/src/cli.rs).
- [Engine: crypto_sites::crypto_sites](../../crates/lmprobe-engine/src/crypto_sites.rs).
- [Result schema: CryptoSitesResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: crypto_sites](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Reports local syntactic/lexical evidence, not ownership, control flow, authorization, compliance, or data-flow correctness. Preserve source anchors and diagnostics when extending coverage.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [crypto_sites](../../crates/lmprobe/tests/crypto_sites.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test crypto_sites
```

Also check `lmprobe crypto-sites -h` and `lmprobe crypto-sites --help` after option changes.
