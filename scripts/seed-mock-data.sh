#!/usr/bin/env bash
# Seed MinIO with mock batch + streaming-table data and register the
# batch tables in the master's catalog so they show up in the dashboard
# autocomplete.
#
# Idempotent. Run on the host with the cluster up:
#
#   ./scripts/seed-mock-data.sh
#
# What it does:
#   1. Uses the running `minio-create-bucket` init pattern — but instead
#      of waiting for the (currently broken) Job, just curls MinIO's
#      health endpoint via kubectl exec and then runs `mc mb` against the
#      `rspark-data` bucket.
#   2. Uploads `examples/data/orders.csv` and `examples/data/users.csv`
#      to `s3://rspark-data/batch/` so a pipeline with an S3 source can
#      read them.
#   3. Uploads `examples/data/clickstream.jsonl` to `s3://rspark-data/
#      streaming/clickstream.jsonl` (the streaming-table seed).
#   4. Calls `POST /v1/catalog/tables` against the master to register
#      the two batch CSVs as catalog tables (kind = batch).
#
# The streaming table appears in autocomplete only after a pipeline run
# registers its flow output via `register_with_kind` — there's no shortcut
# around the planner there.
set -euo pipefail

NAMESPACE="${NAMESPACE:-rspark}"
BUCKET="${BUCKET:-rspark-data}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EXAMPLES="$ROOT/examples/data"

say()  { printf '\033[1;36m▸ %s\033[0m\n' "$*"; }
fail() { printf '\033[1;31m✗ %s\033[0m\n' "$*" >&2; exit 1; }

command -v kubectl >/dev/null || fail "kubectl not found"
command -v mc >/dev/null || say "warning: mc not on PATH (will use kubectl exec mc from minio pod)"

say "checking cluster connectivity"
kubectl -n "$NAMESPACE" get pods -l app=minio -o name >/dev/null \
    || fail "no minio pod in namespace $NAMESPACE"

MINIO_POD="$(kubectl -n "$NAMESPACE" get pods -l app=minio -o name | head -1 | cut -d/ -f2)"
[ -n "$MINIO_POD" ] || fail "could not resolve minio pod name"

say "waiting for MinIO health"
for _ in $(seq 1 30); do
    if kubectl -n "$NAMESPACE" exec "$MINIO_POD" -- curl -sf http://127.0.0.1:9000/minio/health/ready >/dev/null 2>&1; then
        break
    fi
    sleep 2
done

# All `mc` calls run inside the minio pod via `kubectl exec`. The minio
# image ships `mc` at /usr/bin/mc; running it in-cluster avoids any
# host install and avoids the `--rm` pod-scheduling race.
mc_run() {
    kubectl -n "$NAMESPACE" exec -i "$MINIO_POD" -- /usr/bin/mc "$@"
}

say "ensuring bucket $BUCKET exists"
mc_run alias set local "http://minio.$NAMESPACE.svc.cluster.local:9000" minio minio12345 >/dev/null
if ! mc_run ls "local/$BUCKET" >/dev/null 2>&1; then
    mc_run mb "local/$BUCKET"
    say "  created bucket $BUCKET"
else
    say "  bucket $BUCKET already present"
fi

say "uploading batch CSVs to s3://$BUCKET/batch/"
for f in "$EXAMPLES"/users.csv "$EXAMPLES"/orders.csv; do
    [ -f "$f" ] || fail "missing fixture: $f"
    # `mc cp <host-path>` doesn't work via kubectl exec because the
    # host file isn't visible inside the pod. Use `mc pipe` instead —
    # cat the local file into mc stdin and write to the bucket.
    kubectl -n "$NAMESPACE" exec -i "$MINIO_POD" -- /usr/bin/mc pipe "local/$BUCKET/batch/$(basename "$f")" <"$f" >/dev/null
done

# Resolve the master pod up front so we can `kubectl cp` the freshly
# expanded fixtures into the pod below. The image-baked copies are the
# original tiny demo files (10 users, 15 orders, 19 events).
RSPARK_POD="$(kubectl -n "$NAMESPACE" get pods -l role=master -o name | head -1 | cut -d/ -f2)"
[ -n "$RSPARK_POD" ] || fail "no rspark-master pod found (looked for label role=master)"
say "copying fresh batch CSVs into master pod $RSPARK_POD"
for f in "$EXAMPLES"/users.csv "$EXAMPLES"/orders.csv; do
    kubectl -n "$NAMESPACE" cp "$f" "$RSPARK_POD:/app/examples/data/$(basename "$f")" >/dev/null
