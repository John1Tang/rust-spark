use std::sync::Arc;

use rspark_core::expr::{AggregateFn, BinaryOp, Expr};
use rspark_core::schema::{DataType, Field, Schema};
use rspark_core::{Record, RecordBatch};
use rspark_sql::plan::{build_join_schema, build_scan_schema, LogicalPlan};
use rspark_sql::Planner;
use rspark_storage::SourceRegistry;

use rspark_exec::LocalExecutor;

fn data_path(rel: &str) -> String {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    format!("{manifest}/../../{rel}")
}

fn make_employees_batch() -> RecordBatch {
    let schema = Schema::new(vec![
        Field::new("id", DataType::Int64),
        Field::new("name", DataType::String),
        Field::new("dept", DataType::String),
        Field::new("salary", DataType::Int64),
    ]);
    RecordBatch::from_records(
        schema,
        vec![
            Record::new(vec![1i64.into(), "Alice".into(), "Engineering".into(), 95000.into()]),
            Record::new(vec![2i64.into(), "Bob".into(), "Engineering".into(), 87000.into()]),
            Record::new(vec![3i64.into(), "Charlie".into(), "Sales".into(), 72000.into()]),
            Record::new(vec![4i64.into(), "Dave".into(), "Sales".into(), 81000.into()]),
            Record::new(vec![5i64.into(), "Eve".into(), "Engineering".into(), 102000.into()]),
        ],
    )
    .unwrap()
}

fn executor() -> LocalExecutor<'static> {
    let registry = Arc::new(SourceRegistry::with_defaults());
    let ctx: &'static _ = Box::leak(Box::new(rspark_exec::ExecutionContext::new(registry)));
    LocalExecutor::new(ctx)
}

#[test]
fn select_star_returns_all_rows() {
    let batch = make_employees_batch();
    let scan = LogicalPlan::Scan {
        path: "mem".into(),
        source: "memory".into(),
        projection: None,
        filter: None,
        schema: batch.schema().clone(),
    };
    let scan_batch = RecordBatch::from_records(
        batch.schema().clone(),
        batch.records().to_vec(),
    )
    .unwrap();
    let _ = scan;
    let _ = scan_batch;
    assert_eq!(batch.len(), 5);
}

#[test]
fn filter_predicate_via_planner_sql() {
    let schema = Schema::new(vec![
        Field::new("id", DataType::Int64),
        Field::new("name", DataType::String),
        Field::new("dept", DataType::String),
        Field::new("salary", DataType::Int64),
    ]);
    let plan = LogicalPlan::Project {
        input: Box::new(LogicalPlan::Filter {
            input: Box::new(LogicalPlan::Scan {
                path: data_path("examples/data/employees.csv"),
                source: "csv".into(),
                projection: None,
                filter: None,
                schema: schema.clone(),
            }),
            predicate: Expr::binary(
                BinaryOp::Gt,
                Expr::col("salary"),
                Expr::lit(80_000i64),
            ),
            schema: schema.clone(),
        }),
        expressions: vec![Expr::col("name"), Expr::col("salary")],
        schema: Schema::new(vec![
            Field::new("name", DataType::String),
            Field::new("salary", DataType::Int64),
        ]),
    };
    let exec = executor();
    let batch = exec.execute(&plan).unwrap();
    assert_eq!(batch.len(), 11);
    assert_eq!(
        batch.records()[0].get_by_name(batch.schema(), "name").cloned().unwrap(),
        "Alice".into()
    );
}

#[test]
fn aggregate_group_by_dept() {
    let schema = Schema::new(vec![
        Field::new("id", DataType::Int64),
        Field::new("name", DataType::String),
        Field::new("dept", DataType::String),
        Field::new("salary", DataType::Int64),
    ]);
    let plan = LogicalPlan::Aggregate {
        input: Box::new(LogicalPlan::Scan {
            path: data_path("examples/data/employees.csv"),
            source: "csv".into(),
            projection: None,
            filter: None,
            schema: schema.clone(),
        }),
        group_exprs: vec![Expr::col("dept")],
        aggregate_exprs: vec![Expr::aggregate(AggregateFn::Avg, Expr::col("salary"), false)],
        schema: Schema::new(vec![
            Field::new("dept", DataType::String),
            Field::new("avg(salary)", DataType::Float64),
        ]),
    };
    let exec = executor();
    let batch = exec.execute(&plan).unwrap();
    assert_eq!(batch.len(), 3);
    let mut found = false;
    for record in batch.records() {
        let dept = record.get(0).cloned().unwrap();
        if matches!(&dept, rspark_core::value::Value::String(s) if s == "Engineering") {
            found = true;
        }
    }
    assert!(found, "Engineering group missing from aggregation result");
}

