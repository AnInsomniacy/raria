# Native Integration Guide

raria integrations should use the native HTTP JSON API and WebSocket event
stream. Do not integrate through JSON-RPC, aria2 method names, aria2 option
names, Gid identifiers, legacy session files, or compatibility adapters.

## Control Plane

Start the daemon with a trusted listen address and an optional bearer token:

```bash
raria daemon --download-dir ~/Downloads --api-port 6800
```

Native resources are served under `/api/v1`. The event stream is
`/api/v1/events`.

Use opaque `taskId` values exactly as returned by raria. Do not parse them, sort
by embedded data, or convert them to legacy identifiers.

## Task Creation

Create ordinary transfer tasks through `POST /api/v1/tasks`:

```json
{
  "sources": ["https://example.com/file.iso"],
  "downloadDir": "/tmp",
  "filename": "file.iso",
  "segments": 8
}
```

BitTorrent options belong under `bt`. Metalink input belongs under `metalink`.
Use native field names such as `selectedFileIds`, `trackerUris`,
`metadataOnly`, `webSeedUris`, `seeding`, `downloadBytesPerSecondLimit`, and
`uploadBytesPerSecondLimit`.

## Lifecycle

Use native lifecycle routes:

```text
POST /api/v1/tasks/{taskId}/pause
POST /api/v1/tasks/{taskId}/resume
POST /api/v1/tasks/{taskId}/restart
DELETE /api/v1/tasks/{taskId}
POST /api/v1/session/save
POST /api/v1/daemon/shutdown
```

Poll task resources only when needed. Prefer `/api/v1/events` for lifecycle,
progress, source failure, BitTorrent metadata, seeding, peer, and tracker
updates.

## Client Responsibilities

Clients own presentation, user workflows, drag-and-drop handling, browser
capture, and platform integration. raria owns transfer execution, task state,
queue policy, persistence, integrity checks, protocol behavior, and native
events.

Future GUI clients, including Motrix Next adapters, must adapt to this native
contract instead of requiring raria to restore old wire formats.
