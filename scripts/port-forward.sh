#!/usr/bin/env bash
# Forward the master + ingest services to localhost so the dashboard,
# API, and demo-page event ingest are reachable from the host. Hit
# Ctrl+C to stop.
#
#   http://127.0.0.1:8088  →  dashboard
#   http://127.0.0.1:7077  →  master API (used by `rspark submit`)
#   http://127.0.0.1:8081  →  rspark-ingest (the demo page POSTs here)
#
# Note: 8080 is owned by Docker Desktop on this host (its settings API),
# so the dashboard is forwarded to 8088 instead. The script used to use
# 8080 but Docker's IPv6 listener on [::1]:8080 intercepted browser
# requests and returned 404.
set -euo pipefail

NAMESPACE="${NAMESPACE:-rspark}"
exec kubectl -n "$NAMESPACE" port-forward "svc/rspark-master" 7077:7077 8088:8080 --address 127.0.0.1 &
INGEST_PID=$!
# Best-effort ingest forward — don't fail the whole script if ingest
# isn't installed yet (run ./scripts/seed-mock-data.sh first).
kubectl -n "$NAMESPACE" port-forward "svc/rspark-ingest" 8081:8081 --address 127.0.0.1 &
INGEST2_PID=$!
trap "kill $INGEST_PID $INGEST2_PID 2>/dev/null || true" EXIT
wait
