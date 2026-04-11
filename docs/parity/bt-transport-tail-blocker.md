# BitTorrent Transport Tail Blocker Memo (DHT / PEX / uTP)

> Updated: 2026-04-11
>
> Scope: the remaining BitTorrent transport-tail audit for DHT, PEX, and uTP only.

## Conclusion

Closing the remaining BT transport tail with a single deterministic proof is not feasible in the
current `raria + librqbit` stack.

- `DHT`: raria now exposes a minimal persistence-file seam, but it still does not expose the
  bootstrap injection needed to force a local deterministic DHT topology.
- `PEX`: present in upstream source, but it does not independently close the lane while DHT and
  uTP remain unproven.
- `uTP`: absent from the current dependency/runtime path, so there is no transport surface to
  prove locally.

## Evidence

### 1. raria now forwards only the DHT on/off booleans plus a persistence-file seam

In [service.rs](/Users/sekiro/Projects/VSCode/raria/crates/raria-bt/src/service.rs), `BtService`
builds `SessionOptions` with:

- `disable_dht`
- `disable_dht_persistence`
- optional persistent DHT config filename
- SOCKS proxy and fastresume persistence

It still does not expose any local bootstrap override.

### 2. librqbit does not expose deterministic bootstrap control through `SessionOptions`

In upstream `librqbit`, `SessionOptions` includes `dht_config: Option<PersistentDhtConfig>`, but
that type is only a persistence wrapper (`dump_interval`, `config_filename`) rather than a true
bootstrap seam.

The deterministic controls that actually matter live in upstream `DhtConfig`:

- `bootstrap_addrs`
- `listen_addr`
- `routing_table`

When `bootstrap_addrs` is not provided, librqbit falls back to public DHT bootstrap nodes.
Current raria code now forwards `disable_dht` / `disable_dht_persistence`, BT persistence and
fastresume, plus an optional persistent DHT config filename. It still cannot inject
`bootstrap_addrs` or a local-only DHT topology.

There is an internal recovery path where `PersistentDht` reloads `listen_addr`, `routing_table`,
and `peer_store` from its JSON persistence file, but that format is not exposed as a stable public
test seam from raria. In practice, that still leaves us without a straightforward deterministic
local DHT harness on the current product path.

I also attempted a stronger local proof by adding a product-path seam for the persistent DHT
config filename and seeding a handcrafted persistence file containing:

- a local listen address
- a routing table with a self-referential node entry
- a peer store entry for the torrent's info-hash

Even with that seam, the BT client still timed out before peer visibility on the product path.
That makes the blocker stricter than "missing config plumb-through": the current stack still lacks
a reliable deterministic local DHT proof recipe.

### 3. That makes a real local DHT proof non-deterministic from raria

Because `BtServiceConfig` cannot inject `dht_config`, a raria test cannot point the BT session at
a local-only DHT bootstrap node. The remaining options are:

1. rely on the public internet, which is not deterministic
2. broaden production code to add a new DHT test seam first

For the current scope, that is a blocker, not a proof.

### 4. PEX exists upstream, but it is not the blocker

Upstream `librqbit` handles incoming and outgoing PEX in its live torrent state, and feeds PEX
peers back into peer discovery. So PEX exists in source.

The reason this lane still does not close is not "PEX missing"; it is that DHT and uTP still
prevent an honest transport-tail proof.

An attempted local deterministic PEX proof using a dual-seed / single-client topology did not
produce a stable second-peer discovery signal within the test window, so PEX also remains
unproven in a deterministic local harness today.

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

1. `DHT` needs a new raria test seam for local bootstrap/persistence injection before a
   deterministic local proof is possible.
2. `uTP` is absent from the current upstream/runtime path, so there is nothing real to prove.

Until those two blockers change, any claim that the full `DHT / PEX / uTP` tail is proven would
overstate what the code and local test environment can actually demonstrate.
