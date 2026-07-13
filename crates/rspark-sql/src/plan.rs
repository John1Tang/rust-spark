use rspark_core::error::Result;
use rspark_core::expr::{AggregateFn, BinaryOp, Expr, Literal};
use rspark_core::schema::{DataType, Field, Schema};
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub enum LogicalPlan {
    Scan {
        /// Catalog table name as written in the query (e.g. `users`,
        /// `click_events`). Used for lineage and to resolve the table
        /// when the destination/path drifts (e.g. a live tail that
        /// re-points the catalog at a different file).
        table_name: String,
        path: String,
        source: String,
        projection: Option<Vec<Expr>>,
        filter: Option<Expr>,
        schema: Schema,
    },
    Project {
        input: Box<LogicalPlan>,
        expressions: Vec<Expr>,
        schema: Schema,
    },
    Filter {
        input: Box<LogicalPlan>,
        predicate: Expr,
        schema: Schema,
    },
    Aggregate {
        input: Box<LogicalPlan>,
        group_exprs: Vec<Expr>,
        aggregate_exprs: Vec<Expr>,
        schema: Schema,
    },
    Sort {
        input: Box<LogicalPlan>,
        order: Vec<SortExpr>,
        schema: Schema,
    },
    Limit {
        input: Box<LogicalPlan>,
        count: usize,
        schema: Schema,
    },
    Join {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
        on: Vec<(String, String)>,
        how: JoinType,
        schema: Schema,
    },
    Union {
        inputs: Vec<LogicalPlan>,
        schema: Schema,
    },
    Distinct {
        input: Box<LogicalPlan>,
        schema: Schema,
    },
    Empty,
}

#[derive(Debug, Clone)]
pub struct SortExpr {
    pub expr: Expr,
    pub ascending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinType {
    Inner,
    Left,
    Right,
    Full,
}

impl LogicalPlan {
    pub fn output_schema(&self) -> &Schema {
        match self {
            LogicalPlan::Scan { schema, .. }
            | LogicalPlan::Project { schema, .. }
            | LogicalPlan::Filter { schema, .. }
            | LogicalPlan::Aggregate { schema, .. }
            | LogicalPlan::Sort { schema, .. }
            | LogicalPlan::Limit { schema, .. }
            | LogicalPlan::Join { schema, .. }
            | LogicalPlan::Union { schema, .. }
            | LogicalPlan::Distinct { schema, .. } => schema,
            LogicalPlan::Empty => empty_schema(),
        }
    }

    pub fn output_schema_owned(&self) -> Schema {
        match self {
            LogicalPlan::Empty => Schema::empty(),
            other => other.output_schema().clone(),
        }
    }

    pub fn children(&self) -> Vec<&LogicalPlan> {
        match self {
            LogicalPlan::Project { input, .. }
            | LogicalPlan::Filter { input, .. }
            | LogicalPlan::Aggregate { input, .. }
            | LogicalPlan::Sort { input, .. }
            | LogicalPlan::Limit { input, .. }
            | LogicalPlan::Distinct { input, .. } => vec![input.as_ref()],
            LogicalPlan::Join { left, right, .. } => vec![left.as_ref(), right.as_ref()],
            LogicalPlan::Union { inputs, .. } => inputs.iter().collect(),
            LogicalPlan::Scan { .. } | LogicalPlan::Empty => vec![],
        }
    }

    /// Walk the plan tree and return the catalog tables referenced by
    /// any `Scan`, in first-occurrence order with duplicates removed.
    /// Used by `/v1/sql` to decide whether a query touches a streaming
    /// table (which triggers pipeline auto-spawn) and by the DAG
    /// endpoint to populate each flow's `reads` lineage field.
    pub fn collect_table_refs(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_table_refs_into(&mut out);
        out
    }

