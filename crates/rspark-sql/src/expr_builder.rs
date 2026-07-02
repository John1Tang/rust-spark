use rspark_core::error::{Error, Result};
use rspark_core::expr::{AggregateFn, BinaryOp, Expr, Literal};
use rspark_core::value::Value;
use sqlparser::ast::{
    BinaryOperator, Expr as SqlExpr, Function, FunctionArg, FunctionArgExpr, FunctionArgumentList,
    FunctionArguments, GroupByExpr, JoinConstraint, JoinOperator, TableFactor, TableWithJoins,
};

/// Translate a `sqlparser` expression into our internal [`Expr`] form.
pub fn build_expr(expr: &SqlExpr) -> Result<Expr> {
    match expr {
        SqlExpr::Identifier(ident) => Ok(Expr::col(ident.value.clone())),
        SqlExpr::CompoundIdentifier(parts) => Ok(Expr::col(parts.last().unwrap().value.clone())),
        SqlExpr::Wildcard => Ok(Expr::Star),
        SqlExpr::QualifiedWildcard(parts) => {
            if let Some(last) = parts.0.last() {
                Ok(Expr::col(last.value.clone()))
            } else {
                Err(Error::Sql("qualified wildcard with no parts".into()))
            }
        }
        SqlExpr::Value(v) => Ok(Expr::Literal(literal_from_value(v)?)),
        SqlExpr::BinaryOp { left, op, right } => {
            let op = BinaryOp::from_sql(op)
                .ok_or_else(|| Error::Sql(format!("unsupported binary operator: {:?}", op)))?;
            Ok(Expr::binary(op, build_expr(left)?, build_expr(right)?))
        }
        SqlExpr::UnaryOp { op, expr } => match op {
            sqlparser::ast::UnaryOperator::Not => Ok(Expr::not(build_expr(expr)?)),
            sqlparser::ast::UnaryOperator::Minus => Ok(Expr::binary(
                BinaryOp::Sub,
                Expr::lit(0i64),
                build_expr(expr)?,
            )),
            sqlparser::ast::UnaryOperator::Plus => build_expr(expr),
            other => Err(Error::Sql(format!(
                "unsupported unary operator: {:?}",
                other
            ))),
        },
        SqlExpr::IsNull(inner) => Ok(Expr::is_null(build_expr(inner)?)),
        SqlExpr::IsNotNull(inner) => Ok(Expr::is_not_null(build_expr(inner)?)),
        SqlExpr::IsTrue(inner) => Ok(Expr::binary(
            BinaryOp::Eq,
            build_expr(inner)?,
            Expr::lit(true),
        )),
        SqlExpr::IsFalse(inner) => Ok(Expr::binary(
            BinaryOp::Eq,
            build_expr(inner)?,
            Expr::lit(false),
        )),
        SqlExpr::IsNotTrue(inner) => Ok(Expr::binary(
            BinaryOp::NotEq,
            build_expr(inner)?,
            Expr::lit(true),
        )),
        SqlExpr::IsNotFalse(inner) => Ok(Expr::binary(
            BinaryOp::NotEq,
            build_expr(inner)?,
            Expr::lit(false),
        )),
        SqlExpr::Nested(inner) => build_expr(inner),
        SqlExpr::Like {
            expr,
            pattern,
            negated,
            ..
        } => {
            let base = Expr::binary(BinaryOp::Like, build_expr(expr)?, build_expr(pattern)?);
            if *negated {
                Ok(Expr::not(base))
            } else {
                Ok(base)
            }
        }
        SqlExpr::ILike {
            expr,
            pattern,
            negated,
            ..
        } => {
            let base = Expr::binary(BinaryOp::Like, build_expr(expr)?, build_expr(pattern)?);
            if *negated {
                Ok(Expr::not(base))
            } else {
                Ok(base)
            }
        }
        SqlExpr::Between {
            expr,
            low,
            high,
            negated,
        } => {
            let ge = Expr::binary(BinaryOp::GtEq, build_expr(expr)?, build_expr(low)?);
            let le = Expr::binary(BinaryOp::LtEq, build_expr(expr)?, build_expr(high)?);
            let between = Expr::and(ge, le);
            if *negated {
                Ok(Expr::not(between))
            } else {
                Ok(between)
            }
        }
        SqlExpr::InList {
            expr,
            list,
            negated,
        } => {
            let target = build_expr(expr)?;
            let mut or_chain: Option<Expr> = None;
            for item in list {
                let cmp = Expr::binary(BinaryOp::Eq, target.clone(), build_expr(item)?);
                or_chain = Some(match or_chain {
                    None => cmp,
                    Some(prev) => Expr::or(prev, cmp),
                });
            }
            let predicate = or_chain.unwrap_or(Expr::lit(false));
            Ok(if *negated {
                Expr::not(predicate)
            } else {
                predicate
            })
        }
        SqlExpr::Function(func) => build_function_call(func),
        SqlExpr::Cast { expr, .. } => build_expr(expr),
        SqlExpr::Subquery(_) => Err(Error::Sql("scalar subqueries are not yet supported".into())),
        other => Err(Error::Sql(format!("unsupported SQL expression: {}", other))),
    }
}

