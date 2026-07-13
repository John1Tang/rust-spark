//! Spark SQL parser and logical planner.
//!
//! Builds a [`LogicalPlan`] tree from a SQL string using `sqlparser-rs`.

pub mod expr_builder;
pub mod parser;
pub mod plan;
pub mod planner;
pub mod session;
pub mod show_create;
pub mod table_kind;

pub use plan::{JoinType, LogicalPlan, SortExpr};
pub use planner::Planner;
pub use session::SessionState;
pub use show_create::{render_create_table, try_show_create, ShowCreateRequest};
pub use table_kind::TableKind;
