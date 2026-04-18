#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

ruby - "$ROOT" <<'RUBY'
require "json"
require "yaml"
require "set"

root = ARGV[0]

def surface_key(entry)
  [entry.fetch("surface_type"), entry.fetch("symbol")]
end

def detect_duplicates!(entries, label)
  duplicates = entries
    .group_by { |entry| surface_key(entry) }
    .select { |_key, grouped| grouped.length > 1 }
    .map do |(surface_type, symbol), grouped|
      ids = grouped.map { |entry| entry.fetch("surface_id") }.uniq.sort
      "#{label}: #{surface_type} #{symbol} => #{ids.join(', ')}"
    end

  abort("duplicate exported surfaces detected: #{duplicates.join(' | ')}") unless duplicates.empty?
end

methods = JSON.parse(File.read(File.join(root, ".omx/parity/generated/live-rpc-methods.json"))).to_set
notifications = JSON.parse(File.read(File.join(root, ".omx/parity/generated/live-rpc-notifications.json"))).to_set
policy = YAML.load_file(File.join(root, ".omx/parity/exported-surface.yaml"))
baseline_ref = policy.fetch("baseline_ref")
baseline_path = File.join(root, baseline_ref)
abort("missing exported-surface baseline #{baseline_ref}") unless File.exist?(baseline_path)

surfaces = YAML.load_file(baseline_path).fetch("entries")
detect_duplicates!(surfaces, "baseline")
baseline_source_failed = surfaces.find { |entry| entry.fetch("symbol") == "aria2.onSourceFailed" }
abort("baseline must declare aria2.onSourceFailed") unless baseline_source_failed
abort("baseline aria2.onSourceFailed must be extension surface") unless baseline_source_failed.fetch("policy_class") == "raria_extension_surface"
abort("baseline aria2.onSourceFailed must remain provisional before formal override") unless baseline_source_failed.fetch("parity_claim_state") == "provisional"

policy.fetch("surfaces").each do |entry|
  existing = surfaces.index { |surface| surface_key(surface) == surface_key(entry) }
  if existing
    merged = surfaces[existing].merge(entry)
    merged["surface_id"] = entry.fetch("surface_id")
    surfaces[existing] = merged
  else
    surfaces << entry
  end
end
detect_duplicates!(surfaces, "merged")

declared_methods = surfaces.select { |entry| entry.fetch("surface_type") == "rpc_method" }.map { |entry| entry.fetch("symbol") }.to_set
declared_notifications = surfaces.select { |entry| entry.fetch("surface_type") == "rpc_notification" }.map { |entry| entry.fetch("symbol") }.to_set

missing_methods = methods - declared_methods
abort("missing declared RPC surfaces: #{missing_methods.to_a.sort.join(', ')}") unless missing_methods.empty?

missing_notifications = notifications - declared_notifications
abort("missing declared notification surfaces: #{missing_notifications.to_a.sort.join(', ')}") unless missing_notifications.empty?

abort("generated methods missing system.listMethods") unless methods.include?("system.listMethods")
abort("generated notifications missing aria2.onBtDownloadComplete") unless notifications.include?("aria2.onBtDownloadComplete")

source_failed = surfaces.find { |entry| entry.fetch("symbol") == "aria2.onSourceFailed" }
abort("aria2.onSourceFailed must be extension surface") unless source_failed && source_failed.fetch("policy_class") == "raria_extension_surface"
abort("aria2.onSourceFailed must reach ready state in formal exported surface") unless source_failed.fetch("parity_claim_state") == "ready"

puts "exported surface ok"
RUBY
