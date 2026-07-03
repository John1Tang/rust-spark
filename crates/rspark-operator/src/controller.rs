//! SparkCluster reconciler.
//!
//! The reconciler is intentionally narrow: it owns a fixed set of child
//! objects and reconciles them in this order:
//!
//!   1. ServiceAccount for the master + workers (shared).
//!   2. Master Service (ClusterIP, port 7077 + 8080).
//!   3. Master ConfigMap (so workers can discover the catalog).
//!   4. Master Deployment.
//!   5. Worker Deployment.
//!   6. PodDisruptionBudgets (minAvailable on master, maxUnavailable on workers).
//!   7. Status update — endpoint, ready counts, conditions, last-reconciled.
//!
//! Every owned object carries an `ownerReference` back to the
//! `SparkCluster`, so deleting the CR garbage-collects the children.

use std::collections::BTreeMap;
use std::sync::Arc;

use futures::StreamExt;
use k8s_openapi::api::apps::v1::{
    Deployment, DeploymentSpec, DeploymentStrategy, RollingUpdateDeployment,
};
use k8s_openapi::api::core::v1::{
    ConfigMap, ConfigMapVolumeSource, Container, ContainerPort, EnvVar, EnvVarSource,
    HTTPGetAction, ObjectFieldSelector, Pod, PodSpec, PodTemplateSpec, Probe, ResourceRequirements,
    Service, ServiceAccount, ServicePort, ServiceSpec, Volume, VolumeMount,
};
use k8s_openapi::api::policy::v1::{PodDisruptionBudget, PodDisruptionBudgetSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{
    LabelSelector, LabelSelectorRequirement, ObjectMeta, OwnerReference,
};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use k8s_openapi::chrono;
use kube::api::{Api, ListParams, Patch, PatchParams, PostParams};
use kube::core::Resource;
use kube::runtime::controller::Action;
use kube::runtime::{controller::Controller, watcher};
use kube::{Client, ResourceExt};
use thiserror::Error;
use tracing::{info, warn};

use crate::crd::{Phase, SparkCluster, SparkClusterStatus};

const APP_LABEL: &str = "app.kubernetes.io/name";
const RSPARK_APP: &str = "rspark";
const MANAGED_BY_LABEL: &str = "app.kubernetes.io/managed-by";
const MANAGED_BY_VALUE: &str = "rspark-operator";
const RSPARK_CLUSTER_LABEL: &str = "spark.rspark.io/cluster";
const RSPARK_ROLE_LABEL: &str = "spark.rspark.io/role";
const RSPARK_ROLE_MASTER: &str = "master";
const RSPARK_ROLE_WORKER: &str = "worker";

#[derive(Debug, Error)]
pub enum Error {
    #[error("kube error: {0}")]
    Kube(#[from] kube::Error),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub async fn run(client: Client) -> Result<()> {
    let crds: Api<SparkCluster> = Api::all(client.clone());
    let ctx = Context { client };

    Controller::new(crds, watcher::Config::default().any_semantic())
        .owns(
            Api::<Deployment>::all(ctx.client.clone()),
            watcher::Config::default().any_semantic(),
        )
        .shutdown_on_signal()
        .run(reconcile, error_policy, Arc::new(ctx))
        .for_each(|res| async move {
            if let Err(e) = res {
                tracing::warn!(error = ?e, "controller stream error");
            }
        })
        .await;

    Ok(())
}

#[derive(Clone)]
pub struct Context {
    pub client: Client,
}

async fn reconcile(cr: Arc<SparkCluster>, ctx: Arc<Context>) -> Result<Action> {
    let ns = cr.namespace().unwrap_or_else(|| "default".to_string());
    let name = cr.name_any();

    info!(cluster = %name, namespace = %ns, "reconciling SparkCluster");

    if cr.spec.image.is_empty() {
        return Err(Error::Invalid("spec.image is empty".into()));
    }

    reconcile_children(&cr, &ctx, &ns).await?;
    let status = compute_status(&cr, &ctx, &ns).await?;
    apply_status(&cr, &ctx, status).await?;

    // Requeue every 30s so status reflects rolling updates without
    // waiting for an external change.
    Ok(Action::requeue(std::time::Duration::from_secs(30)))
}

fn error_policy(_cr: Arc<SparkCluster>, err: &Error, _ctx: Arc<Context>) -> Action {
    warn!(error = %err, "reconcile failed; requeueing");
    Action::requeue(std::time::Duration::from_secs(15))
}

fn owner_ref(cr: &SparkCluster) -> OwnerReference {
    let meta = cr.meta();
    OwnerReference {
        api_version: <SparkCluster as Resource>::api_version(&()).into_owned(),
        kind: <SparkCluster as Resource>::kind(&()).into_owned(),
        name: meta.name.clone().unwrap_or_default(),
        uid: meta.uid.clone().unwrap_or_default(),
        controller: Some(true),
        block_owner_deletion: Some(true),
    }
}

fn labels(cr: &SparkCluster, role: &str) -> BTreeMap<String, String> {
    let mut l = BTreeMap::new();
    l.insert(APP_LABEL.to_string(), RSPARK_APP.to_string());
    l.insert(MANAGED_BY_LABEL.to_string(), MANAGED_BY_VALUE.to_string());
    l.insert(RSPARK_CLUSTER_LABEL.to_string(), cr.name_any());
    l.insert(RSPARK_ROLE_LABEL.to_string(), role.to_string());
    l
}

async fn reconcile_children(cr: &SparkCluster, ctx: &Context, ns: &str) -> Result<()> {
    ensure_service_account(cr, ctx, ns).await?;
    ensure_master_service(cr, ctx, ns).await?;
    ensure_master_configmap(cr, ctx, ns).await?;
    ensure_master_deployment(cr, ctx, ns).await?;
    ensure_worker_deployment(cr, ctx, ns).await?;
    ensure_pdbs(cr, ctx, ns).await?;
    Ok(())
}

fn sa_name(cr: &SparkCluster) -> String {
    format!("{}-{}", cr.name_any(), RSPARK_APP)
}

async fn ensure_service_account(cr: &SparkCluster, ctx: &Context, ns: &str) -> Result<()> {
    let api: Api<ServiceAccount> = Api::namespaced(ctx.client.clone(), ns);
    let name = sa_name(cr);
    let sa = ServiceAccount {
        metadata: ObjectMeta {
            name: Some(name.clone()),
            namespace: Some(ns.to_string()),
            labels: Some(labels(cr, "shared")),
            owner_references: Some(vec![owner_ref(cr)]),
            ..Default::default()
        },
        automount_service_account_token: Some(false),
        ..Default::default()
    };
    apply_owned(api, &name, sa).await
}

fn master_service_name(cr: &SparkCluster) -> String {
    format!("{}-master", cr.name_any())
}

async fn ensure_master_service(cr: &SparkCluster, ctx: &Context, ns: &str) -> Result<()> {
    let api: Api<Service> = Api::namespaced(ctx.client.clone(), ns);
    let svc = Service {
        metadata: ObjectMeta {
            name: Some(master_service_name(cr)),
            namespace: Some(ns.to_string()),
            labels: Some(labels(cr, RSPARK_ROLE_MASTER)),
            owner_references: Some(vec![owner_ref(cr)]),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            type_: Some("ClusterIP".to_string()),
            selector: Some(labels(cr, RSPARK_ROLE_MASTER)),
            ports: Some(vec![
                ServicePort {
                    name: Some("api".into()),
                    port: 7077,
                    target_port: Some(IntOrString::Int(7077)),
                    ..Default::default()
                },
                ServicePort {
                    name: Some("dashboard".into()),
                    port: 8080,
                    target_port: Some(IntOrString::Int(8080)),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        }),
        status: None,
    };
    apply_owned(api, &master_service_name(cr), svc).await
}

fn master_config_name(cr: &SparkCluster) -> String {
    format!("{}-master-config", cr.name_any())
}

async fn ensure_master_configmap(cr: &SparkCluster, ctx: &Context, ns: &str) -> Result<()> {
    let api: Api<ConfigMap> = Api::namespaced(ctx.client.clone(), ns);
    let data = serde_json::json!({
        "examples": cr.spec.master.examples.to_string(),
        "load": cr.spec.master.load.join(","),
    })
    .to_string();
    let mut m = BTreeMap::new();
    m.insert("rspark.json".to_string(), data);
    let cm = ConfigMap {
        metadata: ObjectMeta {
            name: Some(master_config_name(cr)),
            namespace: Some(ns.to_string()),
            labels: Some(labels(cr, RSPARK_ROLE_MASTER)),
            owner_references: Some(vec![owner_ref(cr)]),
            ..Default::default()
        },
        data: Some(m),
        binary_data: None,
        immutable: None,
    };
    apply_owned(api, &master_config_name(cr), cm).await
}

fn master_deployment_name(cr: &SparkCluster) -> String {
    master_service_name(cr)
}

async fn ensure_master_deployment(cr: &SparkCluster, ctx: &Context, ns: &str) -> Result<()> {
    let api: Api<Deployment> = Api::namespaced(ctx.client.clone(), ns);
    let name = master_deployment_name(cr);
    let args = build_master_args(cr);
    let container = Container {
        name: "rspark".to_string(),
        image: Some(cr.spec.image.clone()),
        image_pull_policy: Some(cr.spec.image_pull_policy.clone()),
        command: Some(vec!["/usr/local/bin/rspark-cli".into()]),
        args: Some(args),
        env: Some(vec![EnvVar {
            name: "POD_NAME".into(),
            value_from: Some(EnvVarSource {
                field_ref: Some(ObjectFieldSelector {
                    field_path: "metadata.name".into(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }]),
        ports: Some(vec![
            ContainerPort {
                name: Some("api".into()),
                container_port: 7077,
                protocol: Some("TCP".into()),
                ..Default::default()
            },
            ContainerPort {
                name: Some("dashboard".into()),
                container_port: 8080,
                protocol: Some("TCP".into()),
                ..Default::default()
            },
        ]),
        startup_probe: Some(probe("/health", 7077, 2, 60)),
        readiness_probe: Some(probe("/health", 7077, 5, 3)),
        liveness_probe: Some(probe("/health", 7077, 20, 3)),
        resources: Some(resources(&cr.spec.master.cpu, &cr.spec.master.memory)),
        volume_mounts: Some(vec![VolumeMount {
            name: master_config_name(cr),
            mount_path: "/etc/rspark".to_string(),
            read_only: Some(true),
            ..Default::default()
        }]),
        ..Default::default()
    };
    let pod = PodSpec {
        service_account_name: Some(sa_name(cr)),
        containers: vec![container],
        volumes: Some(vec![Volume {
            name: master_config_name(cr),
            config_map: Some(ConfigMapVolumeSource {
                name: master_config_name(cr),
                ..Default::default()
            }),
            ..Default::default()
        }]),
        termination_grace_period_seconds: Some(30),
        ..Default::default()
    };
    let dep = Deployment {
        metadata: ObjectMeta {
            name: Some(name.clone()),
            namespace: Some(ns.to_string()),
            labels: Some(labels(cr, RSPARK_ROLE_MASTER)),
            owner_references: Some(vec![owner_ref(cr)]),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(cr.spec.master.replicas),
            revision_history_limit: Some(5),
            progress_deadline_seconds: Some(120),
            strategy: Some(DeploymentStrategy {
                type_: Some("RollingUpdate".to_string()),
                rolling_update: Some(RollingUpdateDeployment {
                    max_surge: Some(IntOrString::Int(1)),
                    max_unavailable: Some(IntOrString::Int(0)),
                }),
            }),
            selector: LabelSelector {
                match_labels: Some(labels(cr, RSPARK_ROLE_MASTER)),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels(cr, RSPARK_ROLE_MASTER)),
                    ..Default::default()
                }),
                spec: Some(pod),
            },
            ..Default::default()
        }),
        status: None,
    };
    apply_owned(api, &name, dep).await
}

fn build_master_args(cr: &SparkCluster) -> Vec<String> {
    let mut args = vec![
        "master".to_string(),
        "--api-addr".to_string(),
        "0.0.0.0:7077".to_string(),
        "--dashboard-addr".to_string(),
        "0.0.0.0:8080".to_string(),
        "--master-id".to_string(),
        "master-$(POD_NAME)".to_string(),
    ];
    if cr.spec.master.examples {
        args.push("--examples".to_string());
    }
    for spec in &cr.spec.master.load {
        args.push("--load".to_string());
        args.push(spec.clone());
    }
    args
}

fn worker_deployment_name(cr: &SparkCluster) -> String {
    format!("{}-worker", cr.name_any())
}

async fn ensure_worker_deployment(cr: &SparkCluster, ctx: &Context, ns: &str) -> Result<()> {
    let api: Api<Deployment> = Api::namespaced(ctx.client.clone(), ns);
    let name = worker_deployment_name(cr);
    let container = Container {
        name: "rspark".to_string(),
        image: Some(cr.spec.image.clone()),
        image_pull_policy: Some(cr.spec.image_pull_policy.clone()),
        command: Some(vec!["/usr/local/bin/rspark-cli".to_string()]),
        args: Some(vec![
            "worker".to_string(),
            "--master".to_string(),
            format!("http://{}:7077", master_service_name(cr)),
            "--bind".to_string(),
            "0.0.0.0:9090".to_string(),
            "--cores".to_string(),
            cr.spec.workers.cores.to_string(),
            "--memory-mb".to_string(),
            cr.spec.workers.memory_mb.to_string(),
        ]),
        env: Some(vec![EnvVar {
            name: "RSPARK_LOG".to_string(),
            value: Some("info".to_string()),
            ..Default::default()
        }]),
        resources: Some(resources(&cr.spec.workers.cpu, &cr.spec.workers.memory)),
        ..Default::default()
    };
    let pod = PodSpec {
        service_account_name: Some(sa_name(cr)),
        containers: vec![container],
        termination_grace_period_seconds: Some(20),
        ..Default::default()
    };
    let dep = Deployment {
        metadata: ObjectMeta {
            name: Some(name.clone()),
            namespace: Some(ns.to_string()),
            labels: Some(labels(cr, RSPARK_ROLE_WORKER)),
            owner_references: Some(vec![owner_ref(cr)]),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(cr.spec.workers.replicas),
            revision_history_limit: Some(5),
            progress_deadline_seconds: Some(120),
            strategy: Some(DeploymentStrategy {
                type_: Some("RollingUpdate".to_string()),
                rolling_update: Some(RollingUpdateDeployment {
                    max_surge: Some(IntOrString::Int(1)),
                    max_unavailable: Some(IntOrString::Int(1)),
                }),
            }),
            selector: LabelSelector {
                match_labels: Some(labels(cr, RSPARK_ROLE_WORKER)),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels(cr, RSPARK_ROLE_WORKER)),
                    ..Default::default()
                }),
                spec: Some(pod),
            },
            ..Default::default()
        }),
        status: None,
    };
    apply_owned(api, &name, dep).await
}

async fn ensure_pdbs(cr: &SparkCluster, ctx: &Context, ns: &str) -> Result<()> {
    let api: Api<PodDisruptionBudget> = Api::namespaced(ctx.client.clone(), ns);

    let master_name = format!("{}-master-pdb", cr.name_any());
    let master_pdb = PodDisruptionBudget {
        metadata: ObjectMeta {
            name: Some(master_name.clone()),
            namespace: Some(ns.to_string()),
            labels: Some(labels(cr, RSPARK_ROLE_MASTER)),
            owner_references: Some(vec![owner_ref(cr)]),
            ..Default::default()
        },
        spec: Some(PodDisruptionBudgetSpec {
            min_available: Some(IntOrString::Int(1)),
            selector: Some(LabelSelector {
                match_expressions: Some(vec![LabelSelectorRequirement {
                    key: RSPARK_ROLE_LABEL.to_string(),
                    operator: "In".to_string(),
                    values: Some(vec![RSPARK_ROLE_MASTER.to_string()]),
                }]),
                match_labels: Some(labels(cr, RSPARK_ROLE_MASTER)),
            }),
            ..Default::default()
        }),
        status: None,
    };
    apply_owned(api.clone(), &master_name, master_pdb).await?;

    let worker_name = format!("{}-worker-pdb", cr.name_any());
    let worker_pdb = PodDisruptionBudget {
        metadata: ObjectMeta {
            name: Some(worker_name.clone()),
            namespace: Some(ns.to_string()),
            labels: Some(labels(cr, RSPARK_ROLE_WORKER)),
            owner_references: Some(vec![owner_ref(cr)]),
            ..Default::default()
        },
        spec: Some(PodDisruptionBudgetSpec {
            max_unavailable: Some(IntOrString::Int(1)),
            selector: Some(LabelSelector {
                match_expressions: Some(vec![LabelSelectorRequirement {
                    key: RSPARK_ROLE_LABEL.to_string(),
                    operator: "In".to_string(),
                    values: Some(vec![RSPARK_ROLE_WORKER.to_string()]),
                }]),
                match_labels: Some(labels(cr, RSPARK_ROLE_WORKER)),
            }),
            ..Default::default()
        }),
        status: None,
    };
    apply_owned(api, &worker_name, worker_pdb).await
}

/// Server-side patch: try to update, and if the resource is missing,
/// create it. Both paths preserve the owner reference set by `build_*`.
async fn apply_owned<K>(api: Api<K>, name: &str, obj: K) -> Result<()>
where
    K: Clone
        + std::fmt::Debug
        + serde::de::DeserializeOwned
        + serde::Serialize
        + kube::Resource
        + 'static,
    K::DynamicType: Default,
{
    let pp = PatchParams::apply("rspark-operator");
    let patch = Patch::Apply(&obj);
    match api.patch(name, &pp, &patch).await {
        Ok(_) => Ok(()),
        Err(kube::Error::Api(e)) if e.code == 404 => api
            .create(&PostParams::default(), &obj)
            .await
            .map(|_| ())
            .map_err(Error::from),
        Err(e) => Err(e.into()),
    }
}

fn probe(path: &str, port: i32, period_seconds: i32, failure_threshold: i32) -> Probe {
    Probe {
        http_get: Some(HTTPGetAction {
            path: Some(path.to_string()),
            port: IntOrString::Int(port),
            scheme: Some("HTTP".to_string()),
            ..Default::default()
        }),
        period_seconds: Some(period_seconds),
        failure_threshold: Some(failure_threshold),
        ..Default::default()
    }
}

fn resources(cpu: &str, memory: &str) -> ResourceRequirements {
    let mut limits = BTreeMap::new();
    limits.insert(
        "cpu".to_string(),
        k8s_openapi::apimachinery::pkg::api::resource::Quantity(cpu.to_string()),
    );
    limits.insert(
        "memory".to_string(),
        k8s_openapi::apimachinery::pkg::api::resource::Quantity(memory.to_string()),
    );
    ResourceRequirements {
        requests: Some(limits.clone()),
        limits: Some(limits),
        ..Default::default()
    }
}

async fn compute_status(cr: &SparkCluster, ctx: &Context, ns: &str) -> Result<SparkClusterStatus> {
    let deployments: Api<Deployment> = Api::namespaced(ctx.client.clone(), ns);
    let master_name = master_deployment_name(cr);
    let worker_name = worker_deployment_name(cr);

    let master_replicas = match deployments.get_opt(&master_name).await? {
        Some(d) => d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0),
        None => 0,
    };
    let worker_replicas = match deployments.get_opt(&worker_name).await? {
        Some(d) => d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0),
        None => 0,
    };

    let pods: Api<Pod> = Api::namespaced(ctx.client.clone(), ns);
    let lp = ListParams::default().labels(&format!("{}={}", RSPARK_CLUSTER_LABEL, cr.name_any()));
    let mut ready_masters = 0i32;
    let mut ready_workers = 0i32;
    for pod in pods.list(&lp).await? {
        let role = pod
            .metadata
            .labels
            .as_ref()
            .and_then(|m| m.get(RSPARK_ROLE_LABEL))
            .cloned()
            .unwrap_or_default();
        let ready = is_pod_ready(&pod);
        match (role.as_str(), ready) {
            (RSPARK_ROLE_MASTER, true) => ready_masters += 1,
            (RSPARK_ROLE_WORKER, true) => ready_workers += 1,
            _ => {}
        }
    }

    let phase = if master_replicas == 0 || worker_replicas == 0 {
        Phase::Reconciling
    } else if ready_masters >= master_replicas && ready_workers >= worker_replicas {
        Phase::Ready
    } else {
        Phase::Reconciling
    };

    Ok(SparkClusterStatus {
        phase,
        master_endpoint: Some(format!(
            "{}.{}.svc.cluster.local:7077",
            master_service_name(cr),
            ns
        )),
        ready_workers,
        ready_masters,
        conditions: vec![],
        last_reconciled_at: Some(chrono::Utc::now().to_rfc3339()),
    })
}

fn is_pod_ready(pod: &Pod) -> bool {
    pod.status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .map(|conds| {
            conds
                .iter()
                .any(|c| c.type_ == "Ready" && c.status == "True")
        })
        .unwrap_or(false)
}

async fn apply_status(cr: &SparkCluster, ctx: &Context, status: SparkClusterStatus) -> Result<()> {
    let ns = cr.namespace().unwrap_or_else(|| "default".to_string());
    let api: Api<SparkCluster> = Api::namespaced(ctx.client.clone(), &ns);
    let name = cr.name_any();
    let patch = serde_json::json!({
        "apiVersion": <SparkCluster as Resource>::api_version(&()).into_owned(),
        "kind": <SparkCluster as Resource>::kind(&()).into_owned(),
        "status": status,
    });
    api.patch_status(&name, &PatchParams::default(), &Patch::Merge(patch))
        .await?;
    Ok(())
}
