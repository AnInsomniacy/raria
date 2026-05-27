<!--
Before submitting, read CONTRIBUTING.md and the relevant docs under docs/.

PR titles should use concise Conventional Commit style:
  fix: preserve native task state during restart
  feat(api): add task source health projection
  docs: add native API integration guide
  ci: tighten release checksum upload
-->

## Summary

<!-- What changed, why, and which issue does it close? -->

## Contract impact

<!-- Note changed CLI flags, raria.toml keys, /api/v1 routes, events, persistence, release artifacts, or docs. Write "none" if unchanged. -->

## Verification

<!-- Paste exact commands and results. -->

## Checklist

- [ ] The change keeps raria-native CLI, configuration, API, event, and persistence names.
- [ ] No JSON-RPC, aria2 method or option name, public Gid behavior, old session format, or legacy client adapter was added.
- [ ] Tests are focused, local, and necessary.
- [ ] Documentation changed with any public contract change.
- [ ] Generated output is absent or kept under ignored `var/`.

## Release note

<!-- One user-facing sentence, or "none". -->
