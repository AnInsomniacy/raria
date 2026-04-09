# Option Tier Classification

> Updated: 2026-04-09

## Tier A: Must Complete (blocks "can replace aria2")

These options must work correctly for AriaNg/Motrix to function.

| Option | aria2 flag | Area | Status |
|--------|-----------|------|--------|
| `dir` | `--dir` | Core | ✅ done |
| `out` | `--out` | Core | ✅ done |
| `split` | `--split` | Core | 🔧 hardcoded to 16 |
| `max-connection-per-server` | `--max-connection-per-server` | Core | 🔧 hardcoded to 16 |
| `max-concurrent-downloads` | `--max-concurrent-downloads` | Core | ✅ done (scheduler) |
| `max-overall-download-limit` | `--max-overall-download-limit` | Core | ✅ done (governor) |
| `max-download-limit` | `--max-download-limit` | Core | 🔧 partial |
| `continue` | `--continue` / `-c` | Core | ❌ stub |
| `max-tries` | `--max-tries` | Core | 🔧 hardcoded to 5 |
| `timeout` | `--timeout` | Network | ❌ stub |
| `connect-timeout` | `--connect-timeout` | Network | ❌ stub |
| `rpc-listen-port` | `--rpc-listen-port` | RPC | ✅ done (6800 default) |
| `rpc-secret` | `--rpc-secret` | RPC | ❌ stub |
| `rpc-listen-all` | `--rpc-listen-all` | RPC | ✅ done (0.0.0.0) |
| `enable-rpc` | `--enable-rpc` | RPC | ✅ done (daemon mode) |
| `daemon` | `--daemon` / `-D` | CLI | ❌ stub |
| `conf-path` | `--conf-path` | CLI | ❌ stub |
| `input-file` | `--input-file` / `-i` | CLI | ❌ stub |
| `save-session` | `--save-session` | Session | ❌ stub |
| `save-session-interval` | `--save-session-interval` | Session | ❌ stub |
| `checksum` | `--checksum` | Verify | ✅ done |

## Tier B: High Value (post-parity polish)

| Option | aria2 flag | Area |
|--------|-----------|------|
| `all-proxy` | `--all-proxy` | Network |
| `http-proxy` | `--http-proxy` | Network |
| `https-proxy` | `--https-proxy` | Network |
| `no-proxy` | `--no-proxy` | Network |
| `load-cookies` | `--load-cookies` | HTTP |
| `save-cookies` | `--save-cookies` | HTTP |
| `netrc-path` | `--netrc-path` | HTTP |
| `no-netrc` | `--no-netrc` | HTTP |
| `ca-certificate` | `--ca-certificate` | TLS |
| `check-certificate` | `--check-certificate` | TLS |
| `certificate` | `--certificate` | TLS |
| `private-key` | `--private-key` | TLS |
| `file-allocation` | `--file-allocation` | I/O |
| `min-split-size` | `--min-split-size` | Core |
| `lowest-speed-limit` | `--lowest-speed-limit` | Core |
| `max-file-not-found` | `--max-file-not-found` | Core |
| `retry-wait` | `--retry-wait` | Core |
| `select-file` | `--select-file` | BT |
| `seed-ratio` | `--seed-ratio` | BT |
| `seed-time` | `--seed-time` | BT |
| `bt-tracker` | `--bt-tracker` | BT |
| `quiet` | `--quiet` / `-q` | CLI |
| `log` | `--log` / `-l` | CLI |

## Tier C: Deferred / Gap

| Option | aria2 flag | Reason |
|--------|-----------|--------|
| `enable-xml-rpc` | N/A | XML-RPC explicitly not implemented |
| `xml-rpc-listen-port` | N/A | XML-RPC explicitly not implemented |
| `bt-require-crypto` | `--bt-require-crypto` | BT-GAP-001: MSE not in librqbit |
| `bt-min-crypto-level` | `--bt-min-crypto-level` | BT-GAP-001 |
| `metalink-enable-unique-protocol` | Various | Complex multi-protocol not in v1 |
| `on-download-start` | `--on-download-start` | Hook scripts deferred |
| `on-download-complete` | `--on-download-complete` | Hook scripts deferred |
| `on-download-error` | `--on-download-error` | Hook scripts deferred |
