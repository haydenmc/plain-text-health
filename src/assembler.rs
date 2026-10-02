use core::fmt;
#[cfg(test)]
use std::collections::HashMap;
use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
};

use crate::{
    directives::{Directive, Span},
    parser::parse,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceId(u32);

/// References a fitlog data file, providing its filepath and source text
#[derive(Debug)]
pub struct Source {
    pub path: PathBuf,
    pub text: String,
}

/// Maintains a set of source files (path and contents) mapped to an integer ID
#[derive(Default, Debug)]
pub struct SourceMap {
    sources: Vec<Source>,
}

impl SourceMap {
    pub fn add(&mut self, path: PathBuf, text: String) -> SourceId {
        self.sources.push(Source { path, text });
        SourceId((self.sources.len() - 1) as u32)
    }

    pub fn get(&self, id: SourceId) -> &Source {
        &self.sources[id.0 as usize]
    }

    pub fn len(&self) -> usize {
        self.sources.len()
    }
}

/// References a location inside of a fitlog data file
#[derive(Debug)]
pub struct Location {
    pub source: SourceId,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Severity::Error => "error",
        })
    }
}

/// Represents problems that can be encountered during the parsing/assembling
/// process.
#[derive(Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub msg: String,
    pub location: Location,
}

/// Fully assembled collection of fitlog files, including source file data,
/// directives, and diagnostics generated during processing.
#[derive(Default, Debug)]
pub struct Assembled {
    pub sources: SourceMap,
    pub directives: Vec<(SourceId, Directive)>,
    pub diagnostics: Vec<Diagnostic>,
}

/// The SourceTextProvider abstracts how the assembler reads file text.
/// In production, these functions will read directly from disk.
/// In testing, it's useful to override this functionality to directly inject
/// source text.
pub trait SourceTextProvider {
    fn read(&self, path: &Path) -> io::Result<String>;
    fn path_to_key(&self, path: &Path) -> io::Result<PathBuf>;
}

/// An implementation of SourceTextProvider that reads files directly from disk.
pub struct DiskSourceTextProvider;

impl SourceTextProvider for DiskSourceTextProvider {
    fn read(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }

    fn path_to_key(&self, path: &Path) -> io::Result<PathBuf> {
        fs::canonicalize(path)
    }
}

/// An implementation of SourceTextProvider that reads files from a hashmap
/// keyed by file path. Useful for test scenarios where we don't want to hit the
/// actual disk.
#[cfg(test)]
pub(crate) struct MapSourceTextProvider(HashMap<PathBuf, String>);

#[cfg(test)]
impl MapSourceTextProvider {
    pub(crate) fn new(files: &[(&str, &str)]) -> Self {
        Self(
            files
                .iter()
                .map(|(p, t)| (PathBuf::from(p), t.to_string()))
                .collect(),
        )
    }
}

