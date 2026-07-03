#!/usr/bin/env bash
# Install Headlamp into the running k3d cluster. Idempotent.
set -euo pipefail

CLUSTER="${CLUSTER:-rspark}"
IMAGE="${IMAGE:-ghcr.io/headlamp-k8s/headlamp:latest}"

command -v k3d >/dev/null || { echo "k3d not found (brew install k3d)" >&2; exit 1; }
k3d cluster list 2>/dev/null | grep -qE "^${CLUSTER}\b" \
    || { echo "cluster '$CLUSTER' not running" >&2; exit 1; }

# Make sure the image is in the k3d node. Pull via ghcr.io then import.
if ! docker exec "k3d-${CLUSTER}-server-0" crictl images 2>/dev/null | grep -q "ghcr.io/headlamp-k8s/headlamp"; then
    echo "▸ pulling $IMAGE"
    docker pull "$IMAGE"
    k3d image import "$IMAGE" -c "$CLUSTER"
fi

echo "▸ applying manifests"
kubectl apply -f "$(dirname "$0")"
echo
echo "▸ waiting for headlamp pod"
kubectl -n headlamp wait --for=condition=ready pod -l app=headlamp --timeout=90s
echo
echo "▸ done. Port-forward with:"
echo "    kubectl -n headlamp port-forward svc/headlamp 8099:4466 --address 127.0.0.1"
echo "    open http://127.0.0.1:8099"
