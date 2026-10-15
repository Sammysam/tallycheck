#!/usr/bin/env bash
# Contract test end to end: fresh scratch Postgres in Docker, reference migrations, reference
# API on :3900, new API on :3901 (fresh data dir), then tests/contract/run.mjs.
#
# Nothing is written inside the reference tree: its compiled dist/ is executed as-is with
# environment overrides that point it at the scratch database and test-only secrets.
#
#   REF_BACKEND=/c/Users/.../QuickBooks/backend tests/contract/run-all.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
REF_BACKEND="${REF_BACKEND:-/c/Users/moham/OneDrive/Desktop/Risahd/New folder/Test project/QuickBooks/backend}"
SCRATCH="$ROOT/tests/contract/.scratch"
PG=qb-contract-pg

stop_port() {
  local pid
  pid=$(netstat -ano 2>/dev/null | awk -v p=":$1" '$2 ~ p"$" && $4 == "LISTENING" {print $5; exit}')
  if [ -n "${pid:-}" ]; then taskkill //PID "$pid" //F >/dev/null 2>&1 || true; fi
}

cleanup() {
  stop_port 3900
  stop_port 3901
  stop_port 3902
}
trap cleanup EXIT

cleanup
rm -rf "$SCRATCH" && mkdir -p "$SCRATCH/new-data" "$SCRATCH/ref-uploads"

docker rm -f "$PG" >/dev/null 2>&1 || true
docker run -d --name "$PG" -e POSTGRES_PASSWORD=contract -e POSTGRES_DB=accounting_db -e TZ=UTC \
  -p 127.0.0.1:55432:5432 postgres:16-alpine >/dev/null
for _ in $(seq 1 30); do docker exec "$PG" pg_isready -U postgres >/dev/null 2>&1 && break; sleep 2; done

export DB_HOST=127.0.0.1 DB_PORT=55432 DB_USER=postgres DB_PASS=contract DB_NAME=accounting_db TZ=UTC
(cd "$REF_BACKEND" && node node_modules/typeorm/cli.js migration:run -d dist/common/config/data-source.js >"$SCRATCH/ref-migrate.log" 2>&1)

(cd "$REF_BACKEND" && PORT=3900 NODE_ENV=test \
  JWT_SECRET=contract-test-secret-not-for-production-0123456789 \
  STORAGE_DRIVER=local STORAGE_LOCAL_PATH="$SCRATCH/ref-uploads" \
  STORAGE_ENCRYPTION_KEY=0000000000000000000000000000000000000000000000000000000000000001 \
  ADMIN_USERNAME=admin ADMIN_PASSWORD=admin12345 \
  node dist/main >"$SCRATCH/ref-server.log" 2>&1 &)

(cd "$ROOT" && cargo build -q -p api-server --bin accounting-server)
(cd "$ROOT" && ACCOUNTING_DATA_DIR="$(cygpath -w "$SCRATCH/new-data")" ACCOUNTING_PORT=3901 \
  ADMIN_USERNAME=admin ADMIN_PASSWORD=admin12345 \
  ./target/debug/accounting-server.exe serve >"$SCRATCH/new-server.log" 2>&1 &)

for port in 3900 3901; do
  for _ in $(seq 1 60); do
    code=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$port/api/auth/me" || true)
    [ "$code" = "401" ] && break
    sleep 1
  done
done

set +e
node "$ROOT/tests/contract/run.mjs" http://127.0.0.1:3900 http://127.0.0.1:3901
contract=$?
set -e

# ── Importer: the reference database after the scenario, imported into a fresh data folder,
#    must read back identically through this app (side by side with the reference). ──────────
IMP="$SCRATCH/imported"
mkdir -p "$IMP"
(cd "$ROOT" && cargo build -q -p importer --bin accounting-import)
echo 0000000000000000000000000000000000000000000000000000000000000001 |   ACCOUNTING_DATA_DIR="$(cygpath -w "$IMP")" "$ROOT/target/debug/accounting-server.exe" import-storage-key 2>/dev/null
set +e
"$ROOT/target/debug/accounting-import.exe" --source postgres://postgres:contract@127.0.0.1:55432/accounting_db   --source-tz +00:00 --target "$(cygpath -w "$IMP")" --files "$(cygpath -w "$SCRATCH/ref-uploads")" >"$SCRATCH/import.log" 2>&1
imported=$?
set -e
echo "importer exit code: $imported (report: $IMP/import-report.md)"
(cd "$ROOT" && ACCOUNTING_DATA_DIR="$(cygpath -w "$IMP")" ACCOUNTING_PORT=3902   ./target/debug/accounting-server.exe serve >"$SCRATCH/imported-server.log" 2>&1 &)
for _ in $(seq 1 60); do
  code=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:3902/api/auth/me" || true)
  [ "$code" = "401" ] && break
  sleep 1
done
set +e
node "$ROOT/tests/contract/side_by_side.mjs" http://127.0.0.1:3900 http://127.0.0.1:3902 "$SCRATCH/contract-state.json"
side=$?
set -e
[ "$contract" = 0 ] && [ "$imported" = 0 ] && [ "$side" = 0 ]
