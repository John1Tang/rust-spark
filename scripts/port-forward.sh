#!/usr/bin/env bash
# Forward the master service to localhost so the dashboard and API are
# reachable from the host. Hit Ctrl+C to stop.
#
#   http://127.0.0.1:8080  →  dashboard
#   http://127.0.0.1:7077  →  master API (used by `rspark submit`)
set -euo pipefail

NAMESPACE="${NAMESPACE:-rspark}"
exec kubectl -n "$NAMESPACE" port-forward "svc/rspark-master" 7077:7077 8080:8080 --address 127.0.0.1
