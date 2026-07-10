# Local MinIO + S3 path

`rspark` reads and writes tables through an `s3://` URI when the runtime is
configured with an S3-compatible endpoint. The cheapest way to exercise that
path on a laptop is to run a single-node MinIO StatefulSet inside the same k3d
cluster that already hosts the master and workers.

## Install

```bash
./k8s/minio/apply.sh
```

The script pulls `minio/minio:latest` and `minio/mc:latest`, imports them into
k3d (the image cache does not have them by default), applies the manifests in
`k8s/minio/`, waits for the MinIO pod to become ready, waits for the bucket
Job to create `rspark-data`, and prints the env exports you need on the
client side.

After it returns you should see:

- `kubectl -n rspark get pods -l app=minio` → `Running`
- `kubectl -n rspark get job minio-create-bucket` → `Complete`
- A service `minio.rspark.svc.cluster.local` reachable from inside the cluster

## Console

```bash
kubectl -n rspark port-forward svc/minio-console 9001:9001 --address 127.0.0.1
open http://127.0.0.1:9001
```

Default credentials are `minio` / `minio12345`. The bucket `rspark-data` is
created on first install.

## Pointing rspark at it

The master container is already wired with the right env vars by `k8s/11-master-deployment.yaml`:

| Env | Value |
| --- | --- |
| `AWS_S3_BUCKET` | `rspark-data` |
| `AWS_REGION` | `us-east-1` |
| `AWS_ENDPOINT_URL_S3` | `http://minio.rspark.svc.cluster.local:9000` |
| `AWS_ACCESS_KEY_ID` | from `minio-credentials` secret |
| `AWS_SECRET_ACCESS_KEY` | from `minio-credentials` secret |

When the master process boots it calls `try_register_s3` against the
`SourceRegistry`; if `AWS_S3_BUCKET` is set, `s3://` paths become readable from
the SQL layer exactly like `file://` paths.

## Reading from SQL

After MinIO is up, push a CSV into the bucket:

```bash
kubectl -n rspark port-forward svc/minio 9000:9000 --address 127.0.0.1 &
mc alias set local http://127.0.0.1:9000 minio minio12345
mc cp examples/data/employees.csv local/rspark-data/employees.csv
```

Then, with the dashboard port-forwarded, register the table and query:

```bash
./scripts/sql.sh "CREATE TABLE employees USING csv OPTIONS (path 's3://rspark-data/employees.csv')"
./scripts/sql.sh "SELECT count(*) FROM employees"
```

(Phase 3 will introduce the declarative-pipeline runner that writes pipeline
output to S3 automatically.)

## Writing from SQL

`s3://` paths work as `OutputWriter` destinations too — for example the
`SHOW CREATE TABLE` shortcut outputs a SQL string; in the pipeline runner
phase, materialised-view results land at `s3://rspark-data/<flow>.csv`.

## Tearing it down

```bash
kubectl -n rspark delete -f k8s/minio/
```

The bucket data lives on a PVC; `kubectl delete pvc -n rspark data-minio-0`
removes the persistent state.

## Why the async `DataSource` slot

The S3 source implements `AsyncDataSource` (`crates/rspark-storage/src/source.rs:18-24`)
rather than blocking inside the sync `DataSource::scan` impl. This way the
tokio reactor stays responsive while S3 GETs are in flight, and the same slot
in the registry can later carry Kafka or Arrow Flight sources that have no
useful sync representation.