# Catalog v1 — cursor migration fixture

This is a local TypeScript REST backend for Swarm scenario S3. Its 24 resource modules deliberately use page/offset pagination at the pinned baseline. Each route returns the documented `{data, meta:{hasMore,limit,nextCursor}}` shape. The page-number `nextCursor` is deliberately not a stable cursor: inserting a row between fetches can duplicate an item, and equal `createdAt` values need a primary-key tie-breaker.

`node --test acceptance.test.ts` is the migration acceptance check. It is expected to fail on this baseline and pass only after the shared contract and all 24 routes are migrated. The `__fixture` endpoints mutate only in-memory data in the local test server. No provider, database, external service or paid model is used.

Run `node replay-backend.mjs` to check the three contract stages in temporary copies. The reference outputs under `reference/` are scripted worker patch inputs for the separate Swarm integration test; they are not code loaded by the baseline service. That test runs with `cargo test --offline -p overseerd --test swarm_scenarios -- --ignored` on Node.js 24 with localhost socket permission. Both replays are deliberately partial S3 evidence; they do not launch model workers or exercise the adaptive scheduler.
