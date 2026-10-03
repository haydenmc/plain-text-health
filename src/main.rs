mod assembler;
mod database;
mod directives;
mod lexer;
mod parser;
mod validator;

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use comfy_table::{Table, presets::UTF8_FULL};
use rusqlite::types::Value;

use crate::{
    assembler::{Diagnostic, Severity, SourceMap},
    database::QueryResult,
    validator::{Validated, validate},
};

#[derive(Parser, Debug)]
#[command(name = "pth", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Check {
        #[arg(env = "PTH_FILE")]
        file: PathBuf,
    },
    Query {
        #[arg(env = "PTH_FILE")]
        file: PathBuf,
        #[arg()]
        sql: String,
    },
}

/// returns the line/column index for the given string offset
fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let col = before[line_start..].chars().count() + 1;
    (line, col)
}

/// prints the given set of diagnostics to stderr
fn render_diagnostics(source_map: &SourceMap, diagnostics: &Vec<Diagnostic>) {
    for d in diagnostics {
        let source = source_map.get(d.location.source);
        let (line, col) = line_col(&source.text, d.location.span.start);
        eprintln!(
            "{}@{}:{}: {}: {}",
            source.path.display(),
            line,
            col,
            d.severity,
            d.msg
        );
    }
}

/// Loads, parses, assembles, and validates a given .fitlog file path
fn load(file: &Path) -> Result<Validated, ExitCode> {
    let asm = assembler::assemble(file).map_err(|e| {
        eprintln!("pth: cannot read `{}`: {e}", file.display());
        ExitCode::FAILURE
    })?;
    Ok(validate(asm))
}

/// Determines if any of the given diagnostics are Error-level, and should halt
/// processing
fn any_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|d| matches!(d.severity, Severity::Error))
}

/// Converts a rusqlite value to something printable
fn display_value(v: &Value) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        Value::Integer(n) => n.to_string(),
        Value::Real(f) => f.to_string(),
        Value::Text(s) => s.clone(),
        Value::Blob(b) => format!("<{} bytes>", b.len()),
    }
}

/// Renders a table of the given SQLite query result data using comfy_table
fn render_table(result: &QueryResult) -> Table {
    let mut table = Table::new();
    table.load_style(UTF8_FULL);
    table.set_header(&result.columns);
    for row in &result.rows {
        table.add_row(row.iter().map(display_value));
    }
    table
}

/// CLI command to run a simple check on the given .fitlog path. Will print any
/// diagnostics and return an error code if there are any errors.
fn check(file: &Path) -> ExitCode {
    let Ok(v) = load(file) else {
        return ExitCode::FAILURE;
    };
    render_diagnostics(&v.sources, &v.diagnostics);

    if any_errors(&v.diagnostics) {
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

/// CLI command to run an SQL query against the .fitlog SQLite database.
fn query(file: &Path, sql: &str) -> ExitCode {
    let Ok(v) = load(file) else {
        return ExitCode::FAILURE;
    };
    render_diagnostics(&v.sources, &v.diagnostics);

    if any_errors(&v.diagnostics) {
        return ExitCode::FAILURE;
    }

    let result = match database::build_database(&v).and_then(|conn| database::run_query(&conn, sql))
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("pth: {e}");
            return ExitCode::FAILURE;
        }
    };

    println!("{}", render_table(&result));
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    env_logger::init();

    let args = Cli::parse();

    match &args.command {
        Command::Check { file } => check(file),
        Command::Query { file, sql } => query(file, sql),
    }
}
