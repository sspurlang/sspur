# Security policy

## Reporting a vulnerability

Please don't open a public issue for security problems. Report them privately through GitHub's security advisories:

1. Go to the [Security tab](https://github.com/utkarshavardhana/sspur/security) of the repository.
2. Choose **Report a vulnerability** and describe the issue, the affected version (`sspur --version`), and how to reproduce it.

The report goes only to me, the maintainer. I'll acknowledge it within a week, keep you updated while I work on a fix, and credit you in the advisory unless you'd rather stay anonymous.

## Supported versions

SSPUR is pre-1.0. Fixes go into the latest release only.

| Version | Supported |
|---|---|
| 0.10.x | Yes |
| < 0.10 | No |

## Scope

Things I'd especially like to hear about:

- Native code that behaves differently from the interpreter in a way that breaks memory safety, a contract, or an effect boundary
- Programs outside `unsafe` that the ownership or race checker accepts but that corrupt memory
- IAM policies from `sspur deploy` that grant more than a service's effects require
- The sync server or MCP server accepting input it shouldn't
- Problems in `install.sh` or the release artifacts

Utkarsha Vardhana
