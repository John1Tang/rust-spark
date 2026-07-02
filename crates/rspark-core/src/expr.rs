use crate::error::{Error, Result};
use crate::record::{Record, RecordBatch};
use crate::value::Value;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

impl Literal {
    pub fn to_value(&self) -> Value {
        match self {
            Literal::Null => Value::Null,
            Literal::Bool(v) => Value::Boolean(*v),
            Literal::Int(v) => Value::Int64(*v),
            Literal::Float(v) => Value::Float64(*v),
            Literal::Str(v) => Value::String(v.clone()),
        }
    }
}

impl From<bool> for Literal {
    fn from(v: bool) -> Self {
        Literal::Bool(v)
    }
}

impl From<i64> for Literal {
    fn from(v: i64) -> Self {
        Literal::Int(v)
    }
}

impl From<i32> for Literal {
    fn from(v: i32) -> Self {
        Literal::Int(v as i64)
    }
}

impl From<f64> for Literal {
    fn from(v: f64) -> Self {
        Literal::Float(v)
    }
}

impl From<&str> for Literal {
    fn from(v: &str) -> Self {
        Literal::Str(v.to_string())
    }
}

impl From<String> for Literal {
    fn from(v: String) -> Self {
        Literal::Str(v)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BinaryOp {
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Like,
}

impl BinaryOp {
    pub fn from_sql(op: &sqlparser::ast::BinaryOperator) -> Option<Self> {
        use sqlparser::ast::BinaryOperator;
        Some(match op {
            BinaryOperator::Plus => BinaryOp::Add,
            BinaryOperator::Minus => BinaryOp::Sub,
            BinaryOperator::Multiply => BinaryOp::Mul,
            BinaryOperator::Divide => BinaryOp::Div,
            BinaryOperator::Modulo => BinaryOp::Mod,
            BinaryOperator::Eq => BinaryOp::Eq,
            BinaryOperator::NotEq => BinaryOp::NotEq,
            BinaryOperator::Lt => BinaryOp::Lt,
            BinaryOperator::LtEq => BinaryOp::LtEq,
            BinaryOperator::Gt => BinaryOp::Gt,
            BinaryOperator::GtEq => BinaryOp::GtEq,
            BinaryOperator::And => BinaryOp::And,
            BinaryOperator::Or => BinaryOp::Or,
            BinaryOperator::StringConcat => BinaryOp::Add,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AggregateFn {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

impl AggregateFn {
    pub fn name(&self) -> &'static str {
        match self {
            AggregateFn::Count => "count",
            AggregateFn::Sum => "sum",
            AggregateFn::Avg => "avg",
            AggregateFn::Min => "min",
            AggregateFn::Max => "max",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "count" => Some(AggregateFn::Count),
            "sum" => Some(AggregateFn::Sum),
            "avg" | "average" | "mean" => Some(AggregateFn::Avg),
            "min" => Some(AggregateFn::Min),
            "max" => Some(AggregateFn::Max),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Column(String),
    Literal(Literal),
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Not(Box<Expr>),
    IsNull(Box<Expr>),
    IsNotNull(Box<Expr>),
    If {
        cond: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },
    Aggregate {
        func: AggregateFn,
        arg: Box<Expr>,
        distinct: bool,
    },
    Aliased {
        expr: Box<Expr>,
        alias: String,
    },
    Star,
}

impl Expr {
    pub fn col(name: impl Into<String>) -> Expr {
        Expr::Column(name.into())
    }

    pub fn lit<L: Into<Literal>>(value: L) -> Expr {
        Expr::Literal(value.into())
    }

    pub fn alias(self, name: impl Into<String>) -> Expr {
        Expr::Aliased {
            expr: Box::new(self),
            alias: name.into(),
        }
    }

    pub fn binary(op: BinaryOp, left: Expr, right: Expr) -> Expr {
        Expr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    pub fn and(left: Expr, right: Expr) -> Expr {
        Self::binary(BinaryOp::And, left, right)
    }

    pub fn or(left: Expr, right: Expr) -> Expr {
        Self::binary(BinaryOp::Or, left, right)
    }

    pub fn not(inner: Expr) -> Expr {
        Expr::Not(Box::new(inner))
    }

    pub fn is_null(inner: Expr) -> Expr {
        Expr::IsNull(Box::new(inner))
    }

    pub fn is_not_null(inner: Expr) -> Expr {
        Expr::IsNotNull(Box::new(inner))
    }

    pub fn aggregate(func: AggregateFn, arg: Expr, distinct: bool) -> Expr {
        Expr::Aggregate {
            func,
            arg: Box::new(arg),
            distinct,
        }
    }

    pub fn display_name(&self) -> String {
        match self {
            Expr::Column(name) => name.clone(),
            Expr::Literal(lit) => match lit {
                Literal::Null => "NULL".into(),
                Literal::Bool(v) => v.to_string(),
                Literal::Int(v) => v.to_string(),
                Literal::Float(v) => v.to_string(),
                Literal::Str(v) => v.clone(),
            },
            Expr::Binary { op, left, right } => format!("{} {:?} {}", left.display_name(), op, right.display_name()),
            Expr::Not(inner) => format!("NOT {}", inner.display_name()),
            Expr::IsNull(inner) => format!("{} IS NULL", inner.display_name()),
            Expr::IsNotNull(inner) => format!("{} IS NOT NULL", inner.display_name()),
            Expr::Aggregate { func, arg, .. } => format!("{}({})", func.name(), arg.display_name()),
            Expr::If { .. } => "if".into(),
            Expr::Aliased { alias, .. } => alias.clone(),
            Expr::Star => "*".into(),
        }
    }

    /// Evaluate the expression against a single record and produce a [`Value`].
    /// Returns [`Value::Null`] for nullable paths when inputs are null.
    pub fn eval(&self, record: &Record, batch: &RecordBatch) -> Result<Value> {
        match self {
            Expr::Column(name) => {
                let idx = batch
                    .schema()
                    .index_of(name)
                    .ok_or_else(|| Error::Schema(format!("column '{name}' not found in schema")))?;
                Ok(record
                    .get(idx)
                    .cloned()
                    .unwrap_or(Value::Null))
            }
            Expr::Literal(lit) => Ok(lit.to_value()),
            Expr::Binary { op, left, right } => {
                let lv = left.eval(record, batch)?;
                let rv = right.eval(record, batch)?;
                if lv.is_null() || rv.is_null() {
                    return Ok(Value::Null);
                }
                apply_binary(*op, &lv, &rv)
            }
            Expr::Not(inner) => {
                let v = inner.eval(record, batch)?;
                Ok(match v {
                    Value::Null => Value::Null,
                    Value::Boolean(b) => Value::Boolean(!b),
                    other => {
                        return Err(Error::Type(format!(
                            "NOT requires boolean operand, got {}",
                            other.data_type_name()
                        )))
                    }
                })
            }
            Expr::IsNull(inner) => {
                let v = inner.eval(record, batch)?;
                Ok(Value::Boolean(v.is_null()))
            }
            Expr::IsNotNull(inner) => {
                let v = inner.eval(record, batch)?;
                Ok(Value::Boolean(!v.is_null()))
            }
            Expr::If {
                cond,
                then_expr,
                else_expr,
            } => {
                let c = cond.eval(record, batch)?;
                if matches!(c, Value::Boolean(true)) {
                    then_expr.eval(record, batch)
                } else {
                    else_expr.eval(record, batch)
                }
            }
            Expr::Aggregate { .. } => Err(Error::Execution(
                "aggregate expression cannot be evaluated against a row; use the aggregate operator"
                    .into(),
            )),
            Expr::Aliased { expr, .. } => expr.eval(record, batch),
            Expr::Star => Err(Error::Execution(
                "* cannot be evaluated as a scalar".into(),
            )),
        }
    }

    pub fn references(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_columns(&mut out);
        out.sort();
        out.dedup();
        out
    }

    fn collect_columns(&self, out: &mut Vec<String>) {
        match self {
            Expr::Column(name) => out.push(name.clone()),
            Expr::Literal(_) | Expr::Star => {}
            Expr::Binary { left, right, .. } => {
                left.collect_columns(out);
                right.collect_columns(out);
            }
            Expr::Not(inner) | Expr::IsNull(inner) | Expr::IsNotNull(inner) => {
                inner.collect_columns(out)
            }
            Expr::If { cond, then_expr, else_expr } => {
                cond.collect_columns(out);
                then_expr.collect_columns(out);
                else_expr.collect_columns(out);
            }
            Expr::Aggregate { arg, .. } => arg.collect_columns(out),
            Expr::Aliased { expr, .. } => expr.collect_columns(out),
        }
    }

    pub fn contains_aggregate(&self) -> bool {
        match self {
            Expr::Aggregate { .. } => true,
            Expr::Binary { left, right, .. } => left.contains_aggregate() || right.contains_aggregate(),
            Expr::Not(inner) | Expr::IsNull(inner) | Expr::IsNotNull(inner) => inner.contains_aggregate(),
            Expr::If {
                cond,
                then_expr,
                else_expr,
            } => {
                cond.contains_aggregate()
                    || then_expr.contains_aggregate()
                    || else_expr.contains_aggregate()
            }
            Expr::Aliased { expr, .. } => expr.contains_aggregate(),
            Expr::Column(_) | Expr::Literal(_) | Expr::Star => false,
        }
    }
}

fn apply_binary(op: BinaryOp, left: &Value, right: &Value) -> Result<Value> {
    use Value::*;
    match op {
        BinaryOp::And => logical_and(left, right),
        BinaryOp::Or => logical_or(left, right),
        BinaryOp::Eq => Ok(Boolean(left == right)),
        BinaryOp::NotEq => Ok(Boolean(left != right)),
        BinaryOp::Lt => Ok(Boolean(matches!(left.try_cmp(right)?, Ordering::Less))),
        BinaryOp::LtEq => Ok(Boolean(matches!(
            left.try_cmp(right)?,
            Ordering::Less | Ordering::Equal
        ))),
        BinaryOp::Gt => Ok(Boolean(matches!(left.try_cmp(right)?, Ordering::Greater))),
        BinaryOp::GtEq => Ok(Boolean(matches!(
            left.try_cmp(right)?,
            Ordering::Greater | Ordering::Equal
        ))),
        BinaryOp::Like => apply_like(left, right),
        BinaryOp::Add => numeric_op(left, right, |a, b| a + b, |a, b| a + b),
        BinaryOp::Sub => numeric_op(left, right, |a, b| a - b, |a, b| a - b),
        BinaryOp::Mul => numeric_op(left, right, |a, b| a * b, |a, b| a * b),
        BinaryOp::Div => numeric_div(left, right),
        BinaryOp::Mod => numeric_mod(left, right),
    }
}

fn logical_and(left: &Value, right: &Value) -> Result<Value> {
    match (left, right) {
        (Value::Boolean(a), Value::Boolean(b)) => Ok(Value::Boolean(*a && *b)),
        (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
        (other, _) => Err(Error::Type(format!(
            "AND requires boolean operands, got {}",
            other.data_type_name()
        ))),
    }
}

fn logical_or(left: &Value, right: &Value) -> Result<Value> {
    match (left, right) {
        (Value::Boolean(a), Value::Boolean(b)) => Ok(Value::Boolean(*a || *b)),
        (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
        (other, _) => Err(Error::Type(format!(
            "OR requires boolean operands, got {}",
            other.data_type_name()
        ))),
    }
}

fn numeric_op<I, F>(left: &Value, right: &Value, int_op: I, float_op: F) -> Result<Value>
where
    I: Fn(i64, i64) -> i64,
    F: Fn(f64, f64) -> f64,
{
    if let (Some(a), Some(b)) = (left.cast_to_i64(), right.cast_to_i64()) {
        return Ok(Value::Int64(int_op(a, b)));
    }
    if let (Some(a), Some(b)) = (left.cast_to_f64(), right.cast_to_f64()) {
        return Ok(Value::Float64(float_op(a, b)));
    }
    Err(Error::Type(format!(
        "cannot apply arithmetic to {} and {}",
        left.data_type_name(),
        right.data_type_name()
    )))
}

fn numeric_div(left: &Value, right: &Value) -> Result<Value> {
    if let (Some(a), Some(b)) = (left.cast_to_f64(), right.cast_to_f64()) {
        if b == 0.0 {
            return Ok(Value::Null);
        }
        return Ok(Value::Float64(a / b));
    }
    Err(Error::Type(format!(
        "cannot divide {} by {}",
        left.data_type_name(),
        right.data_type_name()
    )))
}

fn numeric_mod(left: &Value, right: &Value) -> Result<Value> {
    if let (Some(a), Some(b)) = (left.cast_to_i64(), right.cast_to_i64()) {
        if b == 0 {
            return Ok(Value::Null);
        }
        return Ok(Value::Int64(a % b));
    }
    Err(Error::Type(format!(
        "cannot modulo {} by {}",
        left.data_type_name(),
        right.data_type_name()
    )))
}

fn apply_like(left: &Value, right: &Value) -> Result<Value> {
    let pattern = match right {
        Value::String(s) => s.clone(),
        _ => {
            return Err(Error::Type(format!(
                "LIKE pattern must be a string, got {}",
                right.data_type_name()
            )))
        }
    };
    let target = match left {
        Value::String(s) => s.clone(),
        Value::Null => return Ok(Value::Null),
        _ => {
            return Err(Error::Type(format!(
                "LIKE target must be a string, got {}",
                left.data_type_name()
            )))
        }
    };
    let re_pattern = sql_like_to_regex(&pattern);
    let re = regex::Regex::new(&format!("^{}$", re_pattern))
        .map_err(|e| Error::Type(format!("invalid LIKE pattern: {e}")))?;
    Ok(Value::Boolean(re.is_match(&target)))
}

fn sql_like_to_regex(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len() * 2);
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        match c {
            '%' => out.push_str(".*"),
            '_' => out.push('.'),
            '\\' => {
                if let Some(next) = chars.next() {
                    out.push('\\');
                    out.push(next);
                }
            }
            c => out.push_str(&regex::escape(&c.to_string())),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{DataType, Field, Schema};

    fn schema() -> Schema {
        Schema::new(vec![
            Field::new("name", DataType::String),
            Field::new("age", DataType::Int64),
        ])
    }

    #[test]
    fn eval_column_lookup() {
        let schema = schema();
        let batch = RecordBatch::from_records(
            schema.clone(),
            vec![Record::new(vec!["alice".into(), 30.into()])],
        )
        .unwrap();
        let expr = Expr::col("name");
        let value = expr.eval(&batch.records()[0], &batch).unwrap();
        assert_eq!(value, Value::String("alice".into()));
    }

    #[test]
    fn eval_arithmetic() {
        let schema = schema();
        let batch = RecordBatch::from_records(
            schema,
            vec![Record::new(vec!["alice".into(), 30.into()])],
        )
        .unwrap();
        let expr = Expr::binary(
            BinaryOp::Add,
            Expr::col("age"),
            Expr::lit(5i64),
        );
        let value = expr.eval(&batch.records()[0], &batch).unwrap();
        assert_eq!(value, Value::Int64(35));
    }

    #[test]
    fn like_pattern_matches() {
        let schema = schema();
        let batch = RecordBatch::from_records(
            schema,
            vec![Record::new(vec!["alice".into(), 30.into()])],
        )
        .unwrap();
        let expr = Expr::binary(
            BinaryOp::Like,
            Expr::col("name"),
            Expr::lit("a%"),
        );
        let value = expr.eval(&batch.records()[0], &batch).unwrap();
        assert_eq!(value, Value::Boolean(true));
    }
}
