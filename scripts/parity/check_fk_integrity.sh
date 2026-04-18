#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

ruby - "$ROOT" <<'RUBY'
require "set"
require "yaml"

root = ARGV[0]

capabilities = YAML.load_file(File.join(root, ".omx/parity/capability-matrix.yaml")).fetch("capabilities")
fixtures = YAML.load_file(File.join(root, ".omx/parity/fixtures.yaml")).fetch("fixtures")
parity = YAML.load_file(File.join(root, ".omx/parity/parity-ledger.yaml")).fetch("entries")
tests = YAML.load_file(File.join(root, ".omx/parity/test-ledger.yaml")).fetch("tests")
surfaces = YAML.load_file(File.join(root, ".omx/parity/exported-surface.yaml")).fetch("surfaces")

capability_ids = capabilities.map { |entry| entry.fetch("capability_id") }.to_set
fixture_ids = fixtures.map { |entry| entry.fetch("fixture_id") }.to_set

parity.each do |entry|
  abort("parity-ledger missing capability FK: #{entry.inspect}") unless capability_ids.include?(entry.fetch("capability_id"))
end

tests.each do |entry|
  abort("test-ledger missing capability FK: #{entry.inspect}") unless capability_ids.include?(entry.fetch("capability_id"))
  abort("test-ledger missing fixture FK: #{entry.inspect}") unless fixture_ids.include?(entry.fetch("fixture_id"))
end

surfaces.each do |entry|
  entry.fetch("capability_ids").each do |capability_id|
    abort("exported-surface missing capability FK: #{entry.inspect}") unless capability_ids.include?(capability_id)
  end
end

puts "foreign-key integrity ok"
RUBY