fn literal_from_value(v: &sqlparser::ast::Value) -> Result<Literal> {
    use sqlparser::ast::Value as SqlValue;
    Ok(match v {
        SqlValue::Null => Literal::Null,
        SqlValue::Boolean(b) => Literal::Bool(*b),
        SqlValue::Number(s, _) => {
            if let Ok(int_val) = s.parse::<i64>() {
                Literal::Int(int_val)
            } else if let Ok(f) = s.parse::<f64>() {
                Literal::Float(f)
            } else {
                return Err(Error::Sql(format!("invalid numeric literal: {s}")));
            }
        }
        SqlValue::SingleQuotedString(s) | SqlValue::DoubleQuotedString(s) => {
            Literal::Str(s.clone())
        }
        SqlValue::NationalStringLiteral(s) | SqlValue::HexStringLiteral(s) => {
            Literal::Str(s.clone())
        }
        other => {
            return Err(Error::Sql(format!("unsupported literal value: {}", other)));
        }
    })
}

fn build_function_call(func: &Function) -> Result<Expr> {
    let name = func.name.to_string().to_ascii_lowercase();
    let (args_list, distinct) = extract_args(func)?;

    if let Some(aggregate) = AggregateFn::from_name(&name) {
        if args_list.len() != 1 {
            return Err(Error::Sql(format!(
                "{name}() requires exactly one argument"
            )));
        }
        let arg = match &args_list[0] {
            ArgSpec::Expr(e) => e.clone(),
            ArgSpec::Wildcard => Expr::Star,
        };
        if matches!(arg, Expr::Star) {
            return Ok(Expr::aggregate(aggregate, Expr::lit(1i64), distinct));
        }
        return Ok(Expr::aggregate(aggregate, arg, distinct));
    }
    let args = materialize_args(&args_list)?;
    match name.as_str() {
        "coalesce" => {
            if args.is_empty() {
                return Ok(Expr::Literal(Literal::Null));
            }
            let mut iter = args.into_iter();
            let mut acc = iter.next().unwrap();
            for next in iter {
                let test = Expr::is_not_null(acc.clone());
                let default = next;
                // Build IF(test, acc, IF(...))
                acc = Expr::If {
                    cond: Box::new(test),
                    then_expr: Box::new(acc),
                    else_expr: Box::new(default),
                };
            }
            Ok(acc)
        }
        "nvl" => {
            if args.len() != 2 {
                return Err(Error::Sql("NVL requires two arguments".into()));
            }
            let mut iter = args.into_iter();
            let expr2 = iter.next().unwrap();
            let expr1 = iter.next().unwrap();
            Ok(Expr::If {
                cond: Box::new(Expr::is_not_null(expr1.clone())),
                then_expr: Box::new(expr1),
                else_expr: Box::new(expr2),
            })
        }
        "abs" | "upper" | "ucase" | "lower" | "lcase" => {
            if args.len() != 1 {
                return Err(Error::Sql(format!("{name}() requires one argument")));
            }
            Ok(args.into_iter().next().unwrap())
        }
        "length" | "char_length" | "character_length" => {
            let _ = args;
            Err(Error::Sql(format!(
                "{name}() requires runtime function support not yet implemented"
            )))
        }
        other => Err(Error::Sql(format!("unsupported function: {other}()"))),
    }
}

