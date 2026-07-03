use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `SparkCluster` is the rspark custom resource. One CR == one rspark
/// deployment in the cluster (one master + N workers).
///
/// ```yaml
/// apiVersion: spark.rspark.io/v1alpha1
/// kind: SparkCluster
/// metadata:
///   name: demo
/// spec:
///   image: rspark:latest
///   imagePullPolicy: Never
///   master:
///     replicas: 1
///     examples: true
///     load: ["users=/data/users.csv"]
///   workers:
///     replicas: 2
///     cores: 2
///     memoryMb: 1024
/// ```
#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[kube(
    group = "spark.rspark.io",
    version = "v1alpha1",
    kind = "SparkCluster",
    namespaced
)]
#[kube(status = "SparkClusterStatus", shortname = "rspark")]
#[serde(rename_all = "camelCase")]
pub struct SparkClusterSpec {
    /// Container image. Defaults to `rspark:latest`. `imagePullPolicy`
    /// should be `Never` for k3d-style setups where the image is
    /// loaded directly into the cluster.
    #[serde(default = "default_image")]
    pub image: String,

    /// Image pull policy passed through to every owned container.
    /// Defaults to `Never` so the operator works out-of-the-box on k3d.
    #[serde(default = "default_pull_policy")]
    pub image_pull_policy: String,

    #[serde(default)]
    pub master: MasterSpec,

    #[serde(default)]
    pub workers: WorkersSpec,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct MasterSpec {
    /// Master replicas. Defaults to 1 because the master's in-memory
    /// state isn't shared across pods. Increase only if you wire a
    /// shared backend (etcd / redis) into `ClusterState`.
    #[serde(default = "default_master_replicas")]
    pub replicas: i32,

    /// Preload the bundled mock data (`employees`, `sales`, `events`)
    /// at startup. Equivalent to `rspark-cli master --examples`.
    #[serde(default)]
    pub examples: bool,

    /// Extra tables to register at startup. Format: `name=path`.
    /// Equivalent to `rspark-cli master --load name=path`.
    #[serde(default)]
    pub load: Vec<String>,

    /// CPU request. Default: 200m.
    #[serde(default = "default_master_cpu")]
    pub cpu: String,

    /// Memory request. Default: 256Mi.
    #[serde(default = "default_master_memory")]
    pub memory: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkersSpec {
    /// Worker replicas. Defaults to 2.
    #[serde(default = "default_worker_replicas")]
    pub replicas: i32,

    /// CPUs to advertise per worker. Default: 2.
    #[serde(default = "default_cores")]
    pub cores: i32,

    /// Memory (MiB) to advertise per worker. Default: 1024.
    #[serde(default = "default_memory_mb")]
    pub memory_mb: i32,

    #[serde(default = "default_worker_cpu")]
    pub cpu: String,

    #[serde(default = "default_worker_memory")]
    pub memory: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SparkClusterStatus {
    /// High-level lifecycle phase.
    pub phase: Phase,

    /// `rspark-master.<namespace>.svc.cluster.local:7077` once the
    /// master service exists. Empty until first reconciliation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub master_endpoint: Option<String>,

    /// Number of worker pods currently Ready.
    #[serde(default)]
    pub ready_workers: i32,

    /// Number of master pods currently Ready.
    #[serde(default)]
    pub ready_masters: i32,

    /// Conditions follows the Kubernetes API conventions
    /// (`type`, `status`, `reason`, `message`, `lastTransitionTime`).
    /// Serialized as a JSON string in the schema because
    /// `k8s_openapi::Condition` doesn't implement `JsonSchema` in the
    /// schemars 0.8.x line we share with kube 1.x.
    #[serde(default)]
    pub conditions: Vec<Condition>,

    /// When the controller last reconciled this CR. ISO-8601 string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reconciled_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    #[serde(rename = "type")]
    pub r#type: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_transition_time: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum Phase {
    #[default]
    Pending,
    Reconciling,
    Ready,
    Failed,
}

fn default_image() -> String {
    "rspark:latest".to_string()
}
fn default_pull_policy() -> String {
    "Never".to_string()
}
fn default_master_replicas() -> i32 {
    1
}
fn default_worker_replicas() -> i32 {
    2
}
fn default_cores() -> i32 {
    2
}
fn default_memory_mb() -> i32 {
    1024
}
fn default_master_cpu() -> String {
    "200m".to_string()
}
fn default_master_memory() -> String {
    "256Mi".to_string()
}
fn default_worker_cpu() -> String {
    "200m".to_string()
}
fn default_worker_memory() -> String {
    "256Mi".to_string()
}
