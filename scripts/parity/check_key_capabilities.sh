#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

ruby - "$ROOT" <<'RUBY'
require "yaml"

root = ARGV[0]
capabilities = YAML.load_file(File.join(root, ".omx/parity/capability-matrix.yaml")).fetch("capabilities")
tests = YAML.load_file(File.join(root, ".omx/parity/test-ledger.yaml")).fetch("tests")

test_map = tests.group_by { |entry| entry.fetch("capability_id") }

capabilities.each do |entry|
  next unless entry.fetch("key_capability")
  next if entry.fetch("rubric") == "ExplicitlyExcluded"
  capability_id = entry.fetch("capability_id")
  mapped = test_map[capability_id]
  abort("missing test mapping for key capability #{capability_id}") if mapped.nil? || mapped.empty?
end

puts "key capabilities mapped"
RUBY
