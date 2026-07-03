# Operator

The rspark operator is a single-binary Kubernetes controller
(`crates/rspark-operator`) built on [kube-rs](https://kube.rs/). It
manages rspark deployments as a single object: the `SparkCluster` CRD.

## The CRD

```yaml
apiVersion: spark.rspark.io/v1alpha1
kind: SparkCluster
metadata:
  name: demo
  namespace: rspark
spec:
  image: rspark:latest
  imagePullPolicy: Never
  master:
    replicas: 1
    examples: true
    load:
      - "users=examples/data/employees.csv"
  workers:
    replicas: 2
    cores: 2
    memoryMb: 1024
```

### Spec fields

| Field             | Type      | Default       | Meaning                                                |
| ----------------- | --------- | ------------- | ------------------------------------------------------ |
| `image`           | string    | (required)    | Container image. `imagePullPolicy: Never` for k3d.   |
| `imagePullPolicy` | string    | `Never`       | Forwarded to every container.                          |
| `master.replicas` | integer   | `1`           | Master replicas. **Don't increase** unless you've wired shared state. |
| `master.examples` | boolean   | `false`       | Preload bundled `employees`, `sales`, `events` data.   |
| `master.load`     | []string  | `[]`          | Extra `--load name=path` registrations.               |
| `master.cpu`      | string    | `200m`        | Master container CPU request/limit.                   |
| `master.memory`   | string    | `256Mi`       | Master container memory request/limit.                |
| `workers.replicas`| integer   | `2`           | Number of worker pods.                                 |
| `workers.cores`   | integer   | `2`           | Cores advertised per worker.                          |
| `workers.memoryMb`| integer   | `1024`        | Memory (MiB) advertised per worker.                   |
| `workers.cpu`     | string    | `200m`        | Worker container CPU request/limit.                   |
| `workers.memory`  | string    | `256Mi`       | Worker container memory request/limit.                |

### Status

```yaml
status:
  phase: Ready | Reconciling | Pending | Failed
  masterEndpoint: demo-master.rspark.svc.cluster.local:7077
  readyMasters: 1
  readyWorkers: 2
  conditions: []
  lastReconciledAt: 2026-07-03T05:11:00Z
```

`phase` is computed from the count of Ready pods vs the desired
replica count. `lastReconciledAt` is updated every 30s (the
requeue interval).

The CRD also has printer columns so `kubectl get sparkcluster` shows:

```
NAME   PHASE    MASTERS   WORKERS   ENDPOINT                                            AGE
demo   Ready    1         2         demo-master.rspark.svc.cluster.local:7077          3m
```

## What the controller owns

For each `SparkCluster`, the controller reconciles:

- **ServiceAccount** (`<name>-rspark`) — shared by master + workers.
- **Service** (`<name>-master`) — ClusterIP on 7077 (api) + 8080 (dashboard).
- **ConfigMap** (`<name>-master-config`) — placeholder for future
  per-cluster config.
- **Deployment** (`<name>-master`) — `replicas: 1`, RollingUpdate
  `maxSurge: 1, maxUnavailable: 0`, startup + readiness + liveness
  probes against `/health`.
- **Deployment** (`<name>-worker`) — `replicas: 2` by default,
  RollingUpdate `maxSurge: 1, maxUnavailable: 1`.
- **PodDisruptionBudget** (`<name>-master-pdb`) —
  `minAvailable: 1`.
- **PodDisruptionBudget** (`<name>-worker-pdb`) —
  `maxUnavailable: 1`.

Every child carries an `ownerReference` to the `SparkCluster`, so
deleting the CR garbage-collects the children.

## Install

```bash
kubectl apply -f k8s/operator/
```

The operator is a single binary (`rspark-operator`) running in its own
Deployment. Manifests:

- `k8s/operator/00-sparkcluster-crd.yaml` — the CRD.
- `k8s/operator/10-rbac.yaml` — ServiceAccount + ClusterRole +
  ClusterRoleBinding. Watches the CRD and owns the child types.
- `k8s/operator/20-operator-deployment.yaml` — the operator pod
  itself.
- `k8s/operator/30-sparkcluster-demo.yaml` — a sample CR.

## Examples

### Submit a demo cluster

```bash
kubectl apply -f k8s/operator/30-sparkcluster-demo.yaml
kubectl -n rspark get sparkcluster -w
```

After a few seconds:
```
NAME   PHASE    MASTERS   WORKERS   ENDPOINT
demo   Ready    1         2         demo-master.rspark.svc.cluster.local:7077
```

### Scale workers up

```bash
kubectl -n rspark patch sparkcluster demo \
    --type merge \
    -p '{"spec":{"workers":{"replicas":4}}}'
```

The operator will reconcile and add two more worker pods (RollingUpdate).

### Register an extra table

```bash
kubectl -n rspark patch sparkcluster demo \
    --type merge \
    -p '{"spec":{"master":{"load":["prod=examples/data/sales.csv"]}}}'
```

The operator will restart the master pod to pick up the new `--load`
flag.

### Delete the cluster

```bash
kubectl -n rspark delete sparkcluster demo
```

Watch the children disappear:

```bash
kubectl -n rspark get pods -l spark.rspark.io/cluster=demo -w
```

## Known limitations

- **Single-master by design.** State is per-pod in memory; running
  multiple masters would split state. Don't set `master.replicas > 1`
  without first wiring `ClusterState` to a shared backend.
- **Workers don't re-register after a master rolling restart.** This
  is a pre-existing rspark limitation (the worker code only registers
  once at startup). If you `kubectl rollout restart
  deployment/<name>-master`, the workers will idle until you also
  restart them.
- **No in-place restart on load changes.** Adding a table via
  `spec.master.load` requires a master pod restart. The operator does
  trigger this via `RolloutRequested` because the pod template
  changes, but it's a hard restart.

## Architecture inside the operator

```
   ┌──────────────────┐
   │ SparkCluster CR  │
   └──────────────────┘
            │
            ▼
   ┌──────────────────┐
   │ reconcile()      │  kube::runtime::Controller
   └──────────────────┘
            │
            ├──▶ ensure_service_account()
            ├──▶ ensure_master_service()
            ├──▶ ensure_master_configmap()
            ├──▶ ensure_master_deployment()
            ├──▶ ensure_worker_deployment()
            ├──▶ ensure_pdbs()
            └──▶ apply_status()               ← writes .status

   All child updates use Patch::Apply with ownerReferences set.
   404 falls through to Create, so the operator is purely idempotent.
```

The controller requeues every 30s so the status reflects rolling
updates without an external event source.

## Operator + non-operator deployments

The two ways of deploying rspark on the same cluster:

```bash
# Option A: hand-written YAML (k8s/00-…yaml files)
kubectl apply -f k8s/

# Option B: operator-managed
kubectl apply -f k8s/operator/00-sparkcluster-crd.yaml
kubectl apply -f k8s/operator/10-rbac.yaml
kubectl apply -f k8s/operator/20-operator-deployment.yaml
kubectl apply -f k8s/operator/30-sparkcluster-demo.yaml
```

They will collide if you run both — the hand-written Deployments have
different names from the operator's. Pick one.

## Where to read the code

- `crates/rspark-operator/src/crd.rs` — the CRD type definitions.
- `crates/rspark-operator/src/controller.rs` — the reconciler
  (~600 lines). Each `ensure_*` function builds one Kubernetes object.
- `crates/rspark-operator/tests/crd.rs` — a tiny integration test that
  round-trips a `SparkCluster` through serde.