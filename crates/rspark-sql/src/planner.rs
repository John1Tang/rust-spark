use rspark_core::error::{Error, Result};
use rspark_core::expr::{BinaryOp, Expr, Literal};
use rspark_core::schema::{DataType, Field, Schema};
use sqlparser::ast::{Expr as SqlExpr, GroupByExpr, Query, Select, SetExpr, Statement};

use crate::expr_builder::{build_expr, build_from_tables, build_select_expressions, combine_and};
use crate::plan::{
    build_aggregate_schema, build_join_schema, build_project_schema, build_scan_schema, JoinType,
    LogicalPlan, SortExpr,
};

#[derive(Debug, Clone, Default)]
pub struct Planner;

impl Planner {
    pub fn new() -> Self {
        Self
    }

    pub fn plan_sql(&self, sql: &str, catalog: &dyn Catalog) -> Result<LogicalPlan> {
        let statements = crate::parser::parse_sql(sql)?;
        let mut last_plan: Option<LogicalPlan> = None;
        for stmt in statements {
            last_plan = Some(self.plan_statement(&stmt, catalog)?);
        }
        last_plan.ok_or_else(|| Error::Sql("no statements to plan".into()))
    }

    pub fn plan_statement(&self, stmt: &Statement, catalog: &dyn Catalog) -> Result<LogicalPlan> {
        match stmt {
            Statement::Query(query) => self.plan_query(query, catalog),
            Statement::Explain { statement, .. } => {
                let inner = self.plan_statement(statement, catalog)?;
                let formatted = format!("{inner:?}");
                Ok(LogicalPlan::Project {
                    input: Box::new(inner),
                    expressions: vec![Expr::lit(Literal::from(formatted))],
                    schema: Schema::new(vec![Field::new("plan", DataType::String)]),
                })
            }
            Statement::ShowTables { .. } => Ok(LogicalPlan::Empty),
            Statement::Use { .. } => Ok(LogicalPlan::Empty),
            other => Err(Error::Sql(format!("unsupported statement: {}", other))),
        }
    }

    fn plan_query(&self, query: &Query, catalog: &dyn Catalog) -> Result<LogicalPlan> {
        if query.with.is_some() {
            return Err(Error::Sql("CTE (WITH) not yet supported".into()));
        }
        let mut plan = self.plan_setexpr(&query.body, catalog)?;

        let mut sort_keys = Vec::new();
        if let Some(order_by) = &query.order_by {
            for inner in &order_by.exprs {
                sort_keys.push(SortExpr {
                    expr: build_expr(&inner.expr)?,
                    ascending: inner.asc.unwrap_or(true),
                });
            }
        }
        if !sort_keys.is_empty() {
            let schema = plan.output_schema_owned();
            plan = LogicalPlan::Sort {
                input: Box::new(plan),
                order: sort_keys,
                schema,
            };
        }
        if let Some(limit) = query.limit.as_ref() {
            let count = eval_limit(limit)?;
            let schema = plan.output_schema_owned();
            plan = LogicalPlan::Limit {
                input: Box::new(plan),
                count,
                schema,
            };
        }
        Ok(plan)
    }

    fn plan_setexpr(&self, body: &SetExpr, catalog: &dyn Catalog) -> Result<LogicalPlan> {
        match body {
            SetExpr::Select(select) => self.plan_select(select, catalog),
            SetExpr::Query(query) => self.plan_query(query, catalog),
            SetExpr::SetOperation {
                op,
                left,
                right,
                set_quantifier: _,
            } => match op {
                sqlparser::ast::SetOperator::Union => {
                    let l = self.plan_setexpr(left, catalog)?;
                    let r = self.plan_setexpr(right, catalog)?;
                    let schema = l.output_schema_owned();
                    Ok(LogicalPlan::Union {
                        inputs: vec![l, r],
                        schema,
                    })
                }
                other => Err(Error::Sql(format!("unsupported set operation: {other:?}"))),
            },
            _ => Err(Error::Sql(format!("unsupported set expression: {}", body))),
        }
    }

