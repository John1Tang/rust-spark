#!/usr/bin/env bash
# Install Kafka (KRaft, single-node) into the running k3d cluster.
# Idempotent. Mirrors k8s/minio/apply.sh.
set -euo pipefail

CLUSTER="${CLUSTER:-rspark}"
IMAGE="${IMAGE:-apache/kafka:3.9.0}"

command -v k3d >/dev/null || { echo "k3d not found (brew install k3d)" >&2; exit 1; }
k3d cluster list 2>/dev/null | grep -qE "^${CLUSTER}\b" \
    || { echo "cluster '$CLUSTER' not running" >&2; exit 1; }

short="${IMAGE##*/}"
if ! docker exec "k3d-${CLUSTER}-server-0" crictl images 2>/dev/null | grep -q "${short%%:*}"; then
    echo "▸ pulling $IMAGE"
    docker pull "$IMAGE"
    k3d image import "$IMAGE" -c "$CLUSTER"
fi

echo "▸ applying manifests"
kubectl apply -f "$(dirname "$0")"

echo
echo "▸ waiting for Kafka pod"
kubectl -n rspark wait --for=condition=ready pod -l app=kafka --timeout=180s

echo
echo "▸ done."
echo
echo "    Brokers (in-cluster):  kafka.rspark.svc.cluster.local:9092"
echo "    Topics:                auto-created on first produce"
echo "    Console (via kubectl):  kubectl -n rspark exec -it statefulset/kafka -- /opt/kafka/bin/kafka-topics.sh --bootstrap-server localhost:9092 --list"
echo "    Outside the cluster:   kubectl -n rspark port-forward svc/kafka 9092:9092 --address 127.0.0.1"