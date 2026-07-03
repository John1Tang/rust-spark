# Headlamp

A modern alternative to the [Kubernetes dashboard](https://github.com/kubernetes/dashboard) —
slicker UI, in-cluster auth, good CRD support out of the box.

This directory contains the manifests to deploy Headlamp into the local
k3d cluster with **single-image in-cluster auth**: the in-cluster
service-account token is mounted into the pod and a small init
container materializes it as a `kubeconfig` at the path Headlamp's
binary hard-codes (`/home/headlamp/.kube/config`).

## Files

| File                            | What                                    |
| ------------------------------- | --------------------------------------- |
| `00-namespace-and-rbac.yaml`    | Namespace, ServiceAccount, ClusterRoleBinding (cluster-admin) |
| `01-deployment.yaml`             | Headlamp Deployment with the in-cluster token init container |
| `02-service.yaml`                | ClusterIP service on port 4466         |

## Install

```bash
kubectl apply -f k8s/headlamp/
```

The pod will be `1/1 Running` within a few seconds. The init container
writes the SA token into a shared `emptyDir` volume that the main
container reads from.

## Access

```bash
kubectl -n headlamp port-forward svc/headlamp 8099:4466 --address 127.0.0.1
# open http://127.0.0.1:8099
```

The cluster picker on the home page lists the in-cluster kubeconfig
(`default`) — click it and Headlamp loads the cluster overview with
the rspark operator's `SparkCluster` CRD visible under Custom
Resources.

## Why an init container

Headlamp's `headlamp-server` binary opens `/home/headlamp/.kube/config`
at startup. In a typical install you'd mount the kubeconfig from the
host. Inside the cluster, the SA token is auto-mounted at
`/var/run/secrets/kubernetes.io/serviceaccount/`, but Headlamp doesn't
look there. The init container reads the SA token and rewrites it as a
proper kubeconfig at the path Headlamp expects, so the main container
boots with cluster-admin credentials and the UI populates without
manual setup.

The same approach is used by the upstream
[headlamp-k8s/headlamp](https://github.com/headlamp-k8s/headlamp) Helm
chart — we just inline the same trick so we don't need Helm.

## Troubleshooting

- **`/config` returns an empty cluster list** — the init container
  didn't run. Check `kubectl -n headlamp logs <pod> -c materialize-kubeconfig`.
  Most often this means the SA token mount path is wrong.
- **`403 Forbidden` on every API call** — the ClusterRoleBinding isn't
  applied. Check `kubectl get clusterrolebinding headlamp -o yaml`.
- **Pod stuck in `ImagePullBackOff`** — k3d doesn't have the image in
  its local cache. Pull it first:
  ```bash
  docker pull ghcr.io/headlamp-k8s/headlamp:latest
  k3d image import ghcr.io/headlamp-k8s/headlamp:latest -c rspark
  ```
- **"Choose a cluster" page never resolves to a list** — the SPA's
  picker reads from IndexedDB; that works in a real browser but is
  blocked in the Claude Code preview sandbox. Open
  `http://127.0.0.1:8099/` in a real browser to see the full UI.

## Customising

- **No `cluster-admin`?** Replace the `ClusterRoleBinding` with a tighter
  role (e.g. `view` + `exec`). Update the `cluster-admin` reference in
  `00-namespace-and-rbac.yaml`.
- **Pin the image**: replace `latest` with a specific tag (the image
  is published at `ghcr.io/headlamp-k8s/headlamp:v0.30.0` etc.).
- **Expose publicly**: replace the `ClusterIP` Service in
  `02-service.yaml` with an `Ingress` or a `NodePort`.