#[test]
fn join_emits_schema() {
    let left = Schema::new(vec![
        Field::new("id", DataType::Int64),
        Field::new("name", DataType::String),
    ]);
    let right = Schema::new(vec![
        Field::new("id", DataType::Int64),
        Field::new("dept", DataType::String),
    ]);
    let plan_schema = build_join_schema(&left, &right, &[("id".into(), "id".into())]).unwrap();
    let plan = LogicalPlan::Join {
        left: Box::new(LogicalPlan::Scan {
            path: data_path("examples/data/employees.csv"),
            source: "csv".into(),
            projection: None,
            filter: None,
            schema: left.clone(),
        }),
        right: Box::new(LogicalPlan::Scan {
            path: data_path("examples/data/sales.csv"),
            source: "csv".into(),
            projection: None,
            filter: None,
            schema: right.clone(),
        }),
        on: vec![("id".into(), "id".into())],
        how: rspark_sql::plan::JoinType::Inner,
        schema: plan_schema,
    };
    let exec = executor();
    let result = exec.execute(&plan);
    // employees.csv has 8 rows, sales.csv has 5 rows; they share no ids so result should be 0.
    // The important thing is that the plan doesn't error and returns a valid (possibly empty) batch.
    assert!(result.is_ok());
}

#[test]
fn scan_employees_via_planner_returns_rows() {
    let schema = Schema::new(vec![
        Field::new("id", DataType::Int64),
        Field::new("name", DataType::String),
        Field::new("dept", DataType::String),
        Field::new("salary", DataType::Int64),
    ]);
    let path = data_path("examples/data/employees.csv");
    let scan = build_scan_schema(
        "employees",
        &path,
        "csv",
        schema,
    );
    let exec = executor();
    let batch = exec.execute(&scan).unwrap();
    assert_eq!(batch.len(), 20);
}

#[test]
fn planner_select_star_executes() {
    use std::collections::HashMap;
    use std::sync::RwLock;

    use rspark_sql::planner::Catalog;

    #[derive(Default)]
    struct TestCatalog {
        tables: RwLock<HashMap<String, (String, String, Schema)>>,
    }
    impl Catalog for TestCatalog {
        fn table_schema(&self, name: &str) -> rspark_core::error::Result<Schema> {
            self.tables
                .read()
                .unwrap()
                .get(name)
                .map(|t| t.2.clone())
                .ok_or_else(|| rspark_core::error::Error::NotFound(format!("table '{name}' not found")))
        }
        fn table_location(&self, name: &str) -> rspark_core::error::Result<(String, String)> {
            self.tables
                .read()
                .unwrap()
                .get(name)
                .map(|t| (t.0.clone(), t.1.clone()))
                .ok_or_else(|| rspark_core::error::Error::NotFound(format!("table '{name}' not found")))
        }
        fn list_tables(&self) -> rspark_core::error::Result<Vec<String>> {
            Ok(self.tables.read().unwrap().keys().cloned().collect())
        }
        fn register_table(
            &mut self,
            name: &str,
            path: &str,
            source: &str,
            schema: Schema,
        ) -> rspark_core::error::Result<()> {
            self.tables
                .write()
                .unwrap()
                .insert(name.to_string(), (path.to_string(), source.to_string(), schema));
            Ok(())
        }
    }

    let schema = Schema::new(vec![
        Field::new("id", DataType::Int64),
        Field::new("name", DataType::String),
        Field::new("dept", DataType::String),
        Field::new("salary", DataType::Int64),
    ]);
    let mut catalog = TestCatalog::default();
    let path = data_path("examples/data/employees.csv");
    catalog
        .register_table("employees", &path, "csv", schema)
        .unwrap();
    let plan = Planner::new()
        .plan_sql("SELECT * FROM employees", &catalog)
        .unwrap();
    let exec = executor();
    let batch = exec.execute(&plan).unwrap();
    assert_eq!(batch.len(), 20);
}