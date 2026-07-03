# Deployment handbook — k3s / k3d / kubectl

This is the on-ramp for getting rspark running on a Kubernetes cluster. It
covers the local dev loop (k3d), the deploy-on-real-k3s path, and the
kubectl incantations you'll actually need while operating the cluster.

## 1. The TL;DR

```bash
# One-time
brew install k3d                        # if you don't have k3d already
./scripts/cluster-up.sh                 # creates the `rspark` cluster

# Each time you have a change
./scripts/deploy.sh                     # build image, import, rollout restart

# To use the cluster
./scripts/port-forward.sh               # dashboard :8080, API :7077
./scripts/sql.sh "SELECT * FROM employees LIMIT 5"
kubectl -n rspark get pods -w

# optionally: a real k8s dashboard
./k8s/headlamp/apply.sh                 # installs Headlamp at http://127.0.0.1:8099
```

That covers 95% of the work. The rest of this file is the why and the
edge cases.

## 2. What `cluster-up.sh` actually does

```bash
k3d cluster create rspark \
    --api-port 6553 \
    --port "7077:7077@loadbalancer" \
    --port "8080:8080@loadbalancer" \
    --wait
```

- `--api-port 6553` — kubeconfig API on `127.0.0.1:6553` (so it doesn't
  collide with anything else on `:6443`).
- `--port 7077:7077@loadbalancer` — k3d's `k3d-rspark-serverlb` proxy
  listens on host `:7077` and forwards to the master's service `:7077`.
  Mostly useful for ad-hoc `curl` from outside the cluster; we use
  port-forward in normal dev.
- `--port 8080:8080@loadbalancer` — same idea for the dashboard.

Re-run with `--keep` to skip the delete step:
```bash
./scripts/cluster-up.sh --keep
```

To delete the cluster entirely:
```bash
k3d cluster delete rspark
```

## 3. What `deploy.sh` actually does

```
1. docker build -f docker/Dockerfile -t rspark:latest .
2. k3d image import rspark:latest -c rspark
3. kubectl apply -f k8s/                       # idempotent
4. kubectl rollout restart deployment/rspark-master
5. kubectl rollout restart deployment/rspark-worker
6. kubectl rollout status deployment/...        # wait
```

Image tag stays at `:latest` and only the digest changes; `rollout
restart` is the right hammer because `set image` is a no-op when the tag
reference is unchanged.

### The image build

The Dockerfile is multi-stage. The first stage pulls deps + builds the
release binary; the second stage is a slim runtime image. The operator
binary is built into the same image so the operator Deployment can use
the same tag. If you need to build only one binary, target it explicitly:
```bash
docker build -f docker/Dockerfile --target builder -t rspark:debug .
```

## 4. The k8s manifests (`k8s/`)

| File                                 | What                                            |
| ------------------------------------ | ----------------------------------------------- |
| `k8s/00-namespace.yaml`              | The `rspark` namespace                          |
| `k8s/01-configmap.yaml`              | Example data as a ConfigMap (backup)             |
| `k8s/10-master-service.yaml`         | ClusterIP service on 7077 (api) + 8080 (dashboard) |
| `k8s/11-master-deployment.yaml`      | Master Deployment with `RollingUpdate` (maxSurge=1, maxUnavailable=0) |
| `k8s/20-worker-deployment.yaml`      | Worker Deployment (maxSurge=1, maxUnavailable=1) |
| `k8s/30-pod-disruption-budgets.yaml`  | PDBs: minAvailable=1 on master, maxUnavailable=1 on workers |
| `k8s/operator/`                      | SparkCluster CRD + operator (see `docs/operator.md`) |
| `k8s/headlamp/`                      | Headlamp k8s dashboard (see `k8s/headlamp/README.md`) |

### Standard k8s dashboard via Headlamp

Headlamp is the recommended UI for browsing the cluster state — it has
better CRD support than the upstream `kubernetes/dashboard` and the
single-OCI image keeps the image-pull path simple. See
[`k8s/headlamp/README.md`](../k8s/headlamp/README.md) for the install
procedure; the TL;DR is:

```bash
docker pull ghcr.io/headlamp-k8s/headlamp:latest
k3d image import ghcr.io/headlamp-k8s/headlamp:latest -c rspark
./k8s/headlamp/apply.sh
kubectl -n headlamp port-forward svc/headlamp 8099:4466 --address 127.0.0.1
# open http://127.0.0.1:8099
```

The cluster picker on the home page lists the in-cluster kubeconfig
(`default`) — click it to see the full cluster topology, the rspark
operator's `SparkCluster` CRD, and live pod logs.
| `k8s/operator/`                      | SparkCluster CRD + operator (see `docs/operator.md`) |
| `k8s/headlamp/`                      | Headlamp k8s dashboard (see `k8s/headlamp/README.md`) |

### Standard k8s dashboard via Headlamp

Headlamp is the recommended UI for browsing the cluster state — it has
better CRD support than the upstream `kubernetes/dashboard` and the
single-OCI image keeps the image-pull path simple. See
[`k8s/headlamp/README.md`](../k8s/headlamp/README.md) for the install
procedure; the TL;DR is:

