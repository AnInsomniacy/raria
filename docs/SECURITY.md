# Security Policy

Report suspected vulnerabilities privately through GitHub Security Advisories.
Do not open a public issue for credential exposure, authentication bypass,
path traversal, arbitrary file write, command execution, malicious archive
behavior, or secret leakage.

## Supported Surface

Security support covers the current branch and the latest published release.
Reports should target raria-native surfaces:

- CLI and daemon runtime
- `raria.toml`
- `/api/v1` HTTP JSON resources
- `/api/v1/events` WebSocket events
- bearer-token authentication
- structured logs and redaction
- native persistence and restore
- release archives and checksums

JSON-RPC, XML-RPC, aria2 compatibility, legacy session files, old option names,
AriaNg adapters, and Motrix legacy adapters are not supported surfaces.

## Reporting Guidance

Include the raria version, platform, affected command or API route, reproduction
steps, expected security boundary, observed bypass, and any proof of impact.
Redact secrets unless the secret value itself is necessary to prove the issue.

## Daemon Boundary

raria's native API is intended to be bound to trusted interfaces and protected
with bearer authentication when exposed beyond a local trusted process. Treat
the bearer token as a password. Do not place it in URLs, shell history,
screenshots, public logs, or issue comments.

## Logs and Artifacts

Structured logs are designed to redact common credential-bearing URLs and
headers on covered paths. A redaction bug is a security issue. Release archives
are currently distributed with SHA-256 checksum files; code signing and
installer notarization are not part of the current release contract.
