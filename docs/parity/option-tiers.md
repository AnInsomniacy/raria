# Option Tier Classification

> Updated: 2026-04-09

## Legend

| Status | Meaning |
|--------|---------|
| `has_code` | Parser or underlying support exists, but the product path is incomplete |
| `wired` | Option is connected to the hot path, but needs broader validation |
| `tested` | Automated coverage exists and is passing |
| `client_verified` | Verified through real daemon/binary/client flows |
| `gap` | Known incompatibility or intentionally unsupported |

## Tier A: Must Complete (blocks "can replace aria2")

| Option | aria2 flag | Area | Status |
|--------|-----------|------|--------|
| `dir` | `--dir` | Core | `client_verified` |
| `out` | `--out` | Core | `client_verified` |
| `split` | `--split` | Core | `wired` |
| `max-connection-per-server` | `--max-connection-per-server` | Core | `wired` |
| `max-concurrent-downloads` | `--max-concurrent-downloads` | Core | `tested` |
| `max-overall-download-limit` | `--max-overall-download-limit` | Core | `tested` |
| `max-download-limit` | `--max-download-limit` | Core | `tested` |
| `continue` | `--continue` / `-c` | Core | `has_code` |
| `max-tries` | `--max-tries` | Core | `wired` |
| `timeout` | `--timeout` | Network | `client_verified` |
| `connect-timeout` | `--connect-timeout` | Network | `client_verified` |
| `rpc-listen-port` | `--rpc-listen-port` | RPC | `client_verified` |
| `rpc-secret` | `--rpc-secret` | RPC | `tested` |
| `rpc-listen-all` | `--rpc-listen-all` | RPC | `tested` |
| `enable-rpc` | `--enable-rpc` | RPC | `client_verified` |
| `daemon` | `--daemon` / `-D` | CLI | `has_code` |
| `conf-path` | `--conf-path` | CLI | `wired` |
| `input-file` | `--input-file` / `-i` | CLI | `tested` |
| `save-session` | `--save-session` | Session | `wired` |
| `save-session-interval` | `--save-session-interval` | Session | `has_code` |
| `checksum` | `--checksum` | Verify | `tested` |
| `conditional-get` | `--conditional-get` | HTTP | `client_verified` |
| `allow-overwrite` | `--allow-overwrite` | Core | `wired` |

## Tier B: High Value (post-parity polish)

| Option | aria2 flag | Area | Status |
|--------|-----------|------|--------|
| `all-proxy` | `--all-proxy` | Network | `tested` |
| `http-proxy` | `--http-proxy` | Network | `wired` |
| `https-proxy` | `--https-proxy` | Network | `wired` |
| `no-proxy` | `--no-proxy` | Network | `tested` |
| `load-cookies` | `--load-cookies` | HTTP | `tested` |
| `save-cookies` | `--save-cookies` | HTTP | `has_code` |
| `netrc-path` | `--netrc-path` | HTTP | `client_verified` |
| `no-netrc` | `--no-netrc` | HTTP | `client_verified` |
| `conditional-get` | `--conditional-get` | HTTP | `client_verified` |
| `allow-overwrite` | `--allow-overwrite` | Core | `client_verified` |
| `ca-certificate` | `--ca-certificate` | TLS | `wired` |
| `check-certificate` | `--check-certificate` | TLS | `wired` |
| `max-redirect` | `--max-redirect` | HTTP | `client_verified` |
| `header` | `--header` | HTTP | `client_verified` |
| `auto-file-renaming` | `--auto-file-renaming` | Core | `client_verified` |
| `http-user` | `--http-user` | HTTP | `client_verified` |
| `http-passwd` | `--http-passwd` | HTTP | `client_verified` |
| `sftp-strict-host-key-check` | `--sftp-strict-host-key-check` | SFTP | `wired` |
| `sftp-known-hosts` | `--sftp-known-hosts` | SFTP | `wired` |
| `sftp-private-key` | `--sftp-private-key` | SFTP | `wired` |
| `sftp-private-key-passphrase` | `--sftp-private-key-passphrase` | SFTP | `wired` |
| `certificate` | `--certificate` | TLS | `has_code` |
| `private-key` | `--private-key` | TLS | `has_code` |
| `file-allocation` | `--file-allocation` | I/O | `tested` |
| `min-split-size` | `--min-split-size` | Core | `has_code` |
| `lowest-speed-limit` | `--lowest-speed-limit` | Core | `has_code` |
| `max-file-not-found` | `--max-file-not-found` | Core | `has_code` |
| `retry-wait` | `--retry-wait` | Core | `has_code` |
| `select-file` | `--select-file` | BT | `has_code` |
| `seed-ratio` | `--seed-ratio` | BT | `has_code` |
| `seed-time` | `--seed-time` | BT | `has_code` |
| `bt-tracker` | `--bt-tracker` | BT | `has_code` |
| `quiet` | `--quiet` / `-q` | CLI | `client_verified` |
| `log` | `--log` / `-l` | CLI | `has_code` |

## Tier C: Deferred / Gap

| Option | aria2 flag | Reason | Status |
|--------|-----------|--------|--------|
| `enable-xml-rpc` | N/A | XML-RPC explicitly not implemented | `gap` |
| `xml-rpc-listen-port` | N/A | XML-RPC explicitly not implemented | `gap` |
| `bt-require-crypto` | `--bt-require-crypto` | BT-GAP-001: MSE not in librqbit | `gap` |
| `bt-min-crypto-level` | `--bt-min-crypto-level` | BT-GAP-001 | `gap` |
| `metalink-enable-unique-protocol` | Various | Complex multi-protocol not in v1 | `gap` |
| `on-download-start` | `--on-download-start` | Hook scripts deferred | `has_code` |
| `on-download-complete` | `--on-download-complete` | Hook scripts deferred | `has_code` |
| `on-download-error` | `--on-download-error` | Hook scripts deferred | `has_code` |
