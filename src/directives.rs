//! The types that define the directives in each .fitlog file

pub type Span = std::ops::Range<usize>;

/// Used for identifiers (names of metrics, exercises, activities, etc)
/// Includes a Span to point out the original location for
/// debugging/diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub struct Ident {
    pub text: String,
    pub span: Span,
}

/// Every `.fitlog` file is made up of a series of "directives."
#[derive(Debug, Clone, PartialEq)]
pub enum Directive {
    Include(Include),
    Metric(MetricDecl),
    MetricAlias(MetricAliasDecl),
    Exercise(ExerciseDecl),
    Activity(ActivityDecl),
    Entry(Entry),
}

/// Directive to include the file at the given path.
#[derive(Debug, Clone, PartialEq)]
pub struct Include {
    pub path: String,
    pub span: Span,
}

// Declarations

/// Metric declaration. States what types of measurements will be tracked in
/// Entries.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricDecl {
    pub name: Ident,
    pub unit: Ident,
    pub is_additive: bool,
    pub span: Span,
}

/// Metric Alias declaration. Used to combine two or more metrics into a single
/// shorthand expression.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricAliasDecl {
    pub name: Ident,
    pub composed_metric_names: Vec<Ident>,
    pub span: Span,
}

/// Types of metrics that can be recorded within each exercise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExerciseSlotKind {
    Load,
    Reps,
    Duration,
    Distance,
}

/// Exercise declaration. Used to express a specific piece of work inside of an
/// activity. Includes 1 - 4 "slots" where one of `load`, `reps`, `duration`,
/// and/or `distance` can be recorded at entry time.
#[derive(Debug, Clone, PartialEq)]
pub struct ExerciseDecl {
    pub name: Ident,
    pub slots: Vec<ExerciseSlotKind>,
    pub span: Span,
}

/// Activity declaration. States what kind of activity entries can be recorded.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityDecl {
    pub name: Ident,
    pub span: Span,
}

/// An Entry is a directive that records data, as opposed to declaration
/// directives that define what sort of data can be recorded.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub date: (u16, u8, u8), // YYYY, MM, DD
    pub time: Option<(u8, u8)>, // HH, MM
    pub kind: EntryKind,
    pub tags: Vec<String>,
    pub metadata: Vec<MetaItem>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EntryKind {
    Measurements(Vec<Measurement>),
    Activity {
        name: Ident,
        description: Option<String>,
        measurements: Vec<Measurement>,
        exercises: Vec<Exercise>,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MeasureValue {
    Scalar(f64),
    Compound(Vec<f64>),
}

/// A measurement and its value defined within an Entry
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    pub metric: Ident,
    pub value: MeasureValue,
    pub unit: Option<Ident>,
    pub span: Span,
}

/// Slash listing is a shorthand for recording multiple values for the same
/// slot (ex. 6/5/4 reps)
#[derive(Debug, Clone, PartialEq)]
pub enum SlotValueKind {
    Single(f64),
    List(Vec<f64>), // for slash listing
}

#[derive(Debug, Clone, PartialEq)]
pub struct SlotValue {
    pub unit: Option<Ident>,
    pub value: SlotValueKind,
    pub span: Span,
}

/// Exercises can consist of multiple segments with different slot values
/// separated by commas.
#[derive(Debug, Clone, PartialEq)]
pub struct ExerciseSegment {
    pub values: Vec<SlotValue>,
    pub span: Span,
}

/// Exercise is a part of an Activity entry that specifies a specific type and
/// amount of work done.
#[derive(Debug, Clone, PartialEq)]
pub struct Exercise {
    pub name: Ident,
    pub segments: Vec<ExerciseSegment>,
    pub span: Span,
}

/// Freeform string key/value metadata that can be specified with an Entry
#[derive(Debug, Clone, PartialEq)]
pub struct MetaItem {
    pub key: Ident,
    pub value: String,
    pub span: Span,
}
