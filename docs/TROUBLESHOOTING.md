# Troubleshooting

This guide covers common raria CLI, daemon, native API, transfer, and release
issues.

## Confirm the Binary

```bash
raria --version
raria --help
raria daemon --help
```

If the command is missing, confirm that the release archive was extracted and
that the binary directory is on `PATH`.

## Daemon Does Not Respond

Confirm the configured API port and listen address. The default native API port
is `6800`.

```bash
curl http://127.0.0.1:6800/api/v1/health
```

If bearer auth is configured, include:

```bash
curl -H "Authorization: Bearer $RARIA_TOKEN" http://127.0.0.1:6800/api/v1/health
```

Check whether another process is using the port, whether the daemon was started
with a different `--api-port`, and whether `raria.toml` points to a different
listen address.

## Task Does Not Start

Check `/api/v1/tasks/{taskId}` for lifecycle, `errorMessage`,
`activeConnections`, `sources`, and transfer limits. Confirm `downloadDir`
exists or can be created by the daemon process.

For queue issues, inspect:

```text
GET /api/v1/tasks/{taskId}/queue
GET /api/v1/transfer
```

## Pause or Resume Looks Stuck

Pause and resume are task lifecycle requests. A slow protocol backend may need a
short time to observe cancellation. Use `/api/v1/events` to confirm whether a
`task.paused`, `task.resumed`, `task.failed`, or `task.progress` event was
emitted.

## Resume or Restore Fails

Confirm that the daemon uses the expected native session path and that the file
is writable.

```toml
[daemon]
session_path = "raria.session.redb"
```

Do not mix session files from other tools or old formats. raria supports its
own versioned native persistence schemas only.

## Download Integrity Fails

Checksum failures mean the completed output did not match the expected digest.
Recheck the expected checksum, mirror list, proxy, and any existing partial
file. For multi-source tasks, include source failure events and checksum output
when opening an issue.

## BitTorrent Metadata Does Not Resolve

Metadata depends on reachable trackers, DHT, peers, and the selected network.
Check task trackers and peers:

```text
GET /api/v1/tasks/{taskId}/trackers
GET /api/v1/tasks/{taskId}/peers
```

For deterministic bug reports, prefer a small torrent fixture or a known stable
public torrent and include bounded run time, observed peers, tracker state, and
whether `bt.metadataOnly` was set.

## Release Archive Fails Checksum

Download the archive and matching `.sha256` file from the same GitHub Release.
Then verify with the commands in [Release Integrity](RELEASE_INTEGRITY.md).
If the digest differs, report the artifact name, release tag, platform, command,
and computed digest.
