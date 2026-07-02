#!/usr/bin/env bash
# Build, push, and roll the rspark image into the local k3d cluster.
#
# This is the loop you run after every feature / improvement:
#
#   1. Edit code, run `cargo test --workspace`
#   2. ./scripts/deploy.sh
#   3. Watch the rolling update finish: `kubectl -n rspark get pods -w`
#   4. Use the cluster: ./scripts/port-forward.sh  (opens dashboard at http://127.0.0.1:8080)
#
# Behaviour:
#   - Builds the rspark:latest image (multi-stage Dockerfile in docker/).
#   - Loads it into the named k3d cluster (k3d cluster list to see yours).
#   - Triggers a rolling update on the master and worker Deployments
#     via `kubectl set image`, so the pod template changes even when
#     the tag stays at `:latest`. (kubectl rollout restart would also
#     work, but set image is more explicit about which field changed.)
#   - Waits for both rollouts to finish. Exits non-zero on failure so
#     CI can rely on it.
#
# Override the cluster name with CLUSTER=rspark-prod ./scripts/deploy.sh
set -euo pipefail

CLUSTER="${CLUSTER:-rspark}"
IMAGE="${IMAGE:-rspark:latest}"
NAMESPACE="${NAMESPACE:-rspark}"
CTX="k3d-${CLUSTER}"

say() { printf '\033[1;36m▸ %s\033[0m\n' "$*"; }
die() { printf '\033[1;31m✗ %s\033[0m\n' "$*" >&2; exit 1; }

command -v docker >/dev/null || die "docker not found"
command -v kubectl >/dev/null || die "kubectl not found"
command -v k3d >/dev/null    || die "k3d not found (brew install k3d)"

say "checking cluster '$CLUSTER' is up"
k3d cluster list 2>/dev/null | grep -qE "^${CLUSTER}\b" \
    || die "cluster '$CLUSTER' not running. Start it: k3d cluster create $CLUSTER"

say "building $IMAGE (this is the slow step the first time)"
docker build -f docker/Dockerfile -t "$IMAGE" .

say "importing $IMAGE into k3d cluster '$CLUSTER'"
k3d image import "$IMAGE" -c "$CLUSTER"

say "ensuring namespace + manifests are applied"
kubectl apply -f k8s/

say "triggering rolling update on master + worker"
# `kubectl set image` only triggers a new ReplicaSet when the pod
# template actually changes. With a moving `:latest` tag (rebuilt
# each deploy), the image reference is unchanged and `set image` is a
# no-op. `kubectl rollout restart` is the right hammer here: it
# patches an annotation on the pod template to force a new ReplicaSet
# without touching the image reference, so the new container image
# (just imported by k3d) is what actually gets pulled.
kubectl -n "$NAMESPACE" rollout restart "deployment/rspark-master"
kubectl -n "$NAMESPACE" rollout restart "deployment/rspark-worker"

say "waiting for master rollout"
kubectl -n "$NAMESPACE" rollout status "deployment/rspark-master" --timeout=120s

say "waiting for worker rollout"
kubectl -n "$NAMESPACE" rollout status "deployment/rspark-worker" --timeout=120s

say "done"
echo
echo "  pods:"
kubectl -n "$NAMESPACE" get pods -o wide
echo
echo "  next: ./scripts/port-forward.sh   (dashboard at http://127.0.0.1:8080)"