enum ArgSpec {
    Expr(Expr),
    Wildcard,
}

fn extract_args(func: &Function) -> Result<(Vec<ArgSpec>, bool)> {
    match &func.args {
        FunctionArguments::None => Ok((vec![], false)),
        FunctionArguments::Subquery(_) => Err(Error::Sql(
            "function subquery arguments are not supported".into(),
        )),
        FunctionArguments::List(FunctionArgumentList {
            args,
            duplicate_treatment,
            ..
        }) => {
            let mut out = Vec::with_capacity(args.len());
            for arg in args {
                match arg {
                    FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => {
                        out.push(ArgSpec::Expr(build_expr(e)?))
                    }
                    FunctionArg::Unnamed(FunctionArgExpr::Wildcard) => out.push(ArgSpec::Wildcard),
                    FunctionArg::Named { arg, .. } => match arg {
                        FunctionArgExpr::Expr(e) => out.push(ArgSpec::Expr(build_expr(e)?)),
                        FunctionArgExpr::Wildcard => out.push(ArgSpec::Wildcard),
                        _ => {
                            return Err(Error::Sql(
                                "unsupported qualified function argument".into(),
                            ))
                        }
                    },
                    _ => return Err(Error::Sql("unsupported function argument kind".into())),
                }
            }
            let distinct = matches!(
                duplicate_treatment,
                Some(sqlparser::ast::DuplicateTreatment::Distinct)
            );
            Ok((out, distinct))
        }
    }
}

fn materialize_args(args_list: &[ArgSpec]) -> Result<Vec<Expr>> {
    let mut out = Vec::with_capacity(args_list.len());
    for spec in args_list {
        match spec {
            ArgSpec::Expr(e) => out.push(e.clone()),
            ArgSpec::Wildcard => {
                return Err(Error::Sql(
                    "* not allowed in non-aggregate argument position".into(),
                ))
            }
        }
    }
    Ok(out)
}

/// Translate a `sqlparser` [`SelectItem`] list into our internal expressions.
pub fn build_select_expressions(select: &sqlparser::ast::Select) -> Result<Vec<Expr>> {
    let mut exprs = Vec::new();
    for item in &select.projection {
        match item {
            sqlparser::ast::SelectItem::UnnamedExpr(e) => exprs.push(build_expr(e)?),
            sqlparser::ast::SelectItem::ExprWithAlias { expr, alias } => {
                let inner = build_expr(expr)?;
                exprs.push(inner.alias(alias.value.clone()));
            }
            sqlparser::ast::SelectItem::QualifiedWildcard(_, _)
            | sqlparser::ast::SelectItem::Wildcard(_) => exprs.push(Expr::Star),
        }
    }
    Ok(exprs)
}

pub fn build_group_by(group: &GroupByExpr) -> Result<Vec<Expr>> {
    match group {
        GroupByExpr::All(_) => Ok(vec![]),
        GroupByExpr::Expressions(exprs, _) => exprs.iter().map(build_expr).collect(),
    }
}

