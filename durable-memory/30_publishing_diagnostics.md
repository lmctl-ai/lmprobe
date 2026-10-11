# Publishing diagnostics and trusted publisher configuration

Keep package creation, artifact validation, authentication and registry
authorization failures separate. A correct native artifact can still fail to
publish because the registry publisher configuration does not match the workflow.

## Trusted publisher identity

For the current public project, all six npm packages use the GitHub owner
`lmctl-ai`, repository `lmprobe`, workflow `release-npm.yml`, with `npm publish`
as the publishing operation. Each platform package and the meta package need
their own trusted publisher record. The GitHub owner and npm scope happen to
match here; that equality is not a general rule of trusted publishing.

When a workflow's repository identity changes, recheck every package's publisher
record. Successful configuration elsewhere does not verify the current packages.
Package names must exist before their publisher records can be attached; first
bootstrap and subsequent trusted publication are distinct workflows.

## Diagnose failures before changing credentials

- E404/permission failures at `npm publish` can indicate publisher authorization,
  not an absent tarball or broken native build. Check the failing stage first.
- Verify workflow `id-token: write` permission, the package-specific publisher
  identity, the selected publishing environment, and the current npm version.
- Remove a setup-node placeholder `_authToken` entry when the workflow expects
  OIDC; an unintended token configuration can prevent the expected exchange.
- Preserve the intended trusted-publishing path. Do not add long-lived tokens or
  replace the normal workflow with repeated manual publishing to hide a failure.
- Validate package platform metadata, executable permissions, optional dependency
  versions and launcher exit behavior before attributing failures to authorization.

The release workflows are manual-only and default to dry run. Neither a push nor
an archive/migration verification task authorizes real npm publication. Reproduce
registry publication only as an explicitly requested release task. Avoid storing
tokens, account details, registry authentication files or incident transcripts
in contributor documentation.

See [release provenance](27_release_provenance.md) and
[npm distribution](npm_distribution.md) for the actual scripts and workflows.
