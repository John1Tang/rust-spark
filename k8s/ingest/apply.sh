#!/usr/bin/env bash
# Install rspark-ingest into the running k3d cluster.
# Idempotent. Mirrors k8s/minio/apply.sh and k8s/kafka/apply.sh.
set -euo pipefail

CLUSTER="${CLUSTER:-rspark}"

command -v k3d >/dev/null || { echo "k3d not found (brew install k3d)" >&2; exit 1; }
k3d cluster list 2>/dev/null | grep -qE "^${CLUSTER}\b" \
    || { echo "cluster '$CLUSTER' not running" >&2; exit 1; }

echo "▸ applying manifests"
kubectl apply -f "$(dirname "$0")"

echo
echo "▸ waiting for rspark-ingest pod"
kubectl -n rspark wait --for=condition=ready pod -l app=rspark-ingest --timeout=120s

echo
echo "▸ done."
echo
echo "    Inside cluster: http://rspark-ingest.rspark.svc.cluster.local:8081"
echo "    Port-forward:   kubectl -n rspark port-forward svc/rspark-ingest 8081:8081 --address 127.0.0.1"