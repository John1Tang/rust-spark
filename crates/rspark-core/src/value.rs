use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

/// A nullable value matching the [`crate::schema::DataType`] algebra.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Null,
    Boolean(bool),
    Int32(i32),
    Int64(i64),
    Float32(f32),
    Float64(f64),
    String(String),
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn data_type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Boolean(_) => "boolean",
            Value::Int32(_) => "int",
            Value::Int64(_) => "bigint",
            Value::Float32(_) => "float",
            Value::Float64(_) => "double",
            Value::String(_) => "string",
        }
    }

    pub fn cast_to_string(&self) -> String {
        match self {
            Value::Null => "null".to_string(),
            Value::Boolean(v) => v.to_string(),
            Value::Int32(v) => v.to_string(),
            Value::Int64(v) => v.to_string(),
            Value::Float32(v) => v.to_string(),
            Value::Float64(v) => v.to_string(),
            Value::String(v) => v.clone(),
        }
    }

    pub fn cast_to_f64(&self) -> Option<f64> {
        match self {
            Value::Null => None,
            Value::Boolean(b) => Some(if *b { 1.0 } else { 0.0 }),
            Value::Int32(v) => Some(*v as f64),
            Value::Int64(v) => Some(*v as f64),
            Value::Float32(v) => Some(*v as f64),
            Value::Float64(v) => Some(*v),
            Value::String(s) => s.parse().ok(),
        }
    }

    pub fn cast_to_i64(&self) -> Option<i64> {
        match self {
            Value::Null => None,
            Value::Boolean(b) => Some(if *b { 1 } else { 0 }),
            Value::Int32(v) => Some(*v as i64),
            Value::Int64(v) => Some(*v),
            Value::Float32(v) => Some(*v as i64),
            Value::Float64(v) => Some(*v as i64),
            Value::String(s) => s.parse().ok(),
        }
    }

    pub fn cast_to_bool(&self) -> Option<bool> {
        match self {
            Value::Null => None,
            Value::Boolean(b) => Some(*b),
            Value::Int32(v) => Some(*v != 0),
            Value::Int64(v) => Some(*v != 0),
            Value::Float32(v) => Some(*v != 0.0),
            Value::Float64(v) => Some(*v != 0.0),
            Value::String(s) => match s.to_ascii_lowercase().as_str() {
                "true" | "t" | "1" | "yes" | "y" => Some(true),
                "false" | "f" | "0" | "no" | "n" => Some(false),
                _ => None,
            },
        }
    }

    pub fn try_cmp(&self, other: &Value) -> Result<Ordering> {
        match (self, other) {
            (Value::Null, Value::Null) => Ok(Ordering::Equal),
            (Value::Null, _) => Ok(Ordering::Less),
            (_, Value::Null) => Ok(Ordering::Greater),
            (Value::Boolean(a), Value::Boolean(b)) => Ok(a.cmp(b)),
            (Value::Int32(a), Value::Int32(b)) => Ok(a.cmp(b)),
            (Value::Int64(a), Value::Int64(b)) => Ok(a.cmp(b)),
            (Value::Float32(a), Value::Float32(b)) => a
                .partial_cmp(b)
                .ok_or_else(|| Error::Type(format!("cannot compare NaN floats {} and {}", a, b))),
            (Value::Float64(a), Value::Float64(b)) => a
                .partial_cmp(b)
                .ok_or_else(|| Error::Type(format!("cannot compare NaN floats {} and {}", a, b))),
            (Value::String(a), Value::String(b)) => Ok(a.cmp(b)),
            (Value::Int32(a), Value::Int64(b)) => Ok((*a as i64).cmp(b)),
            (Value::Int64(a), Value::Int32(b)) => Ok(a.cmp(&(*b as i64))),
            (Value::Int32(a), Value::Float64(b)) => (*a as f64)
                .partial_cmp(b)
                .ok_or_else(|| Error::Type(format!("cannot compare NaN floats {} and {}", a, b))),
            (Value::Float64(a), Value::Int32(b)) => a
                .partial_cmp(&(*b as f64))
                .ok_or_else(|| Error::Type(format!("cannot compare NaN floats {} and {}", a, b))),
            (Value::Int64(a), Value::Float64(b)) => (*a as f64)
                .partial_cmp(b)
                .ok_or_else(|| Error::Type(format!("cannot compare NaN floats {} and {}", a, b))),
            (Value::Float64(a), Value::Int64(b)) => a
                .partial_cmp(&(*b as f64))
                .ok_or_else(|| Error::Type(format!("cannot compare NaN floats {} and {}", a, b))),
            (Value::Float32(a), Value::Float64(b)) => (*a as f64)
                .partial_cmp(b)
                .ok_or_else(|| Error::Type(format!("cannot compare NaN floats {} and {}", a, b))),
            (Value::Float64(a), Value::Float32(b)) => a
                .partial_cmp(&(*b as f64))
                .ok_or_else(|| Error::Type(format!("cannot compare NaN floats {} and {}", a, b))),
            (a, b) => Err(Error::Type(format!(
                "cannot compare {} and {}",
                a.data_type_name(),
                b.data_type_name()
            ))),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => f.write_str("null"),
            Value::Boolean(v) => write!(f, "{}", v),
            Value::Int32(v) => write!(f, "{}", v),
            Value::Int64(v) => write!(f, "{}", v),
            Value::Float32(v) => write!(f, "{}", v),
            Value::Float64(v) => write!(f, "{}", v),
            Value::String(v) => write!(f, "{}", v),
        }
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Boolean(v)
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Int32(v)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int64(v)
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Float32(v)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float64(v)
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::String(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::String(v.to_string())
    }
}
