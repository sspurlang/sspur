# Security policy

## Reporting a vulnerability

Please don't open a public issue for security problems. Report them privately through GitHub's security advisories:

1. Go to the [Security tab](https://github.com/sspurlang/sspur/security) of the repository.
2. Choose **Report a vulnerability** and describe the issue, the affected version (`sspur --version`), and how to reproduce it.

The report goes only to me, the maintainer. I'll acknowledge it within a week, keep you updated while I work on a fix, and credit you in the advisory unless you'd rather stay anonymous.

## Supported versions

SSPUR is pre-1.0. Fixes go into the latest release only.

| Version | Supported |
|---|---|
| 0.15.x | Yes |
| < 0.15 | No |

## Verifying release files

Every file attached to a release after 0.15 has a build provenance attestation from the release workflow, and each `sspur` archive also has an SBOM attestation for the release's `sspur-vX.Y.Z.cdx.json` (CycloneDX, every Rust crate in the binary). Both are signed through Sigstore with the workflow's GitHub identity, so no key is involved. Check a file with the GitHub CLI (2.68 or newer, signed in):

```console
gh attestation verify sspur-v1.0.0-aarch64-apple-darwin.tar.gz --repo sspurlang/sspur \
  --signer-workflow sspurlang/sspur/.github/workflows/release.yml --source-ref refs/tags/v1.0.0
gh attestation verify sspur-v1.0.0-aarch64-apple-darwin.tar.gz --repo sspurlang/sspur \
  --predicate-type https://cyclonedx.org/bom
```

`install.sh` runs the first check itself when `gh` is installed and signed in, and refuses to install on a failure (`SSPUR_SKIP_ATTESTATION=1` turns it off); without `gh` it checks `SHA256SUMS` only. `install.ps1` checks `SHA256SUMS` only. [Installation](https://sspurlang.github.io/sspur/get-started/installation.html#verifying-a-release) has the details. A file that fails these checks is worth a report.

## Scope

Things I'd especially like to hear about:

- Native code that behaves differently from the interpreter in a way that breaks memory safety, a contract, or an effect boundary
- Programs outside `unsafe` that the ownership or race checker accepts but that corrupt memory
- IAM policies from `sspur deploy` that grant more than a service's effects require
- The sync server or MCP server accepting input it shouldn't
- Problems in `install.sh` or the release artifacts

Utkarsha Vardhana
