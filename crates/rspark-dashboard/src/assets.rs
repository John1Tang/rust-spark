//! Static assets the dashboard serves alongside the single-page UI.
//!
//! Right now this is just the demo "click/collect" page used by the
//! live event collector button in SQL Lab. Embedding it via
//! `include_str!` means the master pod doesn't need an `examples/`
//! directory mounted — the binary is self-contained.

/// Page that emits page_view / page_scroll / page_click events to the
/// rspark-ingest backend. Served at `/examples/e2e/demo_page.html`.
/// See `examples/e2e/drive.js` for the Playwright driver equivalent.
pub const DEMO_PAGE: &str = include_str!("../../../examples/e2e/demo_page.html");

/// Path suffix the dashboard route matches. Kept as a constant so the
/// JS button (in `ui.rs`) and the route share one source of truth.
pub const DEMO_PAGE_PATH: &str = "examples/e2e/demo_page.html";