# Security policy

## Supported versions

Before 1.0, only the latest released minor version line receives security
fixes. Older lines are not patched.

| Version | Supported |
| --- | --- |
| Latest minor line (the most recent release listed in [`CHANGELOG.md`](CHANGELOG.md), e.g. `0.x`) | Yes |
| Older lines | No |

## Reporting a vulnerability

Report vulnerabilities only through GitHub private vulnerability reporting:
<https://github.com/smart-ssh/smart_ssh/security/advisories/new>
(Security tab → "Report a vulnerability").

**Do not open a public issue, pull request or discussion for a vulnerability.**

A good report contains:

- the affected version and platform,
- steps to reproduce, or a proof of concept,
- the impact,
- whether the issue is already public.

## What to expect

- Acknowledgement within 3 business days.
- An initial assessment (accepted / not a vulnerability / needs more
  information) within 10 business days.
- Status updates at least every 14 days until the report is resolved.
- A fix for a confirmed critical or high finding is released as a patch of the
  supported line as soon as it is ready.
- Reporters are credited in the advisory unless they ask not to be.

## Scope

Anything that breaks a promise of [`THREAT-MODEL.md`](THREAT-MODEL.md), in
particular:

- a command running without the confirmation that the filter engine or a
  confirmation boundary requires (bypass of rules, hard blacklist, chaining
  protection or red-risk confirmation);
- secrets (keys, passwords, tokens) reaching the AI provider, logs or the
  database in clear text where the threat model says they must not;
- credential or keychain exposure;
- unauthenticated or cross-client access to the local MCP endpoint;
- path escape or unconfirmed writes in the file browser;
- host key verification bypass;
- content from a server or web page causing execution without confirmation.

## Out of scope

- The accepted residual risks listed in
  [`THREAT-MODEL.md`, section 6](THREAT-MODEL.md#6-known-residual-risks).
- Attacks that need an already compromised local machine or user account.
- Behaviour of third-party AI providers or models.
- Social engineering.
- Missing hardening without a concrete exploit.
- Vulnerabilities in dependencies that are already tracked as advisories and
  accepted in [`.cargo/audit.toml`](.cargo/audit.toml), unless you show they
  are exploitable in Smart SSH.

## Disclosure

We practise coordinated disclosure. The fix and a GitHub Security Advisory are
published together. The default embargo is up to 90 days after the report;
shorter when a fix is already out, longer only by agreement.
