# Headlamp

[Headlamp](https://github.com/headlamp-k8s/headlamp) is the recommended
visual UI for browsing a Kubernetes cluster. It has better CRD support
than the upstream `kubernetes/dashboard` (so our `SparkCluster` CRD
shows up natively) and a single-OCI image, which keeps the install
path simple.

This page is the operator's walkthrough — what gets deployed, why, and
how to verify it's working. The manifest files live in `k8s/headlamp/`
alongside a one-shot `apply.sh` script.

## What gets installed

```text
namespace: headlamp
serviceaccount: headlamp          (cluster-admin via ClusterRoleBinding)
deployment:   headlamp  (1 replica, port 4466)
service:      headlamp  (ClusterIP, targetPort 4466)
```

The Deployment has an init container (`materialize-kubeconfig`) that
reads the projected service-account token at
`/var/run/secrets/kubernetes.io/serviceaccount/`, writes it as a
proper kubeconfig to a shared `emptyDir` mounted at
`/home/headlamp/.kube/`, then the main container picks it up. Without
this indirection, Headlamp starts but its cluster list is empty (it
hard-codes `/home/headlamp/.kube/config` and doesn't look at the
projected token path).

## Install (one shot)

```bash
cd /Users/john/projects/rust-spark
docker pull ghcr.io/headlamp-k8s/headlamp:latest
k3d image import ghcr.io/headlamp-k8s/headlamp:latest -c rspark
./k8s/headlamp/apply.sh
kubectl -n headlamp port-forward svc/headlamp 8099:4466 --address 127.0.0.1
# open http://127.0.0.1:8099
```

The home page lists the in-cluster kubeconfig (`default`) — click it
and Headlamp loads the cluster overview with:

- **Node metrics** (CPU/memory from metrics-server if installed)
- **Workloads** → `Pods`, `Deployments`, `StatefulSets`, etc.
- **Custom Resources** → `SparkCluster` (from `rspark-operator`),
  visible by default because the operator registers the CRD
- **Logs** (in-browser log viewer for any pod)
- **Exec** (open a shell in a running pod)

The SparkCluster CRD's printer columns (Phase, Masters, Workers,
Endpoint, Age) show up automatically because they're defined in the
CRD's `additionalPrinterColumns`.

## Why Headlamp over the upstream `kubernetes/dashboard`

- **Single OCI image** — easier to manage than the dashboard's
  multi-image deployment (dashboard + metrics-scraper).
- **Real CRD support** — custom resources are first-class, not bolted
  on. The operator's `SparkCluster` shows up without manual setup.
- **Pluggable** — plugin system for custom resource views.

The upstream dashboard install is more complex and its image pulls were
fragile in this environment (it ships two images that needed
auth-restricted mirrors). Headlamp was a single `ghcr.io` pull that
worked first try.

## Rolling update

The default Deployment has `replicas: 1`. If you bump the image tag
and `kubectl rollout restart deployment/headlamp -n headlamp`, the pod
restarts and reconnects to the cluster. The init container re-reads the
SA token each time, so credential rotation Just Works.

## Verifying it's working

```bash
# 1. The pod is healthy
kubectl -n headlamp get pods

# 2. The init container ran and wrote the kubeconfig
kubectl -n headlamp logs <pod> -c materialize-kubeconfig | tail -5

# 3. Headlamp's backend sees the cluster
kubectl -n headlamp logs <pod> -c headlamp | grep "Use In Cluster"
# should print: "Use In Cluster: true"  (or at least not "false")

# 4. The API proxy is wired
kubectl -n headlamp port-forward svc/headlamp 8099:4466 --address 127.0.0.1 &
curl -s http://127.0.0.1:8099/clusters/default/api/v1/namespaces | jq '.items[].metadata.name'
```

A correct install returns the 7 namespaces (default, headlamp,
kube-*, kubernetes-dashboard, rspark).

## Customising

- **Tighten the RBAC** — `00-namespace-and-rbac.yaml` grants
  `cluster-admin` to the headlamp SA. For a read-mostly UI replace it
  with the `view` ClusterRole (add `exec` separately if you want
  in-browser exec).
- **Expose publicly** — replace the `ClusterIP` Service with an
  `Ingress` (Traefik is the k3s default) for a stable hostname.
- **Pin the image** — `ghcr.io/headlamp-k8s/headlamp:v0.30.0` is a
  safe choice; replace `:latest` in `01-deployment.yaml` with the tag.

## Related

- `k8s/headlamp/README.md` — the install manifest walkthrough
- `docs/deployment.md` — the full handbook (this page is referenced
  from the k8s/ manifests table)
- `docs/architecture.md` — the cluster topology and where Headlamp
  fits in
