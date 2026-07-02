use rspark_core::error::Result;
use rspark_exec::{ExecutionContext, LocalExecutor};
use rspark_sql::planner::Catalog;
use rspark_sql::Planner;
use rspark_sql::SessionState;
use rspark_storage::writer::render_table;
use rspark_storage::SourceRegistry;
use std::io::{self, BufRead, Write};
use std::sync::Arc;

/// Minimal interactive SQL REPL. Reads statements terminated by `;` from stdin.
pub fn start_repl(session: Arc<SessionState>) {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let registry = Arc::new(SourceRegistry::with_defaults());
    let context = ExecutionContext::new(registry);
    let planner = Planner::new();
    let executor = LocalExecutor::new(&context);

    println!("rspark shell — type SQL terminated by ';'. Use `:tables` to list, `:quit` to exit.");
    let mut buffer = String::new();
    loop {
        if buffer.trim().is_empty() {
            print!("rspark> ");
            let _ = stdout.flush();
        } else {
            print!("...> ");
            let _ = stdout.flush();
        }
        let mut line = String::new();
        if let Err(err) = stdin.lock().read_line(&mut line) {
            eprintln!("read error: {err}");
            break;
        }
        let trimmed = line.trim();
        if trimmed == ":quit" || trimmed == ":exit" {
            break;
        }
        if trimmed == ":tables" {
            match session.list_tables() {
                Ok(tables) => {
                    for t in tables {
                        println!("{t}");
                    }
                }
                Err(err) => eprintln!("error: {err}"),
            }
            buffer.clear();
            continue;
        }
        buffer.push_str(&line);
        if !buffer.trim_end().ends_with(';') {
            continue;
        }
        let stmt = std::mem::take(&mut buffer);
        let stmt = stmt.trim().trim_end_matches(';').to_string();
        if stmt.is_empty() {
            continue;
        }
        match run_one(&planner, &executor, session.as_ref(), &stmt) {
            Ok(text) => print!("{text}"),
            Err(err) => eprintln!("error: {err}"),
        }
    }
}

fn run_one(
    planner: &Planner,
    executor: &LocalExecutor,
    session: &SessionState,
    sql: &str,
) -> Result<String> {
    let plan = planner.plan_sql(sql, session)?;
    let batch = executor.execute(&plan)?;
    Ok(render_table(&batch))
}
