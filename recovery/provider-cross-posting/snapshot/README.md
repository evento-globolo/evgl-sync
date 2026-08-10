# evgl-sync

Distributed synchronization primitives for Evento Globolo, integrated with Opto Sync.

This service treats Opto Sync as the lease and fencing-token authority instead of embedding a private lock algorithm. A reconciliation task acquires `evgl/reconcile/{tenant}/{shard}`, carries the returned fencing token on every mutation, renews before expiry, and releases best-effort.

The `syncer-rs` dependency is pinned to an immutable Opto Sync commit for native interoperability and shared contract evolution. The HTTP lease API remains the runtime process boundary so deployments can scale and upgrade the coordination service independently.
