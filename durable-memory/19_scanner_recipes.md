# 19 — Evidence Scanners and Recipe Templates

`lmprobe` evidence scanners are command and GraphQL surfaces that collect **mechanical local or repository evidence only** for AI-assisted triage. They do not emit findings, vulnerabilities, or verdict claims.

The contract is:

- `ruleId` is the recipe name, not a bug class verdict.
- `evidenceKind` names the evidence shape, for example `resource_pairing`, `config_key_reference`, `guard_distance`, or `api_route_fact`.
- `summary` must avoid claim words such as `bug`, `vulnerability`, `leak`, `missing auth`, `drift`, and `broken`.
- Anchors must include enough file/line/source/structure for an AI agent to inspect locally before proposing a fix.
- A recipe can rank or group evidence, but semantic judgment stays with the AI/human unless a later recipe documents a deterministic proof boundary.

There is intentionally no `scan` resolver in this first slice. Use GraphQL aliases to batch existing primitive resolvers in one process.

## Evidence-scanner command family (from capability matrix)

| Command | GraphQL field | Envelope schema | Output formats | Language scope | Read/Write |
| --- | --- | --- | --- | --- | --- |
| `resource-sites` | `resourceSites` | `lmprobe.resource_sites.v1` | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| `guard-sites` | `guardSites` | `lmprobe.guard_sites.v1` | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| `async-leaks` | `asyncLeaks` | `lmprobe.async_leaks.v1` | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| `crypto-sites` | `cryptoSites` | `lmprobe.crypto_sites.v1` | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| `config-keys` | `configKeys` | `lmprobe.config_keys.v1` | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| `openapi-facts` | `openapiFacts` | `lmprobe.openapi_facts.v1` | Markdown, JSON, NDJSON, Sarif, Github | source code API facts | Read |
| `route-anchors` | `routeAnchors` | `lmprobe.route_anchors.v1` | Markdown, JSON, NDJSON, Sarif, Github | source code API facts | Read |
| `http-client-calls` | `httpClientCalls` | `lmprobe.http_client_calls.v1` | Markdown, JSON, NDJSON, Sarif, Github | source code API facts | Read |
| `secrets` | `secrets` | `lmprobe.secrets.v1` | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| `secrets-recipes` | — (CLI discovery/execution helper for provider recipes) | `lmprobe.secrets_recipes.v1`, `lmprobe.secrets_recipes.show.v1` | JSON | provider recipe catalog + generated resolver text | Read |
| `secrets-sources` | — (CLI discovery/execution helper for source DB entries) | `lmprobe.secrets_sources.v1`, `lmprobe.secrets_sources.show.v1` | JSON | secret source catalog + generated resolver text | Read |
| `py-slice-candidates` | `—` | `lmprobe.py_slice_candidates.v1` | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| `callsites` | `callsites` | `lmprobe.callsites.v1` | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| `diff-context` | `—` | `lmprobe.diff_context.v1` | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| `query` | n/a (custom query resolver) | `lmprobe.query.v1` | JSON, NDJSON | GraphQL composition layer | Read |
| `serve` | n/a (HTTP interface) | n/a | JSON only (HTTP transport) | GraphQL service | Interface |

## Evidence-scanner command classes and safety

Scanner commands are evidence collectors only. They emit bounded anchors, local context, and diagnostics; they do not assert security findings, flow proof, or policy verdicts.

### Scanner command safety classes

| Safety class | Commands | Side-effect model |
| --- | --- | --- |
| Pure read | `resource-sites`, `guard-sites`, `async-leaks`, `crypto-sites`, `config-keys`, `openapi-facts`, `route-anchors`, `http-client-calls`, `secrets`, `secrets-recipes`, `secrets-sources`, `py-slice-candidates`, `callsites`, `diff-context` | Working tree + inputs only; no writes. |
| Cache-writing read | `none` | No scanner command in this family currently writes cache-backed state. |
| GitHub API read | `none` (scanner-family surface) | API reads are handled on `pr-triage` (`gh pr view`) and are not part of the scanner family contract. |
| Worktree-writing mutation | `none` | Mutations are `trace`, `trace --watch`, and `fix`; not scanner commands. |
| Interface | `query`, `serve` | Resolver/HTTP interface surface; both compose scanner evidence from scanner-family primitives. |

## Scanner read/write table (command-level)

