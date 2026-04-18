#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

ruby - "$ROOT" <<'RUBY'
require "yaml"

root = ARGV[0]
files = {
  ".omx/parity/capability-matrix.yaml" => "capabilities",
  ".omx/parity/parity-ledger.yaml" => "entries",
  ".omx/parity/test-ledger.yaml" => "tests",
  ".omx/parity/exported-surface.yaml" => "surfaces",
  ".omx/parity/fixtures.yaml" => "fixtures",
  ".omx/parity/claim-policy.yaml" => "claims",
  ".omx/parity/ownership-decisions.yaml" => "decisions",
  ".omx/parity/phase-exit-policy.yaml" => "phases",
}

files.each do |rel, top_key|
  path = File.join(root, rel)
  abort("missing schema file: #{rel}") unless File.exist?(path)
  data = YAML.load_file(path)
  abort("missing top-level key #{top_key} in #{rel}") unless data.is_a?(Hash) && data.key?(top_key)
end

puts "ledger schema ok"
RUBY
