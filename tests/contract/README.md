# Contract tests

`run-all.sh` proves API compatibility by sending one scripted request sequence to the
**reference NestJS app** and to the **new Rust server**, then comparing status codes and JSON
bodies after normalising volatile values (UUIDs are numbered by first appearance; timestamps,
JWTs, QR data URLs and TOTP secrets become placeholders).

```
bash tests/contract/run-all.sh
```

What it does:

1. Starts a throwaway `postgres:16-alpine` container (`qb-contract-pg`, port 55432 on
   loopback) and runs the reference's compiled migrations against it.
2. Runs the reference's compiled `dist/main` on port 3900 with environment overrides only:
   scratch database, test JWT secret, local storage in `.scratch`, a test encryption key.
   Nothing is written inside the reference repository.
3. Builds and runs `accounting-server` on port 3901 with a fresh data directory.
4. Runs `run.mjs` and stops both servers.

Requirements: Docker Desktop running, Node 24, the reference repository with its
`node_modules` and an up-to-date `dist/` (`REF_BACKEND` overrides the path).

## Reading the output

- `PASS` — identical after normalisation.
- `DEVN` — an approved deviation (CLAUDE.md §9); both sides are printed for review.
- `FAIL` — a real incompatibility. The run exits non-zero.

W0 result (2026-10-14): 71 steps — 65 identical, 6 approved deviations, 0 mismatches.
W1 result (2026-10-14): 120 steps — 110 identical, 10 approved deviations, 0 mismatches.
W2 result (2026-10-14): 232 steps — 218 identical, 14 approved deviations, 0 mismatches.
After D11–D15 (2026-10-14): 236 steps — 217 identical, 19 approved deviations, 0 mismatches.
Journal lines are compared as a set, because the reference returns them in no fixed order.

Other reference-derived fixtures (regenerate with the scripts in `tools/`):

- `crates/api-server/tests/fixtures/reference_validation.json` — the reference DTOs run through
  Nest's real `ValidationPipe` (`tools/gen_validation_fixture.js`).
- `crates/audit/tests/fixtures/reference_summaries.json` — the reference audit interceptor's
  `summarize` / `detailFrom` (`tools/gen_audit_fixture.mjs`).
- `crates/database/tests/fixtures/reference_pg_*.txt` — columns, constraints and indexes of the
  PostgreSQL database built by the reference migrations (`psql` queries in the W0 report).
