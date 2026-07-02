# Security Policy

rspark is a learning project. It does not yet have a published release with a
formal security SLA. That said, if you find a security issue, please report it
responsibly.

## Supported versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |
| < 0.1   | :x:                |

## Reporting a vulnerability

Please **do not** open a public GitHub issue for security problems.

Instead, email the maintainers at the address listed on the GitHub profile
of the repository owner. Include:

- A clear description of the issue and its impact.
- Reproduction steps, ideally with a minimal SQL or HTTP request.
- The commit / tag / commit hash you reproduced against.
- Whether you intend to disclose publicly and on what timeline.

We will:

1. Acknowledge the report within 3 business days.
2. Investigate and confirm the issue.
3. Coordinate a fix and a release.
4. Credit you in the release notes (unless you prefer to remain anonymous).

## Scope

The following are in scope:

- Anything that lets a remote attacker read, modify, or destroy data on a
  running rspark master or worker, or impersonate one to another.
- Anything that lets a malicious SQL query escape the executor sandbox (it
  isn't a sandbox yet, but no future change should regress that).
- Path traversal in `DataSource::scan` or `OutputWriter`.

Out of scope:

- Bugs in dependencies (`sqlparser-rs`, `axum`, `tokio`, etc.) — please report
  them upstream.
- Denial of service against a single-user local install.

## Disclosure policy

We follow a 90-day coordinated disclosure timeline. If a fix is going to take
longer than that, we will discuss an extension with the reporter.