done

say "uploading streaming-table seed to s3://$BUCKET/streaming/"
[ -f "$EXAMPLES/clickstream.jsonl" ] || fail "missing $EXAMPLES/clickstream.jsonl"
kubectl -n "$NAMESPACE" exec -i "$MINIO_POD" -- /usr/bin/mc pipe "local/$BUCKET/streaming/clickstream.jsonl" <"$EXAMPLES/clickstream.jsonl" >/dev/null

# Also push the new clickstream into the master pod so the pipeline's
# local `kind: json` source can read it. The image-baked copy is the
# original 19-row demo file; the pipeline runner doesn't yet support
# `kind: s3` sources, so this copy is what makes the dashboard's
# "stream × batch join" actually have 1500 rows to join against.
RSPARK_POD="$(kubectl -n "$NAMESPACE" get pods -l role=master -o name | head -1 | cut -d/ -f2)"
say "copying fresh clickstream.jsonl into master pod $RSPARK_POD"
kubectl -n "$NAMESPACE" cp "$EXAMPLES/clickstream.jsonl" "$RSPARK_POD:/app/examples/data/clickstream.jsonl" >/dev/null
say "  pod sees $(kubectl -n "$NAMESPACE" exec "$RSPARK_POD" -- wc -l < /app/examples/data/clickstream.jsonl 2>/dev/null || echo '?') lines"

say "registering batch tables in the master catalog"
say "  master pod $RSPARK_POD"

# Re-resolve the master pod name in case a deploy rolled it between the
# `kubectl cp` above and the catalog calls below. Each call that needs
# the pod re-resolves it via the same selector.
fresh_pod() {
    kubectl -n "$NAMESPACE" get pods -l role=master -o name | head -1 | cut -d/ -f2
}

# Wait for the master's own /healthz before curling it. Running curl
# INSIDE the master pod hits localhost:7077 directly, which avoids
# `kubectl port-forward` racing against a Terminating sibling pod.
for _ in $(seq 1 30); do
    RSPARK_POD="$(fresh_pod)"
    if [ -n "$RSPARK_POD" ] && \
       kubectl -n "$NAMESPACE" exec "$RSPARK_POD" -- curl -sf http://127.0.0.1:7077/healthz >/dev/null 2>&1; then
        break
    fi
    sleep 1
done

for t in users orders; do
    RSPARK_POD="$(fresh_pod)"
    body="{\"name\":\"$t\",\"path\":\"/app/examples/data/$t.csv\",\"source\":\"csv\"}"
    if kubectl -n "$NAMESPACE" exec -i "$RSPARK_POD" -- \
        /bin/sh -c "printf '%s' '$body' | curl -sf -X POST -H 'Content-Type: application/json' --data-binary @- http://127.0.0.1:7077/v1/catalog/tables"
    then
        say "  registered $t"
    else
        say "  (re)registration of $t skipped"
    fi
done

# Pre-register click_events as a regular batch table so the pipeline's
# `FROM click_events` resolves on first run. The pipeline then re-registers
# it with kind=streaming_table via register_with_kind. A rolling restart
# wipes the in-memory catalog, so this block re-establishes it idempotently.
RSPARK_POD="$(fresh_pod)"
ce_body='{"name":"click_events","path":"/app/examples/data/clickstream.jsonl","source":"json"}'
kubectl -n "$NAMESPACE" exec -i "$RSPARK_POD" -- \
    /bin/sh -c "printf '%s' '$ce_body' | curl -sf -X POST -H 'Content-Type: application/json' --data-binary @- http://127.0.0.1:7077/v1/catalog/tables" \
    >/dev/null 2>&1 || say "  click_events pre-registration failed (continuing)"

say "re-promoting click_events to kind=streaming_table via pipeline"
PIPELINE_PATH="/tmp/clickstream_aggregator.yaml"
RSPARK_POD="$(fresh_pod)"
# Drop the YAML into the master pod so /v1/pipelines can read it locally.
kubectl -n "$NAMESPACE" cp "$ROOT/examples/pipelines/clickstream_aggregator.yaml" \
    "$RSPARK_POD:$PIPELINE_PATH" >/dev/null
