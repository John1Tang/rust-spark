#!/usr/bin/env bash
# Run a SQL query against the cluster. Pass the SQL as the first
# argument, or use --file to read from a path. Example:
#
#   ./scripts/sql.sh "SELECT COUNT(*) FROM employees"
#   ./scripts/sql.sh --file query.sql
set -euo pipefail

NAMESPACE="${NAMESPACE:-rspark}"
PORT="${PORT:-7077}"

if [[ "${1:-}" == "--file" ]]; then
    payload=$(jq -Rs --arg sql "$(cat "$2")" '{sql: $sql}')
else
    payload=$(jq -Rs --arg sql "$1" '{sql: $sql}')
fi

exec curl -sS -X POST \
    -H "content-type: application/json" \
    -d "$payload" \
    "http://127.0.0.1:${PORT}/v1/sql" | jq