#[cfg(test)]
impl SourceTextProvider for MapSourceTextProvider {
    fn read(&self, path: &Path) -> io::Result<String> {
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn path_to_key(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(path.to_path_buf())
    }
}

/// Parses and assembles a series of fitlog files starting with the given path.
pub fn assemble(entry: &Path) -> io::Result<Assembled> {
    assemble_with(entry, &DiskSourceTextProvider)
}

/// Parses and assembles a series of fitlog files starting with the given path,
/// allows overriding the SourceTextProvider to change how source text is read
/// from a given file path.
pub fn assemble_with(entry: &Path, provider: &dyn SourceTextProvider) -> io::Result<Assembled> {
    let mut out = Assembled::default();
    let mut visited = HashSet::new();
    // Entrypoint failures propagate directly to the caller, since they are
    // pathless.
    if let Some(text) = open(provider, entry, &mut visited)? {
        walk(entry, text, provider, &mut out, &mut visited);
    }
    Ok(out)
}

/// Checks/marks if this path has already been read, then reads the contents.
fn open(
    provider: &dyn SourceTextProvider,
    path: &Path,
    visited: &mut HashSet<PathBuf>,
) -> io::Result<Option<String>> {
    let key = provider.path_to_key(path)?;
    if !visited.insert(key) {
        // we've already read this file - return None
        return Ok(None);
    }
    provider.read(path).map(Some)
}

/// Parses the given path's text contents into directives, and does the same for
/// any `include` directives contained within.
fn walk(
    path: &Path,
    text: String,
    provider: &dyn SourceTextProvider,
    out: &mut Assembled,
    visited: &mut HashSet<PathBuf>,
) {
    // add to source map, assign ID
    let id = out.sources.add(path.to_path_buf(), text);

    // parse into directives (and parsing errors)
    let (directives, errors) = parse(&out.sources.get(id).text);

    // map errors into our diagnostics output
    out.diagnostics
        .extend(errors.into_iter().map(|e| Diagnostic {
            severity: Severity::Error,
            msg: e.msg,
            location: Location {
                source: id,
                span: e.span,
            },
        }));

    // walk any additional include directives
    for d in directives {
        match d {
            Directive::Include(inc) => {
                let target = path.parent().unwrap_or(Path::new("")).join(&inc.path);
                match open(provider, &target, visited) {
                    Ok(Some(text)) => walk(&target, text, provider, out, visited),
                    Ok(None) => {} // already assembled
                    Err(e) => out.diagnostics.push(Diagnostic {
                        severity: Severity::Error,
                        msg: format!("could not include `{}`: {}", target.display(), e),
                        location: Location {
                            source: id,
                            span: inc.span,
                        },
                    }),
                }
            }
            other => out.directives.push((id, other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assemble with the first file as the entrypoint.
    fn run(files: &[(&str, &'static str)]) -> Assembled {
        let provider = MapSourceTextProvider::new(files);
        assemble_with(Path::new(files[0].0), &provider).expect("entrypoint readable")
    }

    /// Pulls the list of metric names out of the assembled directives
    fn metric_names(out: &Assembled) -> Vec<&str> {
        out.directives
            .iter()
            .filter_map(|(_, d)| match d {
                Directive::Metric(m) => Some(m.name.text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn single_file_no_includes() {
        let out = run(&[("a.fitlog", "metric weight lb\nmetric steps steps additive")]);
        assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
        assert_eq!(out.sources.len(), 1);
        assert_eq!(metric_names(&out), ["weight", "steps"]);
    }

    #[test]
    fn simple_include_order() {
        let out = run(&[
            ("a.fitlog", "!include \"b.fitlog\"\nmetric weight lb"),
            ("b.fitlog", "metric steps steps additive"),
        ]);
        assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
        assert_eq!(out.sources.len(), 2);
        // b's directives appear at the include's position, before a's later ones
        assert_eq!(metric_names(&out), ["steps", "weight"]);
        assert!(
            !out.directives
                .iter()
                .any(|(_, d)| matches!(d, Directive::Include(_)))
        );
    }

    #[test]
    fn missing_include() {
        let out = run(&[("a.fitlog", "!include \"nope.fitlog\"\nmetric weight lb")]);
        let [d] = &out.diagnostics[..] else {
            panic!("expected one diagnostic: {:#?}", out.diagnostics)
        };
        assert_eq!(d.severity, Severity::Error);
        assert!(d.msg.contains("nope.fitlog"), "{}", d.msg);
        let src = out.sources.get(d.location.source);
        assert_eq!(src.path, Path::new("a.fitlog"));
        assert_eq!(
            &src.text[d.location.span.clone()],
            "!include \"nope.fitlog\""
        );
        // the rest of the file still assembles
        assert_eq!(metric_names(&out), ["weight"]);
    }

    #[test]
    fn cyclical_include() {
        let out = run(&[
            ("a.fitlog", "!include \"b.fitlog\""),
            ("b.fitlog", "!include \"a.fitlog\""),
        ]);
        assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
        assert_eq!(out.sources.len(), 2);
    }

    #[test]
    fn multiple_includes_of_same_file() {
        let out = run(&[
            ("a.fitlog", "!include \"b.fitlog\"\n!include \"c.fitlog\""),
            ("b.fitlog", "!include \"d.fitlog\""),
            ("c.fitlog", "!include \"d.fitlog\""),
            ("d.fitlog", "metric weight lb"),
        ]);
        assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
        assert_eq!(out.sources.len(), 4);
        assert_eq!(metric_names(&out), ["weight"]);
    }

    #[test]
    fn parse_error_attribution() {
        let out = run(&[
            ("a.fitlog", "!include \"b.fitlog\"\nmetric weight lb"),
            ("b.fitlog", "metrc steps steps"),
        ]);
        let [d] = &out.diagnostics[..] else {
            panic!("expected one diagnostic: {:#?}", out.diagnostics)
        };
        assert_eq!(
            out.sources.get(d.location.source).path,
            Path::new("b.fitlog")
        );
        assert_eq!(metric_names(&out), ["weight"]);
    }

    #[test]
    fn missing_entrypoint() {
        let provider = MapSourceTextProvider::new(&[]);
        let err = assemble_with(Path::new("missing.fitlog"), &provider).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}
