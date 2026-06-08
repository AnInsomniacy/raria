# Phase One Release Scope

`raria` phase one is a new-session replacement target. It does not read old `.aria2` files, old queues, old sessions, old partial downloads, old caches, or old history.

Supported new-session surfaces:

- CLI/config/input-file parsing for common aria2-style workflows.
- Direct CLI URI execution for HTTP(S), FTP, SFTP, and BitTorrent magnet tasks through the shared engine.
- Save-session text for new raria tasks.
- JSON-RPC POST on `/jsonrpc`.
- WebSocket notifications on `/jsonrpc`.
- Foreground RPC server mode with a background download loop.
- `token:SECRET` parameter stripping for RPC calls.
- Add, poll, pause, unpause, remove, global stat, multicall, method listing, notification listing, version, session info, and save-session acknowledgement.
- HTTP(S) download with range resume, split ranges, checksum, headers, cookies, netrc, proxy, and task rate limits.
- FTP and SFTP basic downloads, credential options, and `.raria` resume.
- BitTorrent torrent bytes and magnet tasks through `librqbit` 8.1.1, including selected-file mapping and initial-peer fixture support.
- Metalink v3/v4 HTTP resources with size and SHA-256 mapping.
- Versioned `.raria` control files for new-task resume.

Explicit phase-one exclusions:

- ED2K transfer and ED2K search.
- Old `.aria2` state migration or writing.
- XML-RPC, JSONP, and JSON-RPC GET.
- Deprecated `rpc-user` and `rpc-passwd`.
- HTTP pipelining and event-poll selection.
- Built-in daemonization and process hook commands.
- libaria2 C API compatibility.

Post-phase-one probes remain for FTPS, FTP proxy, SFTP host-key pinning, advanced BitTorrent knobs, richer Metalink policy filters, global option mutation, and full all-task RPC convenience methods.

Final smoke evidence:

- `cargo run -p raria -- --dir <temp> http://127.0.0.1:<fixture>/file.txt`
- `crates/raria-core/tests/runtime_server.rs` covers RPC add, poll, and background HTTP completion from a clean session.
