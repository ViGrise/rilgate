# Security policy

## Supported versions

Security fixes are released for the latest minor version. Before 1.0, each minor
release may contain breaking changes; see the
[release notes](https://github.com/ViGrise/rilgate/releases) for migration notes.

| Version | Supported |
| --- | --- |
| Latest 0.x minor | Yes |
| Older releases | No |

## Reporting a vulnerability

Report vulnerabilities privately through
[GitHub private vulnerability reporting](https://github.com/ViGrise/rilgate/security/advisories/new).
Do not open a public issue, pull request or discussion for a suspected
vulnerability.

Include the affected version or commit, the configuration involved (with secrets
removed), steps to reproduce, and the impact you observed. A proof of concept
helps, but keep it to what demonstrates the issue.

You can expect an acknowledgement within 5 business days and a status update at
least every 14 days until the report is resolved. Confirmed issues are fixed in a
release and disclosed through a GitHub security advisory, with credit to the
reporter unless they ask otherwise.

## Scope

In scope:

- Bypassing the proof-of-work gate, policy decisions or authentication cookies
- Forging, replaying or extending proof, receipt or authentication tokens beyond
  the documented semantics in [production operations](docs/production.md#key-rotation-and-token-lifetime)
- Request smuggling, header injection or cache poisoning through the gateway
- Exposure of signing keys, secrets or operational routes on public listeners
- Memory safety, denial of service with small inputs, or resource exhaustion that
  bypasses the documented quotas
- Vulnerabilities in the published binaries, container images or install script

Out of scope:

- Behavior documented as intended, such as stateless reusable tokens within their
  signed lifetime or quotas that are local to an instance
- Vulnerabilities that require a compromised host, operator configuration or
  signing key
- Volumetric denial of service against a deployment
- Findings that only affect unsupported versions

## Dependency advisories

CI and a daily scheduled job check the lockfile against the RustSec advisory
database with `cargo deny`. Exceptions are recorded in `.config/deny.toml` and
described in [production operations](docs/production.md#release-rollout-and-rollback).
