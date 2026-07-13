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
# What this does:
#   1. Builds the rspark:latest image (the master + worker binary).
#   2. Builds the rspark-operator:clean image (the Kubernetes controller).
#   3. Loads both into the named k3d cluster.
#   4. Applies k8s/ (master + worker Deployments) and k8s/operator/
#      (CRD + RBAC + operator Deployment + a sample SparkCluster).
#   5. Rolls both master + worker, then rolls the operator.
#   6. Waits for everything to settle and prints the final state.
#
# Override the cluster name with CLUSTER=rspark-prod ./scripts/deploy.sh
set -euo pipefail

CLUSTER="${CLUSTER:-rspark}"
IMAGE="${IMAGE:-rspark:latest}"
OPERATOR_IMAGE="${OPERATOR_IMAGE:-rspark-operator:clean}"
NAMESPACE="${NAMESPACE:-rspark}"

say() { printf '\033[1;36m▸ %s\033[0m\n' "$*"; }
die() { printf '\033[1;31m✗ %s\033[0m\n' "$*" >&2; exit 1; }

command -v docker >/dev/null || die "docker not found"
command -v kubectl >/dev/null || die "kubectl not found"
command -v k3d >/dev/null    || die "k3d not found (brew install k3d)"

say "checking cluster '$CLUSTER' is up"
k3d cluster list 2>/dev/null | grep -qE "^${CLUSTER}\b" \
    || die "cluster '$CLUSTER' not running. Start it: k3d cluster create $CLUSTER"

# If the build needs Docker Hub, the SOCKS5→HTTP bridge may need to be up.
# The `socks-to-http-bridge` skill provides this; we just check and start
# it here for the operator's own docker build step.
if ! nc -z 127.0.0.1 8888 2>/dev/null; then
    if [ -f "$HOME/.claude/skills/socks-to-http-bridge/socks-to-http.py" ]; then
        say "starting SOCKS5→HTTP bridge (socks-to-http-bridge skill)"
        pkill -f socks-to-http.py 2>/dev/null || true
        nohup python3 "$HOME/.claude/skills/socks-to-http-bridge/socks-to-http.py" \
            > /tmp/socks-to-http.log 2>&1 &
        sleep 1
        nc -z 127.0.0.1 8888 || die "socks-to-http bridge failed to start; see /tmp/socks-to-http.log"
    else
        say "warning: SOCKS5→HTTP bridge not running on 127.0.0.1:8888"
        say "         (install ~/.claude/skills/socks-to-http-bridge if Docker Hub is unreachable)"
    fi
fi

say "building $IMAGE (master + worker)"
# Pass proxy env as empty build args so BuildKit's apt + cargo steps skip
# the Docker daemon's httpProxy when that proxy is unreachable (we hit
# this on networks where the corporate proxy blocks deb.debian.org and
# crates.io, but the VPN tunnel already bypasses it on the host).
docker build \
    --build-arg http_proxy= \
    --build-arg https_proxy= \
    --build-arg HTTP_PROXY= \
    --build-arg HTTPS_PROXY= \
    -f docker/Dockerfile -t "$IMAGE" .

say "importing $IMAGE into k3d cluster '$CLUSTER'"
k3d image import "$IMAGE" -c "$CLUSTER"

# Best-effort: ensure the optional Headlamp image is in the cluster so
# `./k8s/headlamp/apply.sh` works without a separate `docker pull` step.
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
if [ -d "$ROOT/k8s/headlamp" ]; then
    if ! docker exec "k3d-${CLUSTER}-server-0" crictl images 2>/dev/null | grep -q "ghcr.io/headlamp-k8s/headlamp"; then
        say "importing Headlamp image (best-effort)"
        docker pull ghcr.io/headlamp-k8s/headlamp:latest 2>/dev/null || true
        k3d image import ghcr.io/headlamp-k8s/headlamp:latest -c "$CLUSTER" 2>/dev/null || true
    fi
fi

# Best-effort: ensure MinIO + mc are in the cluster so
# `./k8s/minio/apply.sh` works without a separate `docker pull` step.
if [ -d "$ROOT/k8s/minio" ]; then
    for img in minio/minio:latest minio/mc:latest; do
        short="${img%%:*}"
        if ! docker exec "k3d-${CLUSTER}-server-0" crictl images 2>/dev/null | grep -q "${short##*/}"; then
            say "importing ${img} (best-effort)"
            docker pull "$img" 2>/dev/null || true
            k3d image import "$img" -c "$CLUSTER" 2>/dev/null || true
        fi
    done
fi