    fn plan_select(&self, select: &Select, catalog: &dyn Catalog) -> Result<LogicalPlan> {
        let tables = build_from_tables(&select.from)?;
        let mut plan = self.build_from_clause(&tables, catalog)?;

        if let Some(where_clause) = &select.selection {
            let predicate = build_expr(where_clause)?;
            let schema = plan.output_schema_owned();
            plan = LogicalPlan::Filter {
                input: Box::new(plan),
                predicate,
                schema,
            };
        }

        let projections = build_select_expressions(select)?;
        let has_distinct = select.distinct.is_some();

        let group_exprs: Vec<Expr> = if let GroupByExpr::Expressions(exprs, _) = &select.group_by {
            exprs.iter().map(build_expr).collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        let aggregate_exprs: Vec<Expr> = projections
            .iter()
            .filter(|e| e.contains_aggregate())
            .cloned()
            .collect();

        if !aggregate_exprs.is_empty() || !group_exprs.is_empty() {
            let agg_schema = build_aggregate_schema(&group_exprs, &aggregate_exprs)?;
            plan = LogicalPlan::Aggregate {
                input: Box::new(plan),
                group_exprs,
                aggregate_exprs: aggregate_exprs.clone(),
                schema: agg_schema,
            };
            let proj_schema = build_project_schema(&projections, plan.output_schema())?;
            plan = LogicalPlan::Project {
                input: Box::new(plan),
                expressions: projections.clone(),
                schema: proj_schema,
            };
            if let Some(having) = &select.having {
                let predicate = rewrite_having(build_expr(having)?, &projections)?;
                let schema = plan.output_schema_owned();
                plan = LogicalPlan::Filter {
                    input: Box::new(plan),
                    predicate,
                    schema,
                };
            }
        } else if has_distinct {
            let proj_schema = build_project_schema(&projections, plan.output_schema())?;
            plan = LogicalPlan::Project {
                input: Box::new(plan),
                expressions: projections,
                schema: proj_schema,
            };
            let schema = plan.output_schema_owned();
            plan = LogicalPlan::Distinct {
                input: Box::new(plan),
                schema,
            };
        } else {
            let proj_schema = build_project_schema(&projections, plan.output_schema())?;
            plan = LogicalPlan::Project {
                input: Box::new(plan),
                expressions: projections,
                schema: proj_schema,
            };
        }

        Ok(plan)
    }

    fn build_from_clause(
        &self,
        tables: &[(String, Option<String>)],
        catalog: &dyn Catalog,
    ) -> Result<LogicalPlan> {
        if tables.is_empty() {
            return Ok(LogicalPlan::Empty);
        }
        let (first_name, first_alias) = &tables[0];
        let mut plan = self.build_table_scan(first_name, first_alias.as_deref(), catalog)?;

        for (name, alias) in tables.iter().skip(1) {
            let right = self.build_table_scan(name, alias.as_deref(), catalog)?;
            let pairs = join_pairs(plan.output_schema(), right.output_schema());
            let _predicate = combine_and(
                pairs
                    .iter()
                    .map(|(l, r)| Expr::binary(BinaryOp::Eq, Expr::col(l), Expr::col(r)))
                    .collect(),
            );
            let schema = build_join_schema(plan.output_schema(), right.output_schema(), &pairs)?;
            plan = LogicalPlan::Join {
                left: Box::new(plan),
                right: Box::new(right),
                on: pairs,
                how: JoinType::Inner,
                schema,
            };
        }
        Ok(plan)
    }

    fn build_table_scan(
        &self,
        name: &str,
        _alias: Option<&str>,
        catalog: &dyn Catalog,
    ) -> Result<LogicalPlan> {
        let (path, source) = catalog.table_location(name)?;
        let schema = catalog.table_schema(name)?;
        Ok(build_scan_schema(name, &path, &source, schema))
    }
}

fn eval_limit(expr: &SqlExpr) -> Result<usize> {
    match expr {
        SqlExpr::Value(sqlparser::ast::Value::Number(s, _)) => {
            let n: i64 = s
                .parse()
                .map_err(|_| Error::Sql(format!("invalid LIMIT: {s}")))?;
            if n < 0 {
                return Err(Error::Sql("LIMIT must be non-negative".into()));
            }
            Ok(n as usize)
        }
        other => Err(Error::Sql(format!(
            "unsupported LIMIT expression: {}",
            other
        ))),
    }
}

fn join_pairs(left: &Schema, right: &Schema) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for l in left.fields() {
        if let Some(r) = right.field(&l.name) {
            pairs.push((l.name.clone(), r.name.clone()));
        }
    }
    pairs
}

/// Replace any aggregate expressions in `expr` with the corresponding
/// projection entry from `projections` (which carries the alias used in the
/// output schema).
fn rewrite_having(expr: Expr, projections: &[Expr]) -> Result<Expr> {
    if let Some(replacement) = projections.iter().find(|p| expr_strictly_matches(p, &expr)) {
        return Ok(replacement.clone());
    }
    match expr {
        Expr::Binary { op, left, right } => Ok(Expr::binary(
            op,
            rewrite_having(*left, projections)?,
            rewrite_having(*right, projections)?,
        )),
        Expr::Not(inner) => Ok(Expr::not(rewrite_having(*inner, projections)?)),
        Expr::IsNull(inner) => Ok(Expr::is_null(rewrite_having(*inner, projections)?)),
        Expr::IsNotNull(inner) => Ok(Expr::is_not_null(rewrite_having(*inner, projections)?)),
        other => Ok(other),
    }
}

