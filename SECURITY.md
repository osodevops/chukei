# Security Policy

Security reports are handled privately and with priority.

## Supported Versions

| Version | Supported |
|---|---|
| Latest release | Yes |
| Older releases | Best effort |

## Reporting a Vulnerability

Do not open a public GitHub issue for security reports.

Preferred path:

1. Use GitHub private vulnerability reporting for `osodevops/chukei` when it is
   available.
2. If that is unavailable, email `security@oso.sh`.
3. If email bounces, use `enquiries@oso.sh` with the subject prefix
   `[chukei security]`.

Please include:

- affected version or commit SHA;
- reproduction steps;
- expected impact;
- whether credentials, customer query text, or evidence bundles were exposed;
- any proposed patch or mitigation.

## Response Targets

| Severity | Initial response | Target fix window |
|---|---:|---:|
| Critical | 24 hours | 72 hours |
| High | 48 hours | 7 days |
| Medium | 5 business days | 30 days |
| Low | 10 business days | Next planned release |

## Security Boundaries

chukei's core production invariants are:

- fail open to Snowflake when chukei cannot make a safe decision;
- never store or log client credentials;
- never cache unsafe or non-deterministic query results;
- treat cache false positives as security bugs;
- keep evidence signatures verifiable and tamper-evident.

Reports that violate those invariants should be treated as security reports,
even if they do not expose traditional remote-code-execution or privilege
escalation behavior.
