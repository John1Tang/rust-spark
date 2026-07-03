use rspark_operator::crd::{Phase, SparkCluster, SparkClusterSpec};

#[test]
fn spark_cluster_spec_serializes_to_expected_yaml() {
    let spec = SparkClusterSpec {
        image: "rspark:latest".into(),
        image_pull_policy: "Never".into(),
        master: Default::default(),
        workers: Default::default(),
    };
    let cr = SparkCluster::new("demo", spec);
    let yaml = serde_yaml::to_string(&cr).expect("yaml");
    // Quick shape checks — the YAML we expect when a user runs
    // `kubectl apply -f sparkcluster.yaml`.
    assert!(yaml.contains("apiVersion: spark.rspark.io/v1alpha1"));
    assert!(yaml.contains("kind: SparkCluster"));
    assert!(yaml.contains("name: demo"));
}

#[test]
fn phase_default_is_pending() {
    assert_eq!(Phase::default(), Phase::Pending);
}
