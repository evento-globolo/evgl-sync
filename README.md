# evgl-sync

Offline-first event draft, attendee, venue, and publication synchronization built with opto-sync contracts.

Initialized through `DEN-1889` as a testable `sync` foundation. Product behavior continues through focused pull requests.

The service exposes JSON reconciliation at `/v1/reconcile` (with the legacy
`/api/v1/reconcile` alias). The library also provides `OptoSyncClient` and
`LeaseGuard` for the external Opto Sync lease/fencing-token boundary recovered
from the provider-cross-posting work. A caller must carry the current fencing
token on every protected data-plane write; lease ownership alone is not a write
authorization.

`OptoSyncClient` accepts only absolute HTTP(S) base URLs, bounds request and
lease lifetimes, percent-encodes lease keys as a single path segment, and does
not copy remote response bodies into errors. Supply any bearer credential only
through the runtime secret manager.

```bash
python3 scripts/verify_repo.py
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```
