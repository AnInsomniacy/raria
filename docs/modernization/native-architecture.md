# Native raria Architecture

This document defines the target native raria architecture for the modernization goal. It is a design checkpoint, not an implementation claim. aria2 remains a source-derived feature reference only.

## Design Targets

raria will expose native task, source, file, segment, piece, peer, tracker, event, configuration, API, CLI, and persistence models. Public names and wire formats will be raria-owned. Existing aria2-shaped internals may be migrated incrementally while keeping the workspace compiling, but they are not completion-compatible.

The implementation should preserve working downloader code where it already exists. HTTP, FTP, SFTP, Metalink, BitTorrent, segmented execution, checksums, file allocation, rate limiting, persistence, daemon operation, and structured logging should be lifted into the native model instead of rewritten from scratch.

## Verified Library Anchors

reqwest 0.12 remains the HTTP and HTTPS backend choice. Its builder exposes cookies, proxy configuration, redirects, default headers, timeout controls, TLS controls, client identity, local address, interface, and DNS resolver hooks, which cover the modern HTTP transport surface raria needs.

suppaftp 8 remains the FTP and FTPS backend choice. Its documented feature set includes async support, explicit FTPS, rustls or native-tls, stream-oriented transfers, LIST parsing, error codes, and modern Rust error handling.

russh with russh-sftp remains the SFTP backend choice. russh is an async tokio/futures SSH client and server library, and russh-sftp provides high-level client APIs with async I/O over the SFTP subsystem.

quick-xml remains the Metalink parser base. It provides a streaming XML reader and writer plus optional serde support. Metalink v4 is anchored to RFC 5854, which defines locations, mirrors, hashes, pieces, metaurl, and the `application/metalink4+xml` media type.

librqbit remains the BitTorrent engine choice. Its public crate documentation identifies Session, AddTorrent, Magnet, parsed torrent metadata, DHT re-export, tracker communication dependencies, API facade types, file details, torrent stats, and session persistence types. Gaps such as PEX and UDP tracker behavior still require direct verification against its public APIs and runtime tests before deciding whether to wrap library support or add a small native layer.

redb remains the local persistence store. Its documentation describes an ACID embedded key-value store with copy-on-write B-trees, concurrent readers, a single writer, crash safety, savepoints, and typed table definitions. raria should use redb as the storage engine, not as the schema model.

axum remains the native control server. Its documented model is type-safe routing, extractors, responses, shared state with `State`, tokio and hyper integration, tower middleware, JSON support, and optional WebSocket support.

## Native Domain Model

`TaskId` replaces `Gid` as the public identifier. The recommended format is a stable opaque string with a typed prefix, for example `task_01h8x5...`, generated from UUIDv7 or a sortable random identifier. It must not encode aria2 hex semantics. Internal numeric IDs may exist only as private storage keys.

`Task` replaces `Job`. A task is a protocol-neutral download unit with one lifecycle state, one target directory, one source graph, one or more output files, aggregate progress, configured limits, retry policy, auth references, checksum policy, metadata, timestamps, and error history.

`Source` represents an individual fetchable location. Source fields include ID, URI, protocol, priority, region, transport options, health score, last failure, last success, bytes served, response metadata, and whether it is eligible for segment scheduling.

`FileEntry` represents an output file. File fields include ID, path, length, selected flag, completed bytes, piece range, whole-file checksum, content metadata, conflict policy, allocation state, and final verification state.

`Segment` represents byte-range work for range protocols. Segment fields include ID, file ID, source ID when assigned, byte range, completed bytes, validator metadata such as ETag and Last-Modified, state, retry state, and checkpoint timestamp.

`Piece` represents verification and BitTorrent piece state. Piece fields include ID, file span, length, hash algorithm, expected hash, completed bytes, availability, verification state, and source protocol. Metalink pieces and BitTorrent pieces should converge here.

`Peer` and `Tracker` represent BitTorrent runtime state. They are attached to a task and exposed through native API projections with stable names, not through aria2 response fields.

## Lifecycle

The native task lifecycle is `queued`, `running`, `paused`, `seeding`, `completed`, `failed`, and `removed`. `queued` replaces waiting terminology. `running` covers active range and BitTorrent payload work. `seeding` is BitTorrent-specific but remains a first-class task state. `removed` is terminal and should retain only audit/history state until purged by policy.

State transitions must be owned by a single task service. The current engine, scheduler, registry, cancellation registry, event bus, and daemon activation loop should be consolidated behind a native service API before the old RPC facade is removed. Direct public mutation of registry state should disappear.

Crash recovery demotes interrupted `running` and `seeding` tasks to `queued` when payload work can be resumed. Completed, failed, paused, and removed states should restore as-is. Segment checkpoints, file allocation state, validators, piece verification state, and BitTorrent fastresume references must be restored before scheduling work.

## Native Events

The WebSocket stream should use path `/api/v1/events`. Each message is one JSON object with `version`, `sequence`, `time`, `type`, `taskId`, and `data`. Event types should use raria names such as `task.created`, `task.started`, `task.paused`, `task.resumed`, `task.completed`, `task.failed`, `task.removed`, `task.progress`, `task.source.failed`, `task.source.recovered`, `task.file.completed`, `task.piece.verified`, `task.bt.metadata.resolved`, `task.bt.seeding.started`, `task.bt.peer.updated`, and `task.bt.tracker.updated`.

