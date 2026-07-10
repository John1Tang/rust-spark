# Local Kafka (KRaft)

`k8s/kafka/` runs Apache Kafka in **KRaft mode** (no ZooKeeper) as a single-node StatefulSet inside the k3d cluster. Apache Kafka 3.9.0 is the image.

KRaft combines the broker and controller roles in a single process. The data dir is formatted on first boot via `kafka-storage.sh format --standalone --cluster-id ...`. The format step is idempotent — a second `apply.sh` does nothing if `meta.properties` already exists.

## Why KRaft

ZooKeeper is being phased out of Kafka. KRaft is the stable, production-ready replacement as of 3.3+. For a single-node learning cluster, KRaft is also simpler: one StatefulSet, one ConfigMap, one Service. No ZooKeeper ensemble to manage.

## Install

```bash
./k8s/kafka/apply.sh
```

The script pulls `apache/kafka:3.9.0` if it isn't already in the cluster, imports it, applies the manifests, and waits for the pod to be ready. It uses `imagePullPolicy: Never` — `k3d image import` is the only way images reach the cluster nodes.

If you also run `./scripts/deploy.sh`, the Kafka image gets pre-pulled automatically so `./k8s/kafka/apply.sh` is a no-op pull.

## Files

- `k8s/kafka/00-config.yaml` — ConfigMap holding `server.properties` (the KRaft config).
- `k8s/kafka/01-statefulset.yaml` — StatefulSet with the formatting init step.
- `k8s/kafka/02-service.yaml` — ClusterIP Service on 9092 (kafka) and 9093 (controller).
- `k8s/kafka/apply.sh` — install script.

## Verify

From inside the cluster:

```bash
kubectl -n rspark exec -it statefulset/kafka -- /opt/kafka/bin/kafka-topics.sh \
    --bootstrap-server localhost:9092 --list
```

From your laptop:

```bash
kubectl -n rspark port-forward svc/kafka 9092:9092 --address 127.0.0.1
# then in another terminal:
rpk -brokers 127.0.0.1:9092 topic list    # if you have redpanda's rpk
```

## Endpoints

- **In-cluster brokers:** `kafka.rspark.svc.cluster.local:9092`
- **Outside the cluster:** `kubectl -n rspark port-forward svc/kafka 9092:9092 --address 127.0.0.1`, then `127.0.0.1:9092`.

## Topics

`auto.create.topics.enable=true` is set in the ConfigMap, so the first produce to a topic creates it. For the demo:

- `rspark.page_events` — page_view, page_scroll, page_click events from the Chrome extension.

## Reset / wipe

```bash
kubectl -n rspark delete statefulset kafka
kubectl -n rspark delete pvc -l app=kafka
kubectl -n rspark apply -f k8s/kafka/
```

Wiping the PVC forces the format step to run again on the next boot.