# P2 gitleaks parity result

Command:

```bash
cargo test -p lmprobe-secrets --test gitleaks_parity -- --nocapture
```

Corpus:

- `$LMPROBE_GITLEAKS_REPO/testdata`
- Synthetic durable-memory/003-style temp corpus created by the test:
  source indirection, external `key1.txt`, and a high-entropy hardcoded
  `api_key` assignment.

Methodology:

- Gitleaks side runs the real
  `$LMPROBE_GITLEAKS_REPO/gitleaks dir <corpus> --config <config> --report-format json --redact=0`.
- lmprobe side runs the shipped CLI path via
  `cargo run -q -p lmprobe -- --format json secrets --gitleaks-compat --config <config> <corpus>`.
- Normalization is strict on `(rule_id, file_path, start_line, secret, match_text)`.
  The raw secret/match from lmprobe are hydrated in-memory from the same
  `Layer1Engine` used by the shipped CLI because the JSON envelope redacts
  `Secret` values by default. Raw secrets are not written to snapshots or files.

Threshold: `1.000` normalized parity.

Latest observed result during the P2.1 re-fix:

```text
gitleaks=18 ours=18 matched=18 missed=0 extra=0 parity=1.000
```

Deferred-scope deltas: none on this corpus. Decoders, git history,
`.gitleaksignore`, baselines, and inline `gitleaks:allow` remain out of P2
scope and are not exercised by this parity fixture.
