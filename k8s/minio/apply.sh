#!/usr/bin/env bash
# Install MinIO into the running k3d cluster. Idempotent.
#
# What this does:
#   1. Pulls `minio/minio:latest` and `minio/mc:latest` and imports
#      them into the k3d cluster (MinIO images aren't on the in-cluster
#      cache by default).
#   2. Applies k8s/minio/ — namespace, secret, StatefulSet, services,
#      and a one-shot Job that creates the `rspark-data` bucket.
#   3. Waits for MinIO to be ready and the bucket Job to succeed.
#   4. Prints the port-forward hint for the console.
set -euo pipefail

CLUSTER="${CLUSTER:-rspark}"
IMAGE="${IMAGE:-minio/minio:latest}"
MC_IMAGE="${MC_IMAGE:-minio/mc:latest}"
BUCKET="${BUCKET:-rspark-data}"
REGION="${AWS_REGION:-us-east-1}"

command -v k3d >/dev/null || { echo "k3d not found (brew install k3d)" >&2; exit 1; }
k3d cluster list 2>/dev/null | grep -qE "^${CLUSTER}\b" \
    || { echo "cluster '$CLUSTER' not running" >&2; exit 1; }

# Make sure the images are in the k3d node.
for img in "$IMAGE" "$MC_IMAGE"; do
    short="${img##*/}"
    if ! docker exec "k3d-${CLUSTER}-server-0" crictl images 2>/dev/null | grep -q "${short%%:*}"; then
        echo "▸ pulling $img"
        docker pull "$img"
        k3d image import "$img" -c "$CLUSTER"
    fi
done

echo "▸ applying manifests"
kubectl apply -f "$(dirname "$0")"

echo
echo "▸ waiting for MinIO pod"
kubectl -n rspark wait --for=condition=ready pod -l app=minio --timeout=120s

echo
echo "▸ waiting for bucket-init Job"
kubectl -n rspark wait --for=condition=complete job/minio-create-bucket --timeout=60s || \
    echo "(bucket-init Job didn't complete; check 'kubectl -n rspark logs job/minio-create-bucket')"

echo
echo "▸ done."
echo "    export AWS_ENDPOINT_URL_S3=http://minio.rspark.svc.cluster.local:9000"
echo "    export AWS_S3_BUCKET=${BUCKET}"
echo "    export AWS_ACCESS_KEY_ID=minio"
echo "    export AWS_SECRET_ACCESS_KEY=minio12345"
echo "    export AWS_REGION=${REGION}"
echo
echo "    Console: kubectl -n rspark port-forward svc/minio-console 9001:9001 --address 127.0.0.1"
echo "             open http://127.0.0.1:9001"