Events must be useful without RPC state. Progress events include completed bytes, total bytes, instantaneous and rolling speed, active connection count, active sources, selected files, and ETA when known. Error events include stable error code, user-facing message, source ID when relevant, retry classification, and redacted context.

The aria2 JSON-RPC notification projection must be replaced by this native stream. Existing aria2 notification tests should be rewritten as native event contract tests or removed when they only assert compatibility.

## Native HTTP JSON API

The native API root is `/api/v1`. The API is resource-oriented JSON over HTTP, not JSON-RPC. Authentication should use standard HTTP authorization headers. Token-in-params behavior must be removed.

The first stable surface should include `GET /api/v1/health`, `GET /api/v1/version`, `GET /api/v1/tasks`, `POST /api/v1/tasks`, `GET /api/v1/tasks/{taskId}`, `PATCH /api/v1/tasks/{taskId}`, `DELETE /api/v1/tasks/{taskId}`, `POST /api/v1/tasks/{taskId}/pause`, `POST /api/v1/tasks/{taskId}/resume`, `POST /api/v1/tasks/{taskId}/restart`, `GET /api/v1/tasks/{taskId}/files`, `PATCH /api/v1/tasks/{taskId}/files`, `GET /api/v1/tasks/{taskId}/sources`, `PATCH /api/v1/tasks/{taskId}/sources`, `GET /api/v1/tasks/{taskId}/peers`, `GET /api/v1/tasks/{taskId}/trackers`, `GET /api/v1/stats`, `GET /api/v1/config`, and `PATCH /api/v1/config/runtime`.

Task creation accepts typed source inputs: direct URI list, Metalink bytes or path, torrent bytes or path, magnet URI, and optional output policy. The response returns `taskId`, initial state, file projection when known, source projection, and links to resource endpoints.

API errors use a stable envelope with `code`, `message`, `details`, and `requestId`. Codes should be raria-owned, for example `invalid_request`, `task_not_found`, `unsupported_protocol`, `auth_failed`, `source_unreachable`, `checksum_failed`, `conflict`, `storage_error`, and `internal_error`.

## Native Configuration

`raria.toml` replaces aria2 key-value configuration. The parser should be strict by default. Unknown keys should fail unless a documented forward-compatibility escape is added later. The config model should use nested sections and serde validation.

The recommended section layout is `[daemon]`, `[api]`, `[downloads]`, `[network]`, `[http]`, `[ftp]`, `[sftp]`, `[bittorrent]`, `[metalink]`, `[storage]`, `[logging]`, `[hooks]`, and `[[profiles]]`.

Fields should use native names. Examples include `download_dir`, `session_path`, `max_active_tasks`, `max_global_download_bytes_per_second`, `max_global_upload_bytes_per_second`, `listen_addr`, `auth_token_file`, `allow_origins`, `default_segments`, `min_segment_size`, `retry_max_attempts`, `retry_backoff`, `proxy`, `no_proxy`, `tls_ca_certificate`, `tls_client_certificate`, `tls_client_key`, `cookie_store_path`, `netrc_path`, `sftp_known_hosts`, `sftp_identity_file`, `dht_state_path`, `enable_dht`, `enable_udp_trackers`, `enable_pex`, `seed_ratio`, `seed_time`, `file_allocation`, `conflict_policy`, and `structured_log_path`.

Legacy fields such as `rpc_secret`, `rpc_listen_port`, `gid`, `out`, `split`, `bt-min-crypto-level`, and `check-certificate` must not appear in the final public config. Equivalent modern behavior should be renamed and reshaped.

## Native Persistence

The storage schema must be versioned independently from Rust structs. Directly serializing `Task` is acceptable only if wrapped in explicit schema versions and migration tests.

The first native redb schema should include a metadata table with `schema_version`, `store_id`, `created_at`, and `last_migrated_at`; a task table keyed by internal storage ID; an ID index mapping `TaskId` to storage ID; source, file, segment, piece, peer snapshot, tracker snapshot, event cursor, and runtime config tables; plus a migration ledger.

Every persisted row should include a row schema version. Migrations should be idempotent and tested with fixture databases generated by previous schema writers. The store open path should refuse unknown future schemas unless an explicit read-only inspection mode is added.

The existing redb store can be migrated by adding native tables alongside current tables, then writing dual state during the transition, then cutting readers to native tables, then removing old job and segment tables after tests prove coverage. There is no requirement to read aria2 session or control files.

## Migration Strategy

The implementation should proceed in slices that keep the workspace compiling. First add native types and schema tests. Then add adapters from existing `Job` state into native projections to keep current downloader paths usable. Then implement the native API and event stream against projections. Then migrate configuration to `raria.toml`. Then move engine internals from `Gid` and `Job` to `TaskId` and `Task`. Then delete aria2 RPC, facade, config parser, and parity tests once native coverage replaces them.

Compatibility shims may exist only inside the migration branch of the code and must be private. They must not be documented, exposed, or treated as acceptable final state.

## Verification Requirements

Each migration slice must update `modern-feature-matrix.md` and `progress-log.md`. Each native public surface needs contract tests. Each protocol capability needs either an executable test or an explicit verification anchor in the matrix. The final stopping condition remains `cargo test --workspace`, `cargo check --workspace`, and `cargo clippy --workspace --all-targets -- -D warnings`.

