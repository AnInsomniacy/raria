# Local Generated Workspace

`var/` is the single root-level location for local generated files produced
while developing, testing, or smoke-validating raria.

Use this directory for temporary downloads, session databases, logs, packet
captures, benchmark output, smoke-test payloads, scratch fixtures, and other
local runtime artifacts. Keep source code, committed test code, documentation,
and stable fixtures outside `var/`.

The repository ignores everything under `var/` except this README.
