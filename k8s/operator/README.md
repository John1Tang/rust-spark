# Top-level `kubectl apply -f k8s/operator/` order:
#   1. CRD           (k8s/operator/00-sparkcluster-crd.yaml)
#   2. RBAC          (k8s/operator/10-rbac.yaml)
#   3. Operator      (k8s/operator/20-operator-deployment.yaml)
#   4. SparkCluster  (k8s/operator/30-sparkcluster-demo.yaml)
#
# One-shot bootstrap:
#   kubectl apply -f k8s/operator/
#
# Watch the operator reconcile:
#   kubectl -n rspark logs -l app.kubernetes.io/name=rspark-operator -f
#   kubectl -n rspark get sparkcluster demo -w
#   kubectl -n rspark get pods -l spark.rspark.io/cluster=demo
#
# Tear down a cluster (the operator will GC all children):
#   kubectl -n rspark delete sparkcluster demo