mod assembler;
mod directives;
mod lexer;
mod parser;
mod validator;
mod tables;

use std::{path::{Path, PathBuf}, process::ExitCode};

use clap::{Parser, Subcommand};

use crate::{
    assembler::{Diagnostic, Severity, SourceMap},
    validator::validate,
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

fn check(file: &Path) -> ExitCode {
    let assembled = match assembler::assemble(file) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("pth: cannot read `{}`: {e}", file.display());
            return ExitCode::FAILURE;
        }
    };

    let v = validate(assembled);
    render_diagnostics(&v.sources, &v.diagnostics);

    if v.diagnostics
        .iter()
        .any(|d| matches!(d.severity, Severity::Error))
    {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn main() -> ExitCode {
    env_logger::init();

    let args = Cli::parse();

    match &args.command {
        Command::Check { file } => check(file),
    }
}
