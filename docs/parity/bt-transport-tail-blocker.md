# BitTorrent Transport Tail Blocker Memo (DHT / PEX / uTP)

> Updated: 2026-04-11
>
> Scope: the remaining BitTorrent transport-tail audit for DHT, PEX, and uTP only.

## Conclusion

Closing the remaining BT transport tail with a single deterministic proof is still not feasible in
the current `raria + librqbit` stack, even after adding a local DHT bootstrap/listen seam.

- `DHT`: raria now exposes both a persistence-file seam and an explicit local
  bootstrap/listen-address seam, but the deterministic local proof still timed out before peer
  discovery on the product path.
- `PEX`: present in upstream source, but it does not independently close the lane while DHT and
  uTP remain unproven.
- `uTP`: absent from the current dependency/runtime path, so there is no transport surface to
  prove locally.

## Evidence

### 1. raria now forwards DHT on/off, a persistence-file seam, and a local bootstrap/listen seam

In [service.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-bt/src/service.rs), `BtService`
builds `SessionOptions` with:

- `disable_dht`
- `disable_dht_persistence`
- optional persistent DHT config filename
- optional DHT bootstrap address list
- optional DHT listen address
- SOCKS proxy and fastresume persistence

### 2. locked `librqbit 8.1.1` still needs a local patch to surface deterministic bootstrap control

In the crates.io `librqbit 8.1.1` that raria locks today, upstream `SessionOptions` includes
`dht_config: Option<PersistentDhtConfig>`, but that type is only a persistence wrapper
(`dump_interval`, `config_filename`) rather than a true bootstrap seam.

The deterministic controls that actually matter live in upstream `DhtConfig`:

- `bootstrap_addrs`
- `listen_addr`
- `routing_table`

Raria now vendors a minimal local patch so `SessionOptions` can additionally carry:

- `dht_bootstrap_addrs`
- `dht_listen_addr`

When those overrides are set, the session bypasses persistent DHT restore and instantiates
`DhtBuilder::with_config(DhtConfig { bootstrap_addrs, listen_addr, ... })` directly.

There is an internal recovery path where `PersistentDht` reloads `listen_addr`, `routing_table`,
and `peer_store` from its JSON persistence file, but that format is not exposed as a stable public
test seam from raria. In practice, that still leaves us without a straightforward deterministic
local DHT harness on the current product path.

I attempted two stronger local proofs:

1. a product-path persistence-file seam with a handcrafted DHT state file containing:

- a local listen address
- a routing table with a self-referential node entry
- a peer store entry for the torrent's info-hash

2. a product-path bootstrap/listen seam using a locally bound DHT node as the only bootstrap peer

Even with the stronger bootstrap/listen seam, the BT client still timed out before peer visibility
on the product path. That makes the blocker stricter than "missing config plumb-through": the
current stack still lacks a reliable deterministic local DHT proof recipe.

### 3. That still leaves a real local DHT proof non-deterministic from raria

Even after adding the seam, the remaining options are:

1. rely on the public internet, which is not deterministic
2. keep broadening product/runtime seams until the exact missing announce/discovery precondition is exposed

For the current scope, that is a blocker, not a proof.

### 4. PEX exists upstream, but it is not the blocker

Upstream `librqbit` handles incoming and outgoing PEX in its live torrent state, and feeds PEX
peers back into peer discovery. So PEX exists in source.

The reason this lane still does not close is not "PEX missing"; it is that DHT and uTP still
prevent an honest transport-tail proof.

An attempted local deterministic PEX proof using a dual-seed / single-client topology also did not
produce a stable second-peer discovery signal within the test window, even after ensuring one seed
had the other as an explicit outgoing initial peer. So PEX also remains unproven in a
deterministic local harness today.

### 5. uTP is absent from the current source/runtime path

Strict source search across:

- `crates/`
- `librqbit-8.1.1`
- `librqbit-peer-protocol-*`
- `librqbit-dht-*`

found no `utp`, `uTP`, or `UTP` transport implementation identifiers in the active runtime path.

That absence matches the actual runtime path:

- upstream session binds a `TcpListener`
- upstream peer connection path uses `TcpStream::connect(...)`

So there is currently no uTP transport path in the stack that raria could exercise deterministically.

## Precise Blocker Statement

The remaining BT transport tail cannot be promoted with a deterministic proof today because:

1. `DHT` still lacks a reliable deterministic proof recipe on the product path even after adding
   local bootstrap/listen injection.
2. `uTP` is absent from the current upstream/runtime path, so there is nothing real to prove.

Until those two blockers change, any claim that the full `DHT / PEX / uTP` tail is proven would
overstate what the code and local test environment can actually demonstrate.
