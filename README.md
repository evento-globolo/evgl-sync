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

## Environment secrets

Secrets live in this repo **encrypted** with [sops](https://github.com/getsops/sops) + [age](https://github.com/FiloSottile/age):
`env/enc/<dev|prod>.env.enc` is committed; `just env-use <name>` decrypts it to
`env/dec/<name>.env` (gitignored, mode 0600) and symlinks `./.env` to it. The
Nix dev shell provides the tooling, `just env-audit` runs keyless in CI, and
containers decrypt at `docker run` — never at build. See [`env/README.md`](env/README.md).
