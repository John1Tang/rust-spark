#!/usr/bin/env bash
# Bring the local k3d cluster up from scratch (or back if it was
# stopped). Idempotent: re-running while the cluster is up is a no-op.
#
# Removes the cluster first so a stale state from a previous experiment
# doesn't haunt the next deploy. Pass --keep to skip the delete.
set -euo pipefail

CLUSTER="${CLUSTER:-rspark}"
KEEP=0
for arg in "$@"; do
    case "$arg" in
        --keep) KEEP=1 ;;
        *) echo "unknown arg: $arg" >&2; exit 1 ;;
    esac
done

if k3d cluster list 2>/dev/null | grep -qE "^${CLUSTER}\b"; then
    if [[ "$KEEP" -eq 1 ]]; then
        echo "cluster '$CLUSTER' already running; --keep, nothing to do"
        exit 0
    fi
    echo "deleting existing cluster '$CLUSTER'"
    k3d cluster delete "$CLUSTER"
fi

echo "creating cluster '$CLUSTER'"
k3d cluster create "$CLUSTER" \
    --api-port 6553 \
    --port "7077:7077@loadbalancer" \
    --port "8080:8080@loadbalancer" \
    --wait

echo
echo "  kubectl context: $(kubectl config current-context)"
echo "  next: ./scripts/deploy.sh"