```bash
docker pull ghcr.io/headlamp-k8s/headlamp:latest
k3d image import ghcr.io/headlamp-k8s/headlamp:latest -c rspark
./k8s/headlamp/apply.sh
kubectl -n headlamp port-forward svc/headlamp 8099:4466 --address 127.0.0.1
# open http://127.0.0.1:8099
```

The cluster picker on the home page will list the in-cluster
kubeconfig — click it to see the full cluster topology, the rspark
operator's `SparkCluster` CRD, and live pod logs.

Master is `replicas: 1` because state is per-pod in-memory. If you wire
a shared backend into `ClusterState`, you can bump `replicas` and add
session affinity to the service.

### Applying manually

```bash
kubectl apply -f k8s/
# or one file at a time
kubectl apply -f k8s/11-master-deployment.yaml
```

### Deleting everything

```bash
kubectl delete -f k8s/
# or
kubectl delete namespace rspark
```

## 5. The operator (alternative install path)

Instead of writing the `Deployment` YAML by hand, install the operator
and submit a `SparkCluster` CR:

```bash
kubectl apply -f k8s/operator/                  # CRD, RBAC, operator Deployment
kubectl apply -f k8s/operator/30-sparkcluster-demo.yaml
```

See `operator.md` for the full CRD reference.

## 6. kubectl cheat sheet for rspark

```bash
# Pods owned by rspark
kubectl -n rspark get pods

# Tail the master log
kubectl -n rspark logs -l spark.rspark.io/role=master -f

# Tail the operator log (if you installed via the operator)
kubectl -n rspark logs -l app.kubernetes.io/name=rspark-operator -f

# Open a shell inside the master
kubectl -n rspark exec -it deploy/rspark-master -- sh

# Curl the API from inside the cluster
kubectl -n rspark exec deploy/rspark-master -- \
    curl -s http://127.0.0.1:7077/v1/cluster/snapshot

# Force a rolling update of master
kubectl -n rspark rollout restart deployment/rspark-master
kubectl -n rspark rollout status  deployment/rspark-master

# Drain a worker gracefully (PDB will prevent draining the last one)
kubectl -n rspark drain demo-worker-xxx --ignore-daemonsets

# Edit the SparkCluster CR (if using the operator)
kubectl -n rspark edit sparkcluster demo
```

## 7. Production k3s (not k3d)

The `k8s/` manifests apply unchanged to a real k3s cluster. You need to
handle three things differently:

1. **Push the image to a registry.** `imagePullPolicy: Never` is for k3d,
   where the image is loaded by the deploy script. On real k3s, set
   `imagePullPolicy: Always` (or `IfNotPresent` after the first pull)
   and `kubectl apply` against your cluster.

   ```bash
   docker build -f docker/Dockerfile -t your-registry.example.com/rspark:v0.1.0 .
   docker push your-registry.example.com/rspark:v0.1.0
   kubectl -n rspark set image deployment/rspark-master rspark=your-registry.example.com/rspark:v0.1.0
   ```

2. **Use an Ingress** instead of `kubectl port-forward`. The
   `k8s/30-ingress.yaml` file (legacy) used `nginx.ingress.kubernetes.io`.
   k3s ships with `traefik` by default; an Ingress for it looks like:

   ```yaml
   apiVersion: networking.k8s.io/v1
   kind: Ingress
   metadata:
     name: rspark
     namespace: rspark
   spec:
     rules:
       - host: rspark.example.com
         http:
           paths:
             - path: /
               pathType: Prefix
               backend:
                 service:
                   name: rspark-master
                   port:
                     number: 8080
   ```

3. **RBAC for the cluster's service account.** k3s ships sensible
   defaults; you mostly need to make sure the worker ServiceAccount can
   talk to the master API (which it does by default in-cluster).

## 8. Troubleshooting

| Symptom                                   | Fix                                                                |
| ----------------------------------------- | ------------------------------------------------------------------ |
| `ImagePullBackOff`                       | Image wasn't loaded into k3d. Run `./scripts/deploy.sh` again.      |
| `CrashLoopBackOff` on the operator       | Check `kubectl logs` — usually a kubeconfig / RBAC issue.            |
| `connection refused` from a worker to master | DNS or service port issue. `kubectl -n rspark get svc` to verify. |
| Dashboard 404                             | The master pod isn't ready. Wait for `READY 1/1`.                    |
| SQL errors with `Error: not found`        | The table isn't registered. Use `master --load name=path` or `--examples`. |
| Workers show `workers: 0` in snapshot    | Workers registered to a master that's been rolled. Restart workers.  |

### Reading a pod crash

```bash
kubectl -n rspark describe pod rspark-master-xxx | tail -20
kubectl -n rspark logs rspark-master-xxx --previous
```

### Cleaning up

```bash
# Soft reset
kubectl -n rspark delete sparkcluster --all       # operator-managed
kubectl -n rspark delete deployments,pods,services,configmaps -l spark.rspark.io/cluster=demo

# Nuclear
k3d cluster delete rspark
./scripts/cluster-up.sh
```

## 9. CI

`.github/workflows/ci.yml` runs `cargo fmt --check`, `cargo clippy
--workspace --lib --bins -- -D warnings`, `cargo build --workspace
--all-targets`, and `cargo test --workspace --all-targets --no-fail-fast`
on every PR. It does **not** build the docker image or run the operator
against a real cluster — that's left as a follow-up.