pub fn build_join_constraint(
    constraint: &JoinConstraint,
    left_alias: Option<&str>,
    right_alias: Option<&str>,
) -> Result<Option<Expr>> {
    match constraint {
        JoinConstraint::On(expr) => Ok(Some(build_expr(expr)?)),
        JoinConstraint::Using(idents) => {
            let mut conjuncts = Vec::new();
            for ident in idents {
                let name = ident.value.clone();
                let l = match left_alias {
                    Some(a) => Expr::col(format!("{a}.{name}")),
                    None => Expr::col(name.clone()),
                };
                let r = match right_alias {
                    Some(a) => Expr::col(format!("{a}.{name}")),
                    None => Expr::col(name.clone()),
                };
                conjuncts.push(Expr::binary(BinaryOp::Eq, l, r));
            }
            Ok(Some(combine_and(conjuncts)))
        }
        JoinConstraint::Natural => Ok(None),
        JoinConstraint::None => Ok(None),
    }
}

pub fn combine_and(mut exprs: Vec<Expr>) -> Expr {
    if exprs.is_empty() {
        return Expr::lit(true);
    }
    let mut acc = exprs.remove(0);
    for next in exprs {
        acc = Expr::and(acc, next);
    }
    acc
}

pub fn build_table_factor(factor: &TableFactor) -> Result<(String, Option<String>, Vec<String>)> {
    match factor {
        TableFactor::Table { name, alias, .. } => {
            let base = name.to_string();
            let alias_name = alias.as_ref().map(|a| a.name.value.clone());
            Ok((base, alias_name, vec![]))
        }
        other => Err(Error::Sql(format!("unsupported table factor: {}", other))),
    }
}

pub fn build_join_operator(op: &JoinOperator) -> JoinKind {
    match op {
        JoinOperator::Inner(_) => JoinKind::Inner,
        JoinOperator::LeftOuter(_) => JoinKind::Left,
        JoinOperator::RightOuter(_) => JoinKind::Right,
        JoinOperator::FullOuter(_) => JoinKind::Full,
        JoinOperator::CrossJoin => JoinKind::Inner,
        JoinOperator::LeftSemi(_) | JoinOperator::RightSemi(_) => JoinKind::Inner,
        JoinOperator::LeftAnti(_) | JoinOperator::RightAnti(_) => JoinKind::Inner,
        _ => JoinKind::Inner,
    }
}

#[derive(Debug, Clone, Copy)]
pub enum JoinKind {
    Inner,
    Left,
    Right,
    Full,
}

pub fn from_value(v: &Value) -> Literal {
    match v {
        Value::Null => Literal::Null,
        Value::Boolean(b) => Literal::Bool(*b),
        Value::Int32(i) => Literal::Int(*i as i64),
        Value::Int64(i) => Literal::Int(*i),
        Value::Float32(f) => Literal::Float(*f as f64),
        Value::Float64(f) => Literal::Float(*f),
        Value::String(s) => Literal::Str(s.clone()),
    }
}

pub fn build_from_tables(tables: &[TableWithJoins]) -> Result<Vec<(String, Option<String>)>> {
    let mut out = Vec::new();
    for twj in tables {
        let (name, alias, _) = build_table_factor(&twj.relation)?;
        out.push((name, alias));
        for join in &twj.joins {
            let (n, a, _) = build_table_factor(&join.relation)?;
            out.push((n, a));
        }
    }
    Ok(out)
}

/// Returns true if the binary operator should be considered commutative (used during plan rewriting).
pub fn is_commutative_op(op: &BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::Eq | BinaryOp::NotEq | BinaryOp::Add | BinaryOp::Mul
    )
}

#[allow(dead_code)]
pub fn binary_op_name(op: &BinaryOperator) -> &'static str {
    match op {
        BinaryOperator::Plus => "+",
        BinaryOperator::Minus => "-",
        BinaryOperator::Multiply => "*",
        BinaryOperator::Divide => "/",
        BinaryOperator::Modulo => "%",
        BinaryOperator::StringConcat => "||",
        BinaryOperator::Eq => "=",
        BinaryOperator::NotEq => "<>",
        BinaryOperator::Lt => "<",
        BinaryOperator::LtEq => "<=",
        BinaryOperator::Gt => ">",
        BinaryOperator::GtEq => ">=",
        BinaryOperator::And => "AND",
        BinaryOperator::Or => "OR",
        _ => "?",
    }
}
