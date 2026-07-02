use crate::operators::{
    aggregate_batch, eval_predicate, join_batches, limit_batch, lower_plan, project_record,
    sort_batch, PhysicalOp,
};
use crate::partition::{Partition, PartitionedBatch};
use rspark_core::error::{Error, Result};
use rspark_core::expr::Expr;
use rspark_core::schema::Schema;
use rspark_core::RecordBatch;
use rspark_sql::plan::LogicalPlan;
use rspark_storage::{BoxedDataSource, SourceRegistry};
use std::sync::Arc;
use std::time::Instant;

pub struct ExecutionContext {
    pub source_registry: Arc<SourceRegistry>,
    pub default_parallelism: usize,
}

impl ExecutionContext {
    pub fn new(source_registry: Arc<SourceRegistry>) -> Self {
        Self {
            source_registry,
            default_parallelism: num_cpus(),
        }
    }

    pub fn with_parallelism(mut self, p: usize) -> Self {
        self.default_parallelism = p.max(1);
        self
    }

    /// Execute a [`LogicalPlan`] on the calling thread, returning the
    /// final [`RecordBatch`]. Used by the local mode CLI.
    pub fn execute_plan(
        &self,
        plan: &LogicalPlan,
        mut hooks: Option<&mut dyn ExecutionHooks>,
    ) -> Result<RecordBatch> {
        let started = Instant::now();
        let partitions = self.plan_partitions(plan)?;
        let mut current: Vec<PartitionedBatch> = Vec::new();
        for partition in partitions {
            let batch = self.execute_partition(plan, &partition)?;
            current.push(PartitionedBatch {
                partition,
                batch,
                output_schema: plan.output_schema().clone(),
            });
        }
        if let Some(h) = hooks.as_deref_mut() {
            h.on_stages_complete(&[]);
        }
        let final_batch = combine_batches(current, plan.output_schema())?;
        if let Some(h) = hooks {
            h.on_query_complete(final_batch.len(), started.elapsed());
        }
        Ok(final_batch)
    }

    pub fn plan_partitions(&self, plan: &LogicalPlan) -> Result<Vec<Partition>> {
        find_scan_partitions(plan)
    }

    fn execute_partition(&self, plan: &LogicalPlan, partition: &Partition) -> Result<RecordBatch> {
        let physical = lower_plan(plan);
        match (&physical, partition) {
            (PhysicalOp::Scan(scan), Partition::WholeFile { path }) => {
                self.read_full_file(path, &scan.source, &scan.schema)
            }
            (PhysicalOp::Scan(scan), Partition::CsvSlice { path, .. }) => {
                self.read_full_file(path, &scan.source, &scan.schema)
            }
            (other_op, _) => Err(Error::Execution(format!(
                "cannot execute non-scan partition with op {other_op:?}"
            ))),
        }
    }

    fn read_full_file(&self, path: &str, source: &str, schema: &Schema) -> Result<RecordBatch> {
        let src: BoxedDataSource = self.source_registry.get(source)?;
        src.scan(path, Some(schema))
    }
}

pub trait ExecutionHooks {
    fn on_stages_complete(&mut self, _stages: &[()]) {}
    fn on_query_complete(&mut self, _rows: usize, _duration: std::time::Duration) {}
}

fn find_scan_partitions(plan: &LogicalPlan) -> Result<Vec<Partition>> {
    let mut partitions = Vec::new();
    collect_partitions(plan, &mut partitions);
    if partitions.is_empty() {
        partitions.push(Partition::WholeFile {
            path: String::new(),
        });
    }
    Ok(partitions)
}

fn collect_partitions(plan: &LogicalPlan, out: &mut Vec<Partition>) {
    match plan {
        LogicalPlan::Scan { path, .. } => {
            out.push(Partition::WholeFile { path: path.clone() });
        }
        LogicalPlan::Project { input, .. }
        | LogicalPlan::Filter { input, .. }
        | LogicalPlan::Aggregate { input, .. }
        | LogicalPlan::Sort { input, .. }
        | LogicalPlan::Limit { input, .. }
        | LogicalPlan::Distinct { input, .. } => {
            collect_partitions(input, out);
        }
        LogicalPlan::Join { left, right, .. } => {
            collect_partitions(left, out);
            collect_partitions(right, out);
        }
        LogicalPlan::Union { inputs, .. } => {
            for input in inputs {
                collect_partitions(input, out);
            }
        }
        LogicalPlan::Empty => {}
    }
}

fn combine_batches(batches: Vec<PartitionedBatch>, output_schema: &Schema) -> Result<RecordBatch> {
    let mut combined = RecordBatch::new(output_schema.clone());
    for part in batches {
        for record in part.batch.into_records() {
            combined.push(record)?;
        }
    }
    Ok(combined)
}

/// In-process executor that drives the full logical plan tree, applying
/// each operator in pipeline order. This is what the CLI's `local` mode uses.
pub struct LocalExecutor<'a> {
    pub context: &'a ExecutionContext,
}

