# Security policy

TallyCheck holds companies' accounting and payroll data, so we take security reports seriously.

## Supported versions

| Version | Supported |
|---|---|
| 1.x (latest release) | Yes |
| Older | No; please update first |

## Reporting a vulnerability

Please **do not open a public issue** for a security problem.

Report it privately through GitHub: open the repository's **Security** tab and choose
**Report a vulnerability**. Include what you found, the steps to reproduce it, the version and
operating system, and the impact you expect.

We aim to acknowledge a report within 3 working days and to agree a fix and disclosure date
with you. Please give us reasonable time to release a fix before publishing details.

Never send real customer or company data in a report; a made-up company is enough to show a
problem.

## Scope

In scope: the desktop app, the embedded server and its API, the installers and update
mechanism, the Claude (MCP) connector and the online connectors.

Known limitation, documented and not treated as a vulnerability: LAN mode uses plain HTTP inside
the office network ([docs/lan-mode.md](docs/lan-mode.md)).
