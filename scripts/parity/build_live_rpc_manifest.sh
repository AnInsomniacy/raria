#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="$ROOT/.omx/parity/generated"

mkdir -p "$OUT_DIR"

ruby - "$ROOT" "$OUT_DIR" <<'RUBY'
require "json"
require "yaml"
require "time"

root = ARGV[0]
out_dir = ARGV[1]

methods_src = File.read(File.join(root, "crates/raria-rpc/src/methods.rs"))
events_src = File.read(File.join(root, "crates/raria-rpc/src/events.rs"))

methods = methods_src.scan(/#\[method\(name = "([^"]+)"\)\]/).flatten.sort.uniq
methods |= ["system.listMethods", "system.listNotifications", "system.multicall"]

parity_block = events_src[/pub const PARITY_NOTIFICATION_METHODS: &\[&str\] = &\[(.*?)\];/m, 1] || ""
extension_block = events_src[/pub const EXTENSION_NOTIFICATION_METHODS: &\[&str\] = &\[(.*?)\];/m, 1] || ""
notifications = (parity_block.scan(/"([^"]+)"/).flatten + extension_block.scan(/"([^"]+)"/).flatten).sort.uniq

manifest = {
  "generated_at" => Time.now.utc.iso8601,
  "methods" => methods,
  "notifications" => notifications,
  "source" => "repository_source_scan",
  "evidence_level" => "source_scan",
  "manifest_notes" => [
    "Generated from current RPC declarations in the repository tree.",
    "This artifact is a source-derived baseline, not a runtime probe.",
    "aria2.onSourceFailed remains an extension-class surface pending formal policy promotion."
  ],
}

baseline_entries = []
methods.each do |symbol|
  baseline_entries << {
    "generated_at" => manifest["generated_at"],
    "surface_id" => "rpc.#{symbol.downcase.gsub(/[^a-z0-9]+/, '.')}",
    "surface_type" => "rpc_method",
    "symbol" => symbol,
    "generated_manifest_ref" => ".omx/parity/generated/exported-surface-manifest.yaml",
    "policy_class" => "aria2_parity_surface",
    "parity_claim_state" => "provisional",
    "classification_basis" => "legacy_anchor",
    "legacy_anchor" => "aria2-legacy/doc/manual-src/en/aria2c.rst",
  }
end

notifications.each do |symbol|
  entry = {
    "generated_at" => manifest["generated_at"],
    "surface_id" => "ws.notification.#{symbol.downcase.gsub(/[^a-z0-9]+/, '.')}",
    "surface_type" => "rpc_notification",
    "symbol" => symbol,
    "generated_manifest_ref" => ".omx/parity/generated/exported-surface-manifest.yaml",
  }

  if symbol == "aria2.onSourceFailed"
    entry["policy_class"] = "raria_extension_surface"
    entry["parity_claim_state"] = "provisional"
    entry["classification_basis"] = "manual_adjudication"
    entry["extension"] = true
    entry["notes"] = "Extension surface until formal exported-surface policy promotes it and a confirmed legacy anchor exists."
  else
    entry["policy_class"] = "aria2_parity_surface"
    entry["parity_claim_state"] = "provisional"
    entry["classification_basis"] = "legacy_anchor"
    entry["legacy_anchor"] = "aria2-legacy/src/WebSocketSessionMan.cc"
  end

  baseline_entries << entry
end

File.write(File.join(out_dir, "live-rpc-methods.json"), JSON.pretty_generate(methods))
File.write(File.join(out_dir, "live-rpc-notifications.json"), JSON.pretty_generate(notifications))
File.write(File.join(out_dir, "exported-surface-manifest.yaml"), YAML.dump(manifest))
File.write(
  File.join(out_dir, "exported-surface-policy-baseline.yaml"),
  YAML.dump({"generated_at" => manifest["generated_at"], "entries" => baseline_entries})
)
RUBY

printf 'wrote %s\n' "$OUT_DIR"