impl<'a> LocalExecutor<'a> {
    pub fn new(context: &'a ExecutionContext) -> Self {
        Self { context }
    }

    pub fn execute(&self, plan: &LogicalPlan) -> Result<RecordBatch> {
        let mut current_batch = self.materialize_input(plan)?;
        let mut current_op = lower_plan(plan);
        apply_tree(plan, &mut current_batch, &mut current_op, self.context)?;
        Ok(current_batch)
    }

    fn materialize_input(&self, plan: &LogicalPlan) -> Result<RecordBatch> {
        match plan {
            LogicalPlan::Scan {
                path,
                source,
                schema,
                ..
            } => {
                let src = self.context.source_registry.get(source)?;
                src.scan(path, Some(schema))
            }
            LogicalPlan::Empty => Ok(RecordBatch::new(Schema::empty())),
            _ => {
                if let Some(child) = plan.children().first() {
                    self.execute(child)
                } else {
                    Ok(RecordBatch::new(plan.output_schema().clone()))
                }
            }
        }
    }
}

fn apply_tree(
    plan: &LogicalPlan,
    batch: &mut RecordBatch,
    op: &mut PhysicalOp,
    ctx: &ExecutionContext,
) -> Result<()> {
    match plan {
        LogicalPlan::Scan { schema, .. } => {
            if batch.is_empty() {
                *batch = RecordBatch::new(schema.clone());
            }
        }
        LogicalPlan::Empty => {
            if batch.is_empty() {
                *batch = RecordBatch::new(Schema::empty());
            }
        }
        LogicalPlan::Project {
            expressions,
            schema,
            ..
        } => {
            let has_star = expressions.iter().any(|e| matches!(e, Expr::Star));
            if has_star && expressions.len() == 1 {
                *batch = RecordBatch::from_records(schema.clone(), batch.records().to_vec())?;
            } else {
                let mut new_records = Vec::with_capacity(batch.len());
                for record in batch.records() {
                    new_records.push(project_record(expressions, record, batch, schema)?);
                }
                *batch = RecordBatch::from_records(schema.clone(), new_records)?;
            }
            *op = lower_plan(plan);
        }
        LogicalPlan::Filter {
            predicate, schema, ..
        } => {
            let mut new_records = Vec::with_capacity(batch.len());
            for record in batch.records() {
                if eval_predicate(predicate, record, batch)? {
                    new_records.push(record.clone());
                }
            }
            *batch = RecordBatch::from_records(schema.clone(), new_records)?;
            *op = lower_plan(plan);
        }
        LogicalPlan::Aggregate {
            group_exprs,
            aggregate_exprs,
            schema,
            ..
        } => {
            *batch = aggregate_batch(group_exprs, aggregate_exprs, batch, schema)?;
            *op = lower_plan(plan);
        }
        LogicalPlan::Sort { order, schema, .. } => {
            *batch = sort_batch(batch, order)?;
            let records = std::mem::replace(batch, RecordBatch::new(schema.clone())).into_records();
            *batch = RecordBatch::from_records(schema.clone(), records)?;
            *op = lower_plan(plan);
        }
        LogicalPlan::Limit { count, schema, .. } => {
            let limited = limit_batch(batch, *count)?;
            let records = limited.into_records();
            *batch = RecordBatch::from_records(schema.clone(), records)?;
            *op = lower_plan(plan);
        }
        LogicalPlan::Join {
            left,
            right,
            on,
            how,
            schema,
            ..
        } => {
            let mut left_batch = RecordBatch::new(left.output_schema().clone());
            left_batch.extend(std::mem::replace(batch, RecordBatch::new(Schema::empty())))?;
            let right_batch = {
                let exec = LocalExecutor::new(ctx);
                exec.execute(right)?
            };
            let joined = join_batches(&left_batch, &right_batch, on)?;
            *batch = RecordBatch::from_records(schema.clone(), joined.into_records())?;
            let _ = how;
            *op = lower_plan(plan);
        }
        LogicalPlan::Union { inputs, schema, .. } => {
            let mut combined = RecordBatch::new(schema.clone());
            for input in inputs {
                let exec = LocalExecutor::new(ctx);
                let b = exec.execute(input)?;
                combined.extend(b)?;
            }
            *batch = combined;
            *op = lower_plan(plan);
        }
        LogicalPlan::Distinct { schema, .. } => {
            let mut seen = std::collections::HashSet::new();
            let mut new_records = Vec::with_capacity(batch.len());
            for record in batch.records() {
                let key = record
                    .values()
                    .iter()
                    .map(|v| v.cast_to_string())
                    .collect::<Vec<_>>()
                    .join("|");
                if seen.insert(key) {
                    new_records.push(record.clone());
                }
            }
            *batch = RecordBatch::from_records(schema.clone(), new_records)?;
            *op = lower_plan(plan);
        }
    }
    Ok(())
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}