| Command | Read/write class | Evidence behavior |
| --- | --- | --- |
| `resource-sites` | Pure read | Emits `ResourceSite` anchors (acquire/release/defer/error-branch) and scan diagnostics; no cache or mutation side effects. |
| `guard-sites` | Pure read | Emits sensitive-call and nearby guard candidates; empty `nearbyGuards` is an evidence absence signal, not a policy verdict. |
| `async-leaks` | Pure read | Emits call anchors with `awaited` / `returned` flags for teardown-like names; no leak proof. |
| `crypto-sites` | Pure read | Emits algorithm anchors and optional nearby config/build-tag evidence; no compliance verdict. |
| `config-keys` | Pure read | Emits config anchors with optional lexical code anchors from supported source files. |
| `openapi-facts` | Pure read | Emits API operation facts from static specs and parsed route declarations; may emit parse diagnostics. |
| `route-anchors` | Pure read | Emits route handler candidates and optional method/path/handler context. |
| `http-client-calls` | Pure read | Emits literal HTTP call anchors with local context and confidence labels. |
| `secrets` | Pure read | Emits secret-call evidence with command-level scanner diagnostics (shape, context, confidence markers) and warnings when ambiguity is present. |
| `secrets-recipes` | Pure read | Discovery + recipe inspection only; execution remains via `lmprobe query --recipe ...` and `query` aliases. |
| `secrets-sources` | Pure read | Source/provider database discovery and inspection only; execution remains via `query` aliases. |
| `py-slice-candidates` | Pure read | Emits best-effort slice candidates for manual AI triage; low-confidence evidence only. |
| `callsites` | Pure read | Emits callsite-argument anchors and surrounding context; does not infer data-flow correctness. |
| `diff-context` | Pure read | Emits diff hunk context and adjacent AST anchors for review; no edit decisions. |
| `query` | Pure read | Composes any scanner schema fields into one GraphQL request; return envelope remains process-level `query` status semantics. |
| `serve` | Interface | Exposes scanner surfaces over HTTP; does not alter scanner side-effect guarantees. |

## Command mapping (matrix commands)

| Command | GraphQL resolver | Envelope |
| --- | --- | --- |
| `resource-sites` | `resourceSites` | `lmprobe.resource_sites.v1` |
| `guard-sites` | `guardSites` | `lmprobe.guard_sites.v1` |
| `async-leaks` | `asyncLeaks` | `lmprobe.async_leaks.v1` |
| `crypto-sites` | `cryptoSites` | `lmprobe.crypto_sites.v1` |
| `config-keys` | `configKeys` | `lmprobe.config_keys.v1` |
| `openapi-facts` | `openapiFacts` | `lmprobe.openapi_facts.v1` |
| `route-anchors` | `routeAnchors` | `lmprobe.route_anchors.v1` |
| `http-client-calls` | `httpClientCalls` | `lmprobe.http_client_calls.v1` |
| `secrets` | `secrets` | `lmprobe.secrets.v1` |
| `callsites` | `callsites` | `lmprobe.callsites.v1` |

Operational guidance:

- Use `warnings` and `exit_status` in envelopes to separate real evidence absence from scan-path failure.
- Do not repurpose scanner rows into issue labels; feed the rows forward to a follow-up graph or patch plan with explicit uncertainty boundaries.
- Keep scan recipes narrow by command args (`--around`, `--role`, `--framework`, `--all-shapes`) to limit false positives before triage decisions.

## Resource Pairing Evidence

Use this when looking for file/temp/lock/worker/transaction cleanup evidence. The output does not prove a resource leak; it shows local acquire/release/defer/error-return anchors grouped by enclosing declaration. Current evidence covers Go, Rust, JavaScript, TypeScript, and TSX, including syntactic lazy acquire, dynamic import, optional-chained release lines, and page/dispose lifecycle release labels when locally visible.

```graphql
query ResourcePairing($path: String!) {
  resourcePairs: resourceSites(
    path: $path,
    acquire: "os\\.Open|os\\.Create|File::open|File::create",
    release: "Close|Remove|RemoveAll|drop|rollback",
    around: 4,
    parents: true,
    body: true
  ) {
    sites {
      filePath
      enclosing { kind name lineStart lineEnd }
      acquireCalls { matchText: callee lineNumber lineContent }
      releaseCalls { matchText: callee lineNumber lineContent }
      deferCalls { matchText: callee lineNumber lineContent }
      errorBranches { matchText: callee lineNumber lineContent }
      acquireTiming
      releaseLifecycle
      optionalAcquire
      sourceContext { lineNumber lineContent }
      confidence
    }
    warnings { code message hint }
    exitStatus
  }
}
```

AI follow-up: inspect ownership transfer, RAII/drop behavior, helper cleanup calls, and error paths before editing.

## Async Teardown Evidence

Use this when looking for JavaScript/TypeScript teardown calls that may need `await` or return propagation. The output does not prove a dropped Promise or resource leak; it only reports call anchors for names the agent supplies and whether the anchor is locally awaited or returned.

```graphql
query AsyncTeardownEvidence($path: String!) {
  teardown: asyncLeaks(
    path: $path,
    call: ["stop", "close", "dispose", "terminate"],
    around: 3,
    parents: true,
    body: true
  ) {
    sites {
      filePath
      lineNumber
      lineContent
      callee
      awaited
      returned
      parents { kind name lineStart lineEnd }
      sourceContext { lineNumber lineContent }
      confidence
    }
    warnings { code message hint }
    exitStatus
  }
}
```

