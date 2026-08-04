# evgl-sync

opto-sync/syncer.rs JSON reconciliation gateway for Evento Globolo.

**Product:** Evento Globolo — A global event discovery and aggregation platform.

Aggregate, normalize, deduplicate, search, and follow events from sources such as Eventbrite, Meetup, LinkedIn, Facebook, and Craigslist through authorized APIs or permitted ingestion paths.

## Safety and production boundary

Provider names are integration targets, not claims of affiliation. Use official APIs and permitted data-access methods; do not bypass authentication, anti-bot, rate-limit, copyright, or platform-policy controls.

This repository is an executable bootstrap, not a production deployment. Before live
use, add authentication, tenant authorization, rate limits, durable migrations,
observability, backups, incident response, dependency review, and secret management.
## Reconciliation contract

`POST /api/v1/reconcile` accepts `{"base":...,"incoming":...}` and delegates to
`opto-sync/syncer.rs` at immutable commit `132a97c77867128656070be85d3046b0cc065cbf`. The default policy is
identity-keyed array merge using `id` plus last-writer-wins selectors
`updated_at,synced_at`.

The gateway is not a durable record by itself. Persist and authorize the result in
the owning API/database transaction, enforce idempotency, and retain conflict audit
metadata.
