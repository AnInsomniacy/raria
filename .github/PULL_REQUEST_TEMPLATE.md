<!--
Before submitting, read CONTRIBUTING.md and the relevant docs under docs/.

PR titles should use concise Conventional Commit style:
  fix: preserve native task state during restart
  feat(api): add task source health projection
  docs: add native API integration guide
  ci: tighten release checksum upload
-->

## Summary

<!-- What changed, and why? Link related issues when applicable. -->

## Scope

<!-- Name the affected crate, documentation area, workflow, script, or public surface. -->

## Public Contract Impact

<!-- State whether this changes CLI flags, raria.toml keys, /api/v1 routes, event fields, persistence schemas, release artifacts, or user documentation. Write "none" if there is no public contract change. -->

## Verification

<!-- Paste the exact commands run and their result. "It compiles" is not enough. -->

```bash
cargo fmt --all --check
cargo check --workspace --locked
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Checklist

- [ ] The change keeps raria-native CLI, configuration, API, event, and persistence names.
- [ ] No JSON-RPC, aria2 method name, aria2 option name, Gid-facing public behavior, old session format, or legacy client adapter was added.
- [ ] Tests are focused on durable behavior and do not rely on public-network availability.
- [ ] Documentation was updated for changed routes, fields, flags, config keys, release artifacts, or validation claims.
- [ ] Temporary output, logs, session stores, downloads, and generated archives are absent or kept under ignored `var/`.

## Release Notes

<!-- One concise user-facing sentence, or "none" for internal-only changes. -->
