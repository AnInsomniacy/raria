# Support

Use GitHub Issues for reproducible bugs and GitHub Discussions for questions,
integration planning, and troubleshooting that has no confirmed product defect.

## Before Opening an Issue

Search existing issues first. Then capture the smallest command, configuration,
API request, or event sequence that reproduces the behavior.

Useful evidence includes:

- `raria --version`
- platform and CPU architecture
- exact CLI command or `/api/v1` request
- relevant `raria.toml` settings with secrets redacted
- task lifecycle, `taskId`, route, event type, and native error code
- concise structured logs with tokens, cookies, passwords, and private keys
  removed
- checksum output when reporting release archive or data-integrity issues

Do not upload private downloads, private session databases, bearer tokens,
cookies, passwords, SSH keys, or unredacted `.netrc` files.

## Issue Types

Use Bug Report for reproducible runtime, transfer, CLI, daemon, native API, or
documentation defects.

Use Crash or Hang Report for panics, stuck daemon shutdown, deadlocks, or
unresponsive native API behavior.

Use Feature Request for raria-native behavior. Requests that require JSON-RPC,
aria2 method names, aria2 option names, old storage formats, AriaNg adapters,
or legacy Motrix adapters are outside the product contract.

Use Build or Packaging Issue for Cargo builds, CI, release archives, target
triples, and checksum problems.

## Public Network Reports

Public-network behavior can be useful evidence, but it is rarely enough by
itself. When possible, include a local fixture, a reduced server behavior, or a
focused repository test that demonstrates the same problem without depending on
external availability.
