# ADR-0001: BT/DHT Ownership Boundary

## Status

Accepted

## Context

`raria-bt`, `raria-cli`, the daemon runtime, and the RPC facade all currently project BitTorrent and DHT state. That creates ambiguity around who owns:

- live BT session state
- DHT routing table persistence
- peer and tracker visibility snapshots
- seeding lifecycle semantics
- restart and restore behavior

The plan requires concept owners rather than crate-name ownership.

## Decision

The concept owners are:

- `bt_session_authority`
  - owns live torrent session state
  - currently anchored in `crates/raria-bt/src/service.rs`
- `dht_state_authority`
  - owns DHT routing table state, bootstrap semantics, and persistence obligations
  - currently anchored in `crates/raria-bt/src/service.rs`
- `daemon_lifecycle`
  - owns projection into daemon-visible job state and restart coordination
  - currently anchored in `crates/raria-cli/src/bt_runtime.rs`
- `rpc_control_plane`
  - owns discoverability and surface projection only
  - currently anchored in `crates/raria-rpc/src/server.rs`

## Consequences

- BT and DHT behavior must be classified before parity claims are expanded.
- The daemon and RPC layers can project BT/DHT state, but they do not redefine BT/DHT truth.
- Any feature that requires shared owner-grade state beyond `librqbit` must be promoted to `raria_owned_subsystem`.

## Follow-ups

- Keep `.omx/parity/ownership-decisions.yaml` aligned with this ADR.
- Promote unresolved BT/DHT capabilities to explicit owner classes before they enter parity gates.