# Best-effort: ensure the Kafka image is in the cluster so
# `./k8s/kafka/apply.sh` works without a separate `docker pull` step.
if [ -d "$ROOT/k8s/kafka" ]; then
    if ! docker exec "k3d-${CLUSTER}-server-0" crictl images 2>/dev/null | grep -q "apache/kafka"; then
        say "importing apache/kafka:3.9.0 (best-effort)"
        docker pull apache/kafka:3.9.0 2>/dev/null || true
        k3d image import apache/kafka:3.9.0 -c "$CLUSTER" 2>/dev/null || true
    fi
fi

# Detect platform for cross-compile.
ARCH="$(uname -m)"
case "$ARCH" in
    arm64|aarch64)  LINUX_TARGET="aarch64-unknown-linux-musl"; LINUX_GCC="aarch64-linux-musl-gcc" ;;
    x86_64)         LINUX_TARGET="x86_64-unknown-linux-musl";  LINUX_GCC="x86_64-linux-musl-gcc" ;;
    *)              die "unsupported arch: $ARCH" ;;
esac

# musl-cross installs as <arch>-linux-musl-gcc, not the rust target triple.
# BuildKit-less: build the operator binary directly for the linux/musl
# target so the resulting image is portable across architectures.
if command -v "$LINUX_GCC" >/dev/null 2>&1; then
    LINKER_VAR="CARGO_TARGET_$(echo "$LINUX_TARGET" | tr 'a-z-' 'A-Z_')_LINKER"
    env CC="$LINUX_GCC" "$LINKER_VAR"="$LINUX_GCC" \
        cargo build --profile release-fast -p rspark-operator --target "$LINUX_TARGET"
    OPERATOR_BIN="target/${LINUX_TARGET}/release-fast/rspark-operator"
else
    say "  (no $LINUX_GCC found, falling back to host-native build)"
    cargo build --profile release-fast -p rspark-operator
    OPERATOR_BIN="target/release-fast/rspark-operator"
fi

say "building $OPERATOR_IMAGE from $OPERATOR_BIN"
TMPDIR_FOR_BUILD="${TMPDIR:-/tmp}"
TMPDIR_FOR_BUILD=$(echo "$TMPDIR_FOR_BUILD" | sed 's:/private::')
cat > "$TMPDIR_FOR_BUILD/rspark-operator.Dockerfile" <<EOF
FROM ubuntu/squid
COPY $OPERATOR_BIN /usr/local/bin/rspark-operator
ENTRYPOINT ["/usr/local/bin/rspark-operator"]
EOF
docker build -f "$TMPDIR_FOR_BUILD/rspark-operator.Dockerfile" -t "$OPERATOR_IMAGE" .
k3d image import "$OPERATOR_IMAGE" -c "$CLUSTER"

say "applying manifests"
kubectl apply -f k8s/
kubectl apply -f k8s/operator/
# MinIO is opt-in: only apply if the user has the manifests locally.
if [ -d "$ROOT/k8s/minio" ]; then
    kubectl apply -f k8s/minio/ 2>/dev/null || true
fi
# Kafka is opt-in: only apply if the user has the manifests locally.
if [ -d "$ROOT/k8s/kafka" ]; then
    kubectl apply -f k8s/kafka/ 2>/dev/null || true
fi

say "triggering rolling updates"
# rspark-cli runs as the master/worker image, so we restart its pods.
kubectl -n "$NAMESPACE" rollout restart "deployment/rspark-master"
kubectl -n "$NAMESPACE" rollout restart "deployment/rspark-worker"
# The operator Deployment uses a separate image; use set image so the
# rolling update picks up the new tag (rollout restart would also work
# but we want the change to be visible in the deploy log).
kubectl -n "$NAMESPACE" set image "deployment/rspark-operator" "rspark-operator=$OPERATOR_IMAGE"

say "waiting for master rollout"
kubectl -n "$NAMESPACE" rollout status "deployment/rspark-master" --timeout=120s

say "waiting for worker rollout"
kubectl -n "$NAMESPACE" rollout status "deployment/rspark-worker" --timeout=120s

say "waiting for operator rollout"
kubectl -n "$NAMESPACE" rollout status "deployment/rspark-operator" --timeout=120s

say "done"
echo
echo "  pods:"
kubectl -n "$NAMESPACE" get pods -o wide
echo
echo "  SparkCluster status (if using the operator):"
kubectl -n "$NAMESPACE" get sparkcluster 2>/dev/null | tail -n +1 | head -5 || true
echo
echo "  next: ./scripts/port-forward.sh   (dashboard at http://127.0.0.1:8080)"