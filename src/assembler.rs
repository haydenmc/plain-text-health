use std::{
    fs, io, path::{Path, PathBuf},
};

use crate::directives::{Directive, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceId(u32);

/// References a fitlog data file, providing its filepath and source text
pub struct Source {
    pub path: PathBuf,
    pub text: String,
}

pub struct SourceMap {
    sources: Vec<Source>,
}

/// References a location inside of a fitlog data file
pub struct Location {
    pub source: SourceId,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

pub struct Diagnostic {
    pub severity: Severity,
    pub msg: String,
    pub location: Location,
}

pub struct Assembled {
    pub sources: SourceMap,
    pub directives: Vec<(SourceId, Directive)>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn assemble(entry: &Path) -> io::Result<Assembled> {
    assemble_with(entry, &|p| fs::read_to_string(p))
}

pub fn assemble_with(
    entry: &Path,
    read: &dyn Fn(&Path) -> io::Result<String>,
) -> io::Result<Assembled> {
    todo!();
}