AI follow-up: inspect the callee implementation or type declarations before editing; a non-awaited call is only suspicious when the callee actually returns a Promise-like cleanup result.

## Crypto Algorithm Evidence

Use this when looking for Go crypto algorithm anchors and nearby FIPS/config/build-tag evidence. The output does not prove insecure usage or FIPS non-compliance; it only reports requested algorithm anchors, import paths, and local guard/config mentions for review.

```graphql
query CryptoEvidence($path: String!) {
  crypto: cryptoSites(
    path: $path,
    algorithms: ["sha1", "md5", "des"],
    fipsCheck: true,
    around: 4,
    parents: true,
    body: true
  ) {
    sites {
      filePath
      lineNumber
      lineContent
      algorithm
      matchText
      importPaths
      nearbyGuards { matchText lineNumber lineContent }
      parents { kind name lineStart lineEnd }
      sourceContext { lineNumber lineContent }
      confidence
    }
    warnings { code message hint }
    exitStatus
  }
}
```

AI follow-up: inspect build tags, runtime config, imports, wrappers, and policy requirements before proposing any security change.

## Config/Code Reference Evidence

Use this when looking for config keys and lexical code references in one workspace. No code anchor means no lexical anchor found under the current scan, not unused config.

Use `key: "full.path.to.key"` when one config key matters; this filters before lexical code anchoring and avoids broad key-limit truncation.

```graphql
query ConfigCodeEvidence($path: String!, $key: String) {
  config: configKeys(
    path: $path,
    key: $key,
    grepCode: true,
    around: 2,
    parents: true,
    role: true
  ) {
    keys {
      filePath
      lang
      keyPath
      key
      lineNumber
      valueKind
      valueSummary
      codeAnchors {
        filePath
        lineNumber
        lineContent
        role
        parents { kind name lineStart lineEnd }
        sourceContext { lineNumber lineContent }
      }
    }
    warnings { code message hint }
    exitStatus
  }
}
```

AI follow-up: distinguish real config reads from coincidental strings, generated files, docs, and dynamic/interpolated keys.

## Permission-Guard Evidence

Use this when collecting local evidence around sensitive actions. The output does not prove authorization is present or absent. Use `scanStats` and warnings to distinguish no scanned files, no sensitive anchors, and sensitive anchors without nearby guard evidence.

```graphql
query GuardEvidence($path: String!) {
  guards: guardSites(
    path: $path,
    sensitive: "Delete|Update|Create|Chmod|SaveUploadedFile|exec|eval",
    guard: "auth|authorize|permission|allowed|can|rbac|middleware",
    around: 8,
    parents: true,
    body: true
  ) {
    candidates {
      filePath
      sensitive { matchText lineNumber lineContent }
      nearbyGuards { matchText lineNumber lineContent }
      decorations { matchText lineNumber lineContent }
      parents { kind name lineStart lineEnd }
      body { kind name lineStart lineEnd content }
      sourceContext { lineNumber lineContent }
      confidence
    }
    scanStats { filesScanned anchorsFound guardHits unsupportedFiles }
    warnings { code message hint }
    exitStatus
  }
}
```

AI follow-up: inspect middleware, router groups, decorators, callers, and framework dependency injection before proposing a security fix.

## API Surface Evidence

Use this when comparing API specs, producer routes, and consumer client calls. The streams are separate on purpose; matching and compatibility reasoning is AI-owned.

```graphql
query ApiSurfaceEvidence($path: String!, $spec: String!) {
  spec: openapiFacts(path: $spec) {
    operations {
      filePath
      method
      path
      operationId
      requiredParams
      responseSchemas
    }
    warnings { code message hint }
    exitStatus
  }
  routes: routeAnchors(
    path: $path,
    framework: "fastapi",
    around: 2,
    parents: true,
    body: true
  ) {
    routes {
      filePath
      method
      path
      lineNumber
      lineContent
      handler
      parents { kind name lineStart lineEnd }
      sourceContext { lineNumber lineContent }
      confidence
    }
    warnings { code message hint }
    exitStatus
  }
  clients: httpClientCalls(path: $path, around: 2, parents: true, body: true) {
    calls {
      filePath
      method
      url
      lineNumber
      lineContent
      client
      parents { kind name lineStart lineEnd }
      sourceContext { lineNumber lineContent }
      confidence
    }
    warnings { code message hint }
    exitStatus
  }
}
```

AI follow-up: normalize route templates, account for generated clients, inspect dynamic route construction, and compare schemas only after collecting source context.

## Promotion Rule

Add a `scan(path:, profile:)` abstraction only after a recipe has:

- a stable evidence schema,
- measured false-positive boundaries,
- enough anchors for AI fix agents without rerunning broad scans,
- docs stating exactly what the recipe does not prove.

Until then, GraphQL aliases are the scanner contract.
