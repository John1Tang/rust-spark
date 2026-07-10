use rspark_core::error::{Error, Result};
use rspark_core::expr::{AggregateFn, Expr};
use rspark_core::schema::Schema;
use rspark_core::value::Value;
use rspark_core::{Record, RecordBatch};
use rspark_sql::plan::{JoinType, LogicalPlan, SortExpr};
use std::collections::HashSet;

/// Physical operators that produce or transform [`RecordBatch`]es.
#[derive(Debug, Clone)]
pub enum PhysicalOp {
    Scan(ScanOp),
    Project(ProjectOp),
    Filter(FilterOp),
    Aggregate(AggregateOp),
    Sort(SortOp),
    Limit(LimitOp),
    Join(JoinOp),
}

impl PhysicalOp {
    pub fn output_schema(&self) -> &Schema {
        match self {
            PhysicalOp::Scan(o) => &o.schema,
            PhysicalOp::Project(o) => &o.schema,
            PhysicalOp::Filter(o) => &o.schema,
            PhysicalOp::Aggregate(o) => &o.schema,
            PhysicalOp::Sort(o) => &o.schema,
            PhysicalOp::Limit(o) => &o.schema,
            PhysicalOp::Join(o) => &o.schema,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScanOp {
    pub path: String,
    pub source: String,
    pub schema: Schema,
}

#[derive(Debug, Clone)]
pub struct ProjectOp {
    pub expressions: Vec<Expr>,
    pub schema: Schema,
}

#[derive(Debug, Clone)]
pub struct FilterOp {
    pub predicate: Expr,
    pub schema: Schema,
}

#[derive(Debug, Clone)]
pub struct AggregateOp {
    pub group_exprs: Vec<Expr>,
    pub aggregate_exprs: Vec<Expr>,
    pub schema: Schema,
}

#[derive(Debug, Clone)]
pub struct SortOp {
    pub order: Vec<SortExpr>,
    pub schema: Schema,
}

#[derive(Debug, Clone)]
pub struct LimitOp {
    pub count: usize,
    pub schema: Schema,
}

#[derive(Debug, Clone)]
pub struct JoinOp {
    pub on: Vec<(String, String)>,
    pub how: JoinType,
    pub schema: Schema,
}

/// Lower a [`LogicalPlan`] tree into the physical algebra.
pub fn lower_plan(plan: &LogicalPlan) -> PhysicalOp {
    match plan {
        LogicalPlan::Scan {
            path,
            source,
            schema,
            ..
        } => PhysicalOp::Scan(ScanOp {
            path: path.clone(),
            source: source.clone(),
            schema: schema.clone(),
        }),
        LogicalPlan::Project {
            expressions,
            schema,
            ..
        } => PhysicalOp::Project(ProjectOp {
            expressions: expressions.clone(),
            schema: schema.clone(),
        }),
        LogicalPlan::Filter {
            predicate, schema, ..
        } => PhysicalOp::Filter(FilterOp {
            predicate: predicate.clone(),
            schema: schema.clone(),
        }),
        LogicalPlan::Aggregate {
            group_exprs,
            aggregate_exprs,
            schema,
            ..
        } => PhysicalOp::Aggregate(AggregateOp {
            group_exprs: group_exprs.clone(),
            aggregate_exprs: aggregate_exprs.clone(),
            schema: schema.clone(),
        }),
        LogicalPlan::Sort { order, schema, .. } => PhysicalOp::Sort(SortOp {
            order: order.clone(),
            schema: schema.clone(),
        }),
        LogicalPlan::Limit { count, schema, .. } => PhysicalOp::Limit(LimitOp {
            count: *count,
            schema: schema.clone(),
        }),
        LogicalPlan::Join {
            on, how, schema, ..
        } => PhysicalOp::Join(JoinOp {
            on: on.clone(),
            how: *how,
            schema: schema.clone(),
        }),
        LogicalPlan::Union { schema, .. } | LogicalPlan::Distinct { schema, .. } => {
            // `apply_tree` matches on `&LogicalPlan`, not `&PhysicalOp`,
            // so the op returned here is never read for these variants.
            // We still have to return *something* to keep the enum
            // exhaustive; a zero-row Limit mirrors what the executor
            // would produce if `apply_tree` happened to no-op.
            PhysicalOp::Limit(LimitOp {
                count: 0,
                schema: schema.clone(),
            })
        }
        LogicalPlan::Empty => PhysicalOp::Limit(LimitOp {
            count: 0,
            schema: Schema::empty(),
        }),
    }
}

/// Evaluate a predicate against a record, returning the boolean result.
pub fn eval_predicate(predicate: &Expr, record: &Record, batch: &RecordBatch) -> Result<bool> {
    if contains_aggregate(predicate) {
        let v = substitute_aggregates(predicate, record, batch)?;
        match v {
            Value::Null => Ok(false),
            Value::Boolean(b) => Ok(b),
            other => Err(Error::Type(format!(
                "predicate must return boolean, got {}",
                other.data_type_name()
            ))),
        }
    } else {
        let v = predicate.eval(record, batch)?;
        match v {
            Value::Null => Ok(false),
            Value::Boolean(b) => Ok(b),
            other => Err(Error::Type(format!(
                "predicate must return boolean, got {}",
                other.data_type_name()
            ))),
        }
    }
}

fn contains_aggregate(expr: &Expr) -> bool {
    match expr {
        Expr::Aggregate { .. } => true,
        Expr::Aliased { expr, .. } => contains_aggregate(expr),
        Expr::Binary { left, right, .. } => contains_aggregate(left) || contains_aggregate(right),
        Expr::Not(inner) | Expr::IsNull(inner) | Expr::IsNotNull(inner) => {
            contains_aggregate(inner)
        }
        _ => false,
    }
}

/// If `expr` is or contains an [`Expr::Aggregate`] (possibly wrapped in
/// [`Expr::Aliased`]) that has already been materialised into the input
/// batch, look it up by display name and substitute the precomputed value
/// before evaluation.
fn substitute_aggregates(expr: &Expr, record: &Record, batch: &RecordBatch) -> Result<Value> {
    match expr {
        Expr::Aggregate { .. } => {
            let name = expr.display_name();
            let idx = batch.schema().index_of(&name).ok_or_else(|| {
                Error::Schema(format!("aggregate '{name}' not found in input schema"))
            })?;
            Ok(record.get(idx).cloned().unwrap_or(Value::Null))
        }
        Expr::Aliased { expr: inner, .. } => match inner.as_ref() {
            Expr::Aggregate { .. } => {
                let name = expr.display_name();
                let idx = batch.schema().index_of(&name).ok_or_else(|| {
                    Error::Schema(format!("aggregate '{name}' not found in input schema"))
                })?;
                Ok(record.get(idx).cloned().unwrap_or(Value::Null))
            }
            _ => inner.eval(record, batch),
        },
        Expr::Binary { left, op, right } => {
            let lv = substitute_aggregates(left, record, batch)?;
            let rv = substitute_aggregates(right, record, batch)?;
            if lv.is_null() || rv.is_null() {
                return Ok(Value::Null);
            }
            apply_binary_op(*op, &lv, &rv)
        }
        other => other.eval(record, batch),
    }
}

fn apply_binary_op(op: rspark_core::expr::BinaryOp, l: &Value, r: &Value) -> Result<Value> {
    use rspark_core::expr::BinaryOp::*;
    use Value::*;
    match op {
        Eq => Ok(Boolean(l == r)),
        NotEq => Ok(Boolean(l != r)),
        Lt => Ok(Boolean(matches!(l.try_cmp(r)?, std::cmp::Ordering::Less))),
        LtEq => Ok(Boolean(matches!(
            l.try_cmp(r)?,
            std::cmp::Ordering::Less | std::cmp::Ordering::Equal
        ))),
        Gt => Ok(Boolean(matches!(
            l.try_cmp(r)?,
            std::cmp::Ordering::Greater
        ))),
        GtEq => Ok(Boolean(matches!(
            l.try_cmp(r)?,
            std::cmp::Ordering::Greater | std::cmp::Ordering::Equal
        ))),
        And => match (l, r) {
            (Boolean(a), Boolean(b)) => Ok(Boolean(*a && *b)),
            _ => Ok(Value::Null),
        },
        Or => match (l, r) {
            (Boolean(a), Boolean(b)) => Ok(Boolean(*a || *b)),
            _ => Ok(Value::Null),
        },
        Like => Err(rspark_core::error::Error::Type(
            "LIKE is not yet supported in HAVING predicates".into(),
        )),
        other => Err(rspark_core::error::Error::Execution(format!(
            "unsupported binary op in predicate: {:?}",
            other
        ))),
    }
}

/// Compute one projected row by evaluating each expression against the input record.
pub fn project_record(
    expressions: &[Expr],
    record: &Record,
    batch: &RecordBatch,
    output_schema: &Schema,
) -> Result<Record> {
    let mut values = Vec::with_capacity(expressions.len());
    for expr in expressions {
        match expr {
            Expr::Star => {
                for value in record.values() {
                    values.push(value.clone());
                }
            }
            Expr::Aggregate { .. } => {
                values.push(lookup_by_name(expr.display_name(), record, batch)?);
            }
            Expr::Aliased { expr: inner, .. } => match inner.as_ref() {
                Expr::Aggregate { .. } => {
                    values.push(lookup_by_name(expr.display_name(), record, batch)?);
                }
                _ => {
                    values.push(inner.eval(record, batch)?);
                }
            },
            _ => {
                values.push(expr.eval(record, batch)?);
            }
        }
    }
    let rec = Record::new(values);
    rec.validate(output_schema)?;
    Ok(rec)
}

fn lookup_by_name(name: String, record: &Record, batch: &RecordBatch) -> Result<Value> {
    let idx = batch
        .schema()
        .index_of(&name)
        .ok_or_else(|| Error::Schema(format!("column '{name}' not found in input schema")))?;
    Ok(record.get(idx).cloned().unwrap_or(Value::Null))
}

/// Evaluate aggregate functions across a batch.
pub fn aggregate_batch(
    group_exprs: &[Expr],
    aggregate_exprs: &[Expr],
    batch: &RecordBatch,
    output_schema: &Schema,
) -> Result<RecordBatch> {
    use std::collections::BTreeMap;

    struct GroupState {
        group_values: Vec<Value>,
        accumulators: Vec<Accumulator>,
    }

    let mut groups: BTreeMap<Vec<String>, GroupState> = BTreeMap::new();

    for record in batch.records() {
        let group_values: Vec<Value> = group_exprs
            .iter()
            .map(|g| g.eval(record, batch))
            .collect::<Result<_>>()?;
        let key: Vec<String> = group_values.iter().map(|v| v.cast_to_string()).collect();
        let entry = groups.entry(key).or_insert_with(|| GroupState {
            group_values: group_values.clone(),
            accumulators: aggregate_exprs
                .iter()
                .map(|e| match e {
                    Expr::Aggregate { func, .. } => Accumulator::new(func.clone()),
                    Expr::Aliased { expr, .. } => match expr.as_ref() {
                        Expr::Aggregate { func, .. } => Accumulator::new(func.clone()),
                        _ => Accumulator::new(AggregateFn::Count),
                    },
                    _ => Accumulator::new(AggregateFn::Count),
                })
                .collect(),
        });
        for (idx, agg_expr) in aggregate_exprs.iter().enumerate() {
            let (inner, distinct) = match agg_expr {
                Expr::Aggregate { arg, distinct, .. } => (arg.as_ref(), *distinct),
                Expr::Aliased { expr, .. } => match expr.as_ref() {
                    Expr::Aggregate { arg, distinct, .. } => (arg.as_ref(), *distinct),
                    _ => continue,
                },
                _ => continue,
            };
            let arg_value = inner.eval(record, batch)?;
            entry.accumulators[idx].update(arg_value, distinct);
        }
    }

    let mut out_records = Vec::with_capacity(groups.len());
    for (_, state) in groups {
        let mut row = Vec::with_capacity(group_exprs.len() + aggregate_exprs.len());
        row.extend(state.group_values.iter().cloned());
        for acc in state.accumulators {
            row.push(acc.finish());
        }
        out_records.push(Record::new(row));
    }
    RecordBatch::from_records(output_schema.clone(), out_records)
}

#[derive(Debug, Clone)]
enum Accumulator {
    Count(i64),
    /// Distinct-count accumulator; not yet constructed from `Accumulator::new`
    /// but kept on the enum so future work on `COUNT(DISTINCT col)` plugs in
    /// without changing call sites that already match on the variant.
    #[allow(dead_code)]
    CountDistinct(HashSet<String>),
    Sum(f64),
    Avg {
        sum: f64,
        count: i64,
    },
    Min(Option<Value>),
    Max(Option<Value>),
}

impl Accumulator {
    fn new(func: AggregateFn) -> Self {
        match func {
            AggregateFn::Count => Accumulator::Count(0),
            AggregateFn::Sum => Accumulator::Sum(0.0),
            AggregateFn::Avg => Accumulator::Avg { sum: 0.0, count: 0 },
            AggregateFn::Min => Accumulator::Min(None),
            AggregateFn::Max => Accumulator::Max(None),
        }
    }

    fn update(&mut self, value: Value, _distinct: bool) {
        match self {
            Accumulator::Count(c) => {
                if !value.is_null() {
                    *c += 1;
                }
            }
            Accumulator::CountDistinct(set) => {
                if !value.is_null() {
                    set.insert(value.cast_to_string());
                }
            }
            Accumulator::Sum(sum) => {
                if let Some(v) = value.cast_to_f64() {
                    *sum += v;
                }
            }
            Accumulator::Avg { sum, count } => {
                if let Some(v) = value.cast_to_f64() {
                    *sum += v;
                    *count += 1;
                }
            }
            Accumulator::Min(slot) => {
                if value.is_null() {
                    return;
                }
                *slot = Some(match slot.take() {
                    None => value,
                    Some(current) => match current.try_cmp(&value) {
                        Ok(std::cmp::Ordering::Greater) => value,
                        _ => current,
                    },
                });
            }
            Accumulator::Max(slot) => {
                if value.is_null() {
                    return;
                }
                *slot = Some(match slot.take() {
                    None => value,
                    Some(current) => match current.try_cmp(&value) {
                        Ok(std::cmp::Ordering::Less) => value,
                        _ => current,
                    },
                });
            }
        }
    }

    fn finish(self) -> Value {
        match self {
            Accumulator::Count(c) => Value::Int64(c),
            Accumulator::CountDistinct(set) => Value::Int64(set.len() as i64),
            Accumulator::Sum(s) => Value::Float64(s),
            Accumulator::Avg { sum, count } => {
                if count == 0 {
                    Value::Null
                } else {
                    Value::Float64(sum / count as f64)
                }
            }
            Accumulator::Min(Some(v)) => v,
            Accumulator::Min(None) => Value::Null,
            Accumulator::Max(Some(v)) => v,
            Accumulator::Max(None) => Value::Null,
        }
    }
}

/// Sort a batch by the given order list.
pub fn sort_batch(batch: &RecordBatch, order: &[SortExpr]) -> Result<RecordBatch> {
    let mut indexed: Vec<(usize, Vec<Value>)> = Vec::with_capacity(batch.len());
    for (i, r) in batch.records().iter().enumerate() {
        let mut keys = Vec::with_capacity(order.len());
        for sort_expr in order {
            let v = sort_expr.expr.eval(r, batch)?;
            keys.push(v);
        }
        indexed.push((i, keys));
    }

    indexed.sort_by(|a, b| {
        for (i, (av, bv)) in a.1.iter().zip(b.1.iter()).enumerate() {
            let cmp = match av.try_cmp(bv) {
                Ok(c) => c,
                Err(_) => std::cmp::Ordering::Equal,
            };
            let cmp = if order[i].ascending {
                cmp
            } else {
                cmp.reverse()
            };
            if cmp != std::cmp::Ordering::Equal {
                return cmp;
            }
        }
        std::cmp::Ordering::Equal
    });

    let mut sorted = Vec::with_capacity(batch.len());
    for (idx, _) in indexed {
        if let Some(rec) = batch.records().get(idx) {
            sorted.push(rec.clone());
        }
    }
    RecordBatch::from_records(batch.schema().clone(), sorted)
}

/// Limit to the first N records of a batch.
pub fn limit_batch(batch: &RecordBatch, count: usize) -> Result<RecordBatch> {
    let new_records: Vec<Record> = batch.records().iter().take(count).cloned().collect();
    RecordBatch::from_records(batch.schema().clone(), new_records)
}

/// Join two batches on the given column name pairs.
///
/// `Inner` and `Left` are implemented; other variants are rejected at
/// runtime so a half-supported plan doesn't silently misjoin.
pub fn join_batches(
    left: &RecordBatch,
    right: &RecordBatch,
    pairs: &[(String, String)],
    how: JoinType,
) -> Result<RecordBatch> {
    if matches!(how, JoinType::Right | JoinType::Full) {
        return Err(Error::Execution(format!("{how:?} join not implemented")));
    }
    let mut fields = left.schema().fields().to_vec();
    for f in right.schema().fields() {
        if !pairs.iter().any(|(ln, _)| ln == &f.name) {
            fields.push(f.clone());
        }
    }
    let schema = Schema::new(fields);
    let mut out_records = Vec::new();
    for l in left.records() {
        let mut matched = false;
        for r in right.records() {
            let mut cond = true;
            for (l_name, r_name) in pairs {
                let l_idx = left.schema().index_of(l_name);
                let r_idx = right.schema().index_of(r_name);
                if let (Some(li), Some(ri)) = (l_idx, r_idx) {
                    let lv = l.get(li).cloned().unwrap_or(Value::Null);
                    let rv = r.get(ri).cloned().unwrap_or(Value::Null);
                    if lv != rv {
                        cond = false;
                        break;
                    }
                }
            }
            if cond {
                matched = true;
                let mut row: Vec<Value> = Vec::with_capacity(schema.field_count());
                row.extend(l.values().iter().cloned());
                for f in right.schema().fields() {
                    if !pairs.iter().any(|(ln, _)| ln == &f.name) {
                        if let Some(idx) = right.schema().index_of(&f.name) {
                            row.push(r.get(idx).cloned().unwrap_or(Value::Null));
                        }
                    }
                }
                out_records.push(Record::new(row));
            }
        }
        if !matched && matches!(how, JoinType::Left) {
            // Left row with no right match: pad right-side fields with NULL.
            let mut row: Vec<Value> = Vec::with_capacity(schema.field_count());
            row.extend(l.values().iter().cloned());
            for f in right.schema().fields() {
                if !pairs.iter().any(|(ln, _)| ln == &f.name) {
                    row.push(Value::Null);
                }
            }
            out_records.push(Record::new(row));
        }
    }
    RecordBatch::from_records(schema, out_records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rspark_core::schema::{DataType, Field, Schema};

    fn people_schema() -> Schema {
        Schema::new(vec![
            Field::new("id", DataType::Int64),
            Field::new("name", DataType::String),
        ])
    }

    fn people_batch() -> RecordBatch {
        RecordBatch::from_records(
            people_schema(),
            vec![
                Record::new(vec![1i64.into(), "alice".into()]),
                Record::new(vec![2i64.into(), "bob".into()]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn filter_records() {
        let batch = people_batch();
        let predicate = Expr::binary(
            rspark_core::expr::BinaryOp::Gt,
            Expr::col("id"),
            Expr::lit(1i64),
        );
        let mut out = Vec::new();
        for record in batch.records() {
            if eval_predicate(&predicate, record, &batch).unwrap() {
                out.push(record.clone());
            }
        }
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn project_records() {
        let batch = people_batch();
        let mut projected = Vec::new();
        for record in batch.records() {
            let new_rec = project_record(
                &[Expr::col("name")],
                record,
                &batch,
                &Schema::new(vec![Field::new("name", DataType::String)]),
            )
            .unwrap();
            projected.push(new_rec);
        }
        assert_eq!(projected.len(), 2);
        assert_eq!(
            projected[0].get(0).cloned().unwrap(),
            Value::String("alice".into())
        );
    }

    #[test]
    fn aggregate_count_group_by() {
        let batch = people_batch();
        let out_schema = Schema::new(vec![Field::new("count", DataType::Int64)]);
        let result = aggregate_batch(
            &[],
            &[Expr::aggregate(AggregateFn::Count, Expr::lit(1i64), false)],
            &batch,
            &out_schema,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result.records()[0].get(0).cloned().unwrap(),
            Value::Int64(2)
        );
    }

    #[test]
    fn min_max_track_extremes() {
        let batch = people_batch();
        let out_schema = Schema::new(vec![
            Field::new("min_id", DataType::Int64),
            Field::new("max_id", DataType::Int64),
        ]);
        let result = aggregate_batch(
            &[],
            &[
                Expr::aggregate(AggregateFn::Min, Expr::col("id"), false),
                Expr::aggregate(AggregateFn::Max, Expr::col("id"), false),
            ],
            &batch,
            &out_schema,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result.records()[0].get(0).cloned().unwrap(),
            Value::Int64(1)
        );
        assert_eq!(
            result.records()[0].get(1).cloned().unwrap(),
            Value::Int64(2)
        );
    }

    #[test]
    fn sort_orders_records() {
        let batch = people_batch();
        let order = vec![SortExpr {
            expr: Expr::col("id"),
            ascending: false,
        }];
        let sorted = sort_batch(&batch, &order).unwrap();
        assert_eq!(
            sorted.records()[0].get(0).cloned().unwrap(),
            Value::Int64(2)
        );
    }

    #[test]
    fn limit_caps_records() {
        let batch = people_batch();
        let limited = limit_batch(&batch, 1).unwrap();
        assert_eq!(limited.len(), 1);
    }

    #[test]
    fn join_matches_on_column() {
        let left = people_batch();
        let mut right = RecordBatch::new(Schema::new(vec![
            Field::new("id", DataType::Int64),
            Field::new("score", DataType::Float64),
        ]));
        right
            .push(Record::new(vec![1i64.into(), 95.0.into()]))
            .unwrap();
        right
            .push(Record::new(vec![2i64.into(), 85.0.into()]))
            .unwrap();
        let joined = join_batches(
            &left,
            &right,
            &[("id".into(), "id".into())],
            JoinType::Inner,
        )
        .unwrap();
        assert_eq!(joined.len(), 2);
        assert_eq!(joined.schema().field_count(), 3);
    }

    #[test]
    fn left_join_pads_unmatched_with_null() {
        // Left side has 2 rows; right has 1 match for id=1 only.
        // Left join should yield 2 rows; the id=2 row gets a NULL score.
        let left = people_batch();
        let mut right = RecordBatch::new(Schema::new(vec![
            Field::new("id", DataType::Int64),
            Field::new("score", DataType::Float64),
        ]));
        right
            .push(Record::new(vec![1i64.into(), 95.0.into()]))
            .unwrap();
        let joined =
            join_batches(&left, &right, &[("id".into(), "id".into())], JoinType::Left).unwrap();
        assert_eq!(joined.len(), 2);
        // Row for id=2 has no score match → score is NULL.
        let score_idx = joined.schema().index_of("score").unwrap();
        let bob_row = joined
            .records()
            .iter()
            .find(|r| matches!(r.get(0), Some(Value::Int64(2))))
            .unwrap();
        assert_eq!(bob_row.get(score_idx), Some(&Value::Null));
    }

    #[test]
    fn unsupported_join_types_error() {
        let left = people_batch();
        let right = people_batch();
        for how in [JoinType::Right, JoinType::Full] {
            assert!(join_batches(&left, &right, &[("id".into(), "id".into())], how).is_err());
        }
    }
}
