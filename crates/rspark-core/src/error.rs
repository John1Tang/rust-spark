use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("type error: {0}")]
    Type(String),

    #[error("schema error: {0}")]
    Schema(String),

    #[error("execution error: {0}")]
    Execution(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("cluster error: {0}")]
    Cluster(String),

    #[error("sql error: {0}")]
    Sql(String),

    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("csv error: {0}")]
    Csv(#[from] CsvError),

    #[error("network error: {0}")]
    Network(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid state: {0}")]
    InvalidState(String),
}

#[derive(Debug, Error)]
pub enum CsvError {
    #[error(transparent)]
    Inner(#[from] csv::Error),
    #[error("{0}")]
    Message(String),
}

impl From<String> for CsvError {
    fn from(msg: String) -> Self {
        CsvError::Message(msg)
    }
}

impl From<&str> for CsvError {
    fn from(msg: &str) -> Self {
        CsvError::Message(msg.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
