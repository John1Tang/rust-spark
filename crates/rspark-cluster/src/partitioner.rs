use rspark_core::error::Result;
use rspark_sql::plan::LogicalPlan;

#[derive(Debug, Clone)]
pub struct PartitionSpec {
    pub index: usize,
    pub label: String,
}

/// Convert a [`LogicalPlan`] tree into a single linear pipeline of stages
/// where each stage corresponds to a transform over partitions of the same
/// scan(s). Today we always produce one stage; finer stage splitting is a
/// future enhancement.
pub fn plan_partitions(plan: &LogicalPlan, parallelism: usize) -> Result<Vec<PartitionSpec>> {
    let mut out = Vec::new();
    let mut scan_idx = 0usize;
    collect_scans(plan, &mut scan_idx, &mut out, parallelism);
    if out.is_empty() {
        out.push(PartitionSpec {
            index: 0,
            label: "no-input".into(),
        });
    }
    Ok(out)
}

fn collect_scans(
    plan: &LogicalPlan,
    scan_idx: &mut usize,
    out: &mut Vec<PartitionSpec>,
    parallelism: usize,
) {
    match plan {
        LogicalPlan::Scan { path, .. } => {
            let index = *scan_idx;
            *scan_idx += 1;
            for partition in 0..parallelism {
                out.push(PartitionSpec {
                    index: out.len(),
                    label: format!("scan[{path}]:p{partition}"),
                });
            }
            let _ = index;
        }
        LogicalPlan::Project { input, .. }
        | LogicalPlan::Filter { input, .. }
        | LogicalPlan::Aggregate { input, .. }
        | LogicalPlan::Sort { input, .. }
        | LogicalPlan::Limit { input, .. }
        | LogicalPlan::Distinct { input, .. } => {
            collect_scans(input, scan_idx, out, parallelism);
        }
        LogicalPlan::Join { left, right, .. } => {
            collect_scans(left, scan_idx, out, parallelism);
            collect_scans(right, scan_idx, out, parallelism);
        }
        LogicalPlan::Union { inputs, .. } => {
            for input in inputs {
                collect_scans(input, scan_idx, out, parallelism);
            }
        }
        LogicalPlan::Empty => {}
    }
}
