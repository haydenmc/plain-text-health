pub mod assembler;
pub mod database;
pub mod directives;
mod lexer;
mod parser;
pub mod validator;

pub use assembler::{Diagnostic, Location, Severity, SourceMap};
pub use database::{QueryResult, build_database, run_query};
pub use validator::Validated;

pub fn load(entry: &std::path::Path) -> std::io::Result<Validated> {
    Ok(validator::validate(assembler::assemble(entry)?))
}