    fn collect_table_refs_into(&self, out: &mut Vec<String>) {
        match self {
            LogicalPlan::Scan { table_name, .. } => {
                if !out.iter().any(|t| t == table_name) {
                    out.push(table_name.clone());
                }
            }
            LogicalPlan::Project { input, .. }
            | LogicalPlan::Filter { input, .. }
            | LogicalPlan::Aggregate { input, .. }
            | LogicalPlan::Sort { input, .. }
            | LogicalPlan::Limit { input, .. }
            | LogicalPlan::Distinct { input, .. } => input.collect_table_refs_into(out),
            LogicalPlan::Join { left, right, .. } => {
                left.collect_table_refs_into(out);
                right.collect_table_refs_into(out);
            }
            LogicalPlan::Union { inputs, .. } => {
                for i in inputs {
                    i.collect_table_refs_into(out);
                }
            }
            LogicalPlan::Empty => {}
        }
    }
}

pub fn build_scan_schema(name: &str, path: &str, source: &str, schema: Schema) -> LogicalPlan {
    LogicalPlan::Scan {
        table_name: name.to_string(),
        path: path.to_string(),
        source: source.to_string(),
        projection: None,
        filter: None,
        schema: schema.rename_table(name),
    }
}

pub fn build_project_schema(expressions: &[Expr], input: &Schema) -> Result<Schema> {
    let mut fields = Vec::with_capacity(expressions.len());
    for expr in expressions {
        match expr {
            Expr::Star => {
                for f in input.fields() {
                    fields.push(f.clone());
                }
            }
            _ => {
                let name = expr.display_name();
                let data_type = infer_data_type(expr);
                fields.push(Field::new(name, data_type));
            }
        }
    }
    Ok(Schema::new(fields))
}

pub fn build_aggregate_schema(group_exprs: &[Expr], aggregate_exprs: &[Expr]) -> Result<Schema> {
    let mut fields = Vec::with_capacity(group_exprs.len() + aggregate_exprs.len());
    for g in group_exprs {
        fields.push(Field::new(g.display_name(), infer_data_type(g)));
    }
    for a in aggregate_exprs {
        fields.push(Field::new(a.display_name(), infer_data_type(a)));
    }
    Ok(Schema::new(fields))
}

pub fn build_join_schema(left: &Schema, right: &Schema, on: &[(String, String)]) -> Result<Schema> {
    let mut fields = left.fields().to_vec();
    for f in right.fields() {
        if !on.iter().any(|(l, _)| l == &f.name) {
            fields.push(f.clone());
        }
    }
    Ok(Schema::new(fields))
}

pub fn infer_data_type(expr: &Expr) -> DataType {
    match expr {
        Expr::Literal(lit) => match lit {
            Literal::Null => DataType::Null,
            Literal::Bool(_) => DataType::Boolean,
            Literal::Int(_) => DataType::Int64,
            Literal::Float(_) => DataType::Float64,
            Literal::Str(_) => DataType::String,
        },
        Expr::Aggregate { func, .. } => match func {
            AggregateFn::Count => DataType::Int64,
            AggregateFn::Sum | AggregateFn::Avg => DataType::Float64,
            AggregateFn::Min | AggregateFn::Max => DataType::String,
        },
        Expr::Binary { op, .. } => match op {
            BinaryOp::And
            | BinaryOp::Or
            | BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Lt
            | BinaryOp::LtEq
            | BinaryOp::Gt
            | BinaryOp::GtEq
            | BinaryOp::Like => DataType::Boolean,
            _ => DataType::Float64,
        },
        Expr::Not(_) => DataType::Boolean,
        Expr::IsNull(_) | Expr::IsNotNull(_) => DataType::Boolean,
        Expr::If {
            then_expr,
            else_expr,
            ..
        } => {
            let then_ty = infer_data_type(then_expr);
            let else_ty = infer_data_type(else_expr);
            if then_ty == else_ty {
                then_ty
            } else {
                DataType::String
            }
        }
        Expr::Column(_) | Expr::Star => DataType::String,
        Expr::Aliased { expr, .. } => infer_data_type(expr),
    }
}

pub trait RenameTable {
    fn rename_table(self, table: &str) -> Self;
}

impl RenameTable for Schema {
    fn rename_table(self, _table: &str) -> Self {
        self
    }
}

static EMPTY_SCHEMA: OnceLock<Schema> = OnceLock::new();

pub fn empty_schema() -> &'static Schema {
    EMPTY_SCHEMA.get_or_init(Schema::empty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::JoinType;
    use rspark_core::expr::{BinaryOp, Expr};

    fn scan(name: &str) -> LogicalPlan {
        LogicalPlan::Scan {
            table_name: name.into(),
            path: format!("/tmp/{name}.csv"),
            source: "csv".into(),
            projection: None,
            filter: None,
            schema: Schema::empty(),
        }
    }

    fn project(input: LogicalPlan) -> LogicalPlan {
        LogicalPlan::Project {
            input: Box::new(input),
            expressions: vec![Expr::col("x")],
            schema: Schema::empty(),
        }
    }

    fn filter(input: LogicalPlan) -> LogicalPlan {
        LogicalPlan::Filter {
            input: Box::new(input),
            predicate: Expr::binary(BinaryOp::Gt, Expr::col("x"), Expr::col("y")),
            schema: Schema::empty(),
        }
    }

    #[test]
    fn collect_table_refs_scan_only() {
        assert_eq!(scan("users").collect_table_refs(), vec!["users"]);
    }

    #[test]
    fn collect_table_refs_walks_project_filter() {
        let plan = project(filter(scan("events")));
        assert_eq!(plan.collect_table_refs(), vec!["events"]);
    }

    #[test]
    fn collect_table_refs_join_both_sides() {
        let plan = LogicalPlan::Join {
            left: Box::new(scan("click_events")),
            right: Box::new(scan("users")),
            on: vec![("user_id".into(), "id".into())],
            how: JoinType::Inner,
            schema: Schema::empty(),
        };
        assert_eq!(
            plan.collect_table_refs(),
            vec!["click_events", "users"]
        );
    }

    #[test]
    fn collect_table_refs_dedupes_in_order() {
        // Same table referenced from a sub-plan should appear once.
        let plan = LogicalPlan::Join {
            left: Box::new(scan("users")),
            right: Box::new(project(filter(scan("users")))),
            on: vec![("id".into(), "id".into())],
            how: JoinType::Inner,
            schema: Schema::empty(),
        };
        assert_eq!(plan.collect_table_refs(), vec!["users"]);
    }

    #[test]
    fn collect_table_refs_union() {
        let plan = LogicalPlan::Union {
            inputs: vec![scan("a"), scan("b"), scan("a")],
            schema: Schema::empty(),
        };
        assert_eq!(plan.collect_table_refs(), vec!["a", "b"]);
    }

    #[test]
    fn collect_table_refs_empty() {
        assert!(LogicalPlan::Empty.collect_table_refs().is_empty());
    }
}