fn expr_strictly_matches(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (
            Expr::Aggregate {
                func: fa,
                arg: aa,
                distinct: da,
            },
            Expr::Aggregate {
                func: fb,
                arg: ab,
                distinct: db,
            },
        ) => fa == fb && da == db && expr_strictly_matches(aa, ab),
        (Expr::Column(na), Expr::Column(nb)) => na == nb,
        (Expr::Literal(la), Expr::Literal(lb)) => la == lb,
        (
            Expr::Binary {
                op: oa,
                left: la,
                right: ra,
            },
            Expr::Binary {
                op: ob,
                left: lb,
                right: rb,
            },
        ) => oa == ob && expr_strictly_matches(la, lb) && expr_strictly_matches(ra, rb),
        (Expr::Aliased { expr: aa, .. }, other) => expr_strictly_matches(aa, other),
        (other, Expr::Aliased { expr: ab, .. }) => expr_strictly_matches(other, ab),
        _ => false,
    }
}

pub trait Catalog: Send + Sync {
    fn table_schema(&self, name: &str) -> Result<Schema>;
    fn table_location(&self, name: &str) -> Result<(String, String)>;
    fn list_tables(&self) -> Result<Vec<String>>;
    fn register_table(
        &mut self,
        name: &str,
        path: &str,
        source: &str,
        schema: Schema,
    ) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::RwLock;

    pub struct InMemoryCatalog {
        pub tables: RwLock<HashMap<String, (String, String, Schema)>>,
    }

    impl InMemoryCatalog {
        pub fn new() -> Self {
            Self {
                tables: RwLock::new(HashMap::new()),
            }
        }
    }

    impl Catalog for InMemoryCatalog {
        fn table_schema(&self, name: &str) -> Result<Schema> {
            self.tables
                .read()
                .unwrap()
                .get(name)
                .map(|t| t.2.clone())
                .ok_or_else(|| Error::NotFound(format!("table '{name}' not found")))
        }
        fn table_location(&self, name: &str) -> Result<(String, String)> {
            self.tables
                .read()
                .unwrap()
                .get(name)
                .map(|t| (t.0.clone(), t.1.clone()))
                .ok_or_else(|| Error::NotFound(format!("table '{name}' not found")))
        }
        fn list_tables(&self) -> Result<Vec<String>> {
            Ok(self.tables.read().unwrap().keys().cloned().collect())
        }
        fn register_table(
            &mut self,
            name: &str,
            path: &str,
            source: &str,
            schema: Schema,
        ) -> Result<()> {
            self.tables.write().unwrap().insert(
                name.to_string(),
                (path.to_string(), source.to_string(), schema),
            );
            Ok(())
        }
    }

    #[test]
    fn plan_simple_select() {
        let mut cat = InMemoryCatalog::new();
        let schema = Schema::new(vec![
            Field::new("id", DataType::Int64),
            Field::new("name", DataType::String),
        ]);
        cat.register_table("users", "/tmp/users.csv", "csv", schema)
            .unwrap();
        let plan = Planner::new()
            .plan_sql("SELECT id, name FROM users", &cat)
            .unwrap();
        match plan {
            LogicalPlan::Project { expressions, .. } => {
                assert_eq!(expressions.len(), 2);
            }
            other => panic!("expected project, got {other:?}"),
        }
    }

    #[test]
    fn plan_with_filter_and_groupby() {
        let mut cat = InMemoryCatalog::new();
        let schema = Schema::new(vec![
            Field::new("dept", DataType::String),
            Field::new("salary", DataType::Float64),
        ]);
        cat.register_table("emp", "/tmp/emp.csv", "csv", schema)
            .unwrap();
        let plan = Planner::new()
            .plan_sql(
                "SELECT dept, AVG(salary) FROM emp WHERE salary > 0 GROUP BY dept",
                &cat,
            )
            .unwrap();
        let rendered = format!("{plan:?}");
        assert!(rendered.contains("Aggregate"));
        assert!(rendered.contains("Filter"));
    }

    #[test]
    fn plan_count_star() {
        let mut cat = InMemoryCatalog::new();
        let schema = Schema::new(vec![Field::new("a", DataType::Int64)]);
        cat.register_table("t", "/tmp/t.csv", "csv", schema)
            .unwrap();
        let plan = Planner::new()
            .plan_sql("SELECT COUNT(*) FROM t", &cat)
            .unwrap();
        let rendered = format!("{plan:?}");
        assert!(rendered.contains("Aggregate"));
    }

    #[test]
    fn plan_order_limit() {
        let mut cat = InMemoryCatalog::new();
        let schema = Schema::new(vec![Field::new("a", DataType::Int64)]);
        cat.register_table("t", "/tmp/t.csv", "csv", schema)
            .unwrap();
        let plan = Planner::new()
            .plan_sql("SELECT a FROM t ORDER BY a DESC LIMIT 10", &cat)
            .unwrap();
        let rendered = format!("{plan:?}");
        assert!(rendered.contains("Sort"));
        assert!(rendered.contains("Limit"));
    }
}