RSPARK_POD="$(fresh_pod)"
if kubectl -n "$NAMESPACE" exec -i "$RSPARK_POD" -- \
    curl -sf -X POST -H 'Content-Type: application/yaml' \
    --data-binary "@$PIPELINE_PATH" \
    http://127.0.0.1:7077/v1/pipelines >/dev/null
then
    say "  pipeline ran; re-pointing click_events back at the raw JSONL"
    # The pipeline runner registers the flow output (a pipe-delimited
    # `kind: csv` file) in the catalog, but the CsvSource uses comma
    # delimiter, so re-reading it via the catalog yields a 1-column
    # schema mismatch. Point click_events back at the raw NDJSON, which
    # the JsonSource can read losslessly. Pass `kind: streaming_table`
    # so the catalog re-registration keeps the streaming-table kind
    # (the default would demote it back to batch).
    RSPARK_POD="$(fresh_pod)"
    ce_back='{"name":"click_events","path":"/app/examples/data/clickstream.jsonl","source":"json","kind":"streaming_table"}'
    if kubectl -n "$NAMESPACE" exec -i "$RSPARK_POD" -- \
        /bin/sh -c "printf '%s' '$ce_back' | curl -sf -X POST -H 'Content-Type: application/json' --data-binary @- http://127.0.0.1:7077/v1/catalog/tables" >/dev/null
    then
        say "  click_events is now a streaming_table (raw JSONL source)"
    else
        say "  could not repoint click_events — join sample will fail"
    fi
else
    say "  (pipeline run failed — click_events will stay batch; rerun ./scripts/seed-mock-data.sh)"
fi

# Start the live Kafka tail pipeline (tail of the rspark.page_events
# topic → /tmp/rspark/live/click_events.ndjson). Returns 202 + a
# status URL; the runner keeps polling forever. Optional — if Kafka /
# ingest aren't up, we still finish the static seed successfully.
LIVE_YAML="$ROOT/examples/pipelines/clickstream_live.yaml"
if [ -f "$LIVE_YAML" ]; then
    say "starting live clickstream pipeline (Kafka → /tmp/rspark/live/click_events.ndjson)"
    RSPARK_POD="$(fresh_pod)"
    kubectl -n "$NAMESPACE" cp "$LIVE_YAML" "$RSPARK_POD:/tmp/clickstream_live.yaml" >/dev/null 2>&1 || true
    RSPARK_POD="$(fresh_pod)"
    live_resp="$(kubectl -n "$NAMESPACE" exec -i "$RSPARK_POD" -- \
        curl -sf -X POST -H 'Content-Type: application/yaml' \
        --data-binary @/tmp/clickstream_live.yaml \
        http://127.0.0.1:7077/v1/pipelines 2>&1 || true)"
    if echo "$live_resp" | grep -q '"status":"started"'; then
        say "  live pipeline accepted (202); check /v1/pipelines/clickstream_live/status"
        # Re-point click_events at the live NDJSON destination so SQL
        # Lab can join against the running tail. The static seed above
        # pointed at the static file; the live tail writes to a
        # different path. Both are valid; live wins by default.
        RSPARK_POD="$(fresh_pod)"
        ce_live='{"name":"click_events","path":"/tmp/rspark/live/click_events.ndjson","source":"json","kind":"streaming_table"}'
        if kubectl -n "$NAMESPACE" exec -i "$RSPARK_POD" -- \
            /bin/sh -c "printf '%s' '$ce_live' | curl -sf -X POST -H 'Content-Type: application/json' --data-binary @- http://127.0.0.1:7077/v1/catalog/tables" >/dev/null 2>&1
        then
            say "  click_events is now backed by the live NDJSON tail"
        else
            say "  (could not repoint click_events to live tail — falling back to static)"
        fi
    else
        say "  (live pipeline not started — Kafka or rspark-ingest may be missing)"
    fi
else
    say "  (clickstream_live.yaml not present; skipping live tail)"
fi

say "done"
echo
echo "  contents of s3://$BUCKET:"
mc_run ls --recursive "local/$BUCKET" | sed 's/^/    /'
echo
echo "  next: visit http://127.0.0.1:8088 (after ./scripts/port-forward.sh)"
echo "         and click SQL Lab → 'stream × batch join' under Examples."
echo "         Click 'open event collector ↗' to start generating live page events."