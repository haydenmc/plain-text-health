use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
};

use crate::{
    assembler::{Assembled, Diagnostic, Location, Severity, SourceId, SourceMap},
    directives::{
        ActivityDecl, Directive, Entry, ExerciseDecl, ExerciseSlotKind, Ident, MetricAliasDecl,
        MetricDecl, RecordLine, RecordSegment, RecordValue, RecordValueKind, Span,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventId(u32);

pub type SymbolTable = HashMap<String, (SourceId, Directive)>;

/// Intermediary data used to group segments by name
struct SegmentGroup<'a> {
    name: &'a Ident,
    segments: Vec<&'a RecordSegment>,
}

/// Output of the validation process
pub struct Validated {
    pub sources: SourceMap,
    pub events: Vec<Event>,
    pub observations: Vec<Observation>,
    pub sets: Vec<Set>,
    pub diagnostics: Vec<Diagnostic>,
}

/// All entries in a fitlog file are Events
pub struct Event {
    pub id: EventId,
    pub date: (u16, u8, u8),
    pub time: Option<(u8, u8)>,
    pub activity: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub metadata: Vec<(String, String)>,
    pub location: Location,
}

/// A metric that has been recorded as part of an event
pub struct Observation {
    pub event_id: EventId,
    pub metric: String,
    pub value: f64,
    pub unit: String,
    pub location: Location,
}

/// An exercise set that has been recorded as part of an event
pub struct Set {
    pub event_id: EventId,
    pub exercise: String,
    pub set_number: u32, // 1-based, the order of the set in the exercise
    pub load: Option<(f64, String)>,
    pub reps: Option<f64>,
    pub duration: Option<(f64, String)>,
    pub distance: Option<(f64, String)>,
    pub location: Location,
}

fn error(src: SourceId, span: Span, message: String) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        msg: message,
        location: Location {
            source: src,
            span: span,
        },
    }
}

pub fn validate(asm: Assembled) -> Validated {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let symbols = build_symbol_table(&asm.directives, &mut diagnostics);
    check_aliases(&symbols, &asm.directives, &mut diagnostics);
    check_exercises(&asm.directives, &mut diagnostics);
    let entries = EntryValidator::new(&symbols).run(&asm.directives);
    todo!()
}

/// Builds a global symbol table and flags errors if there are any duplicates.
fn build_symbol_table(
    directives: &[(SourceId, Directive)],
    diagnostics: &mut Vec<Diagnostic>,
) -> SymbolTable {
    let mut symbols: SymbolTable = HashMap::new();
    for (s, d) in directives.iter() {
        if let Some(symbol_ident) = match d {
            Directive::Metric(decl) => Some(decl.name.clone()),
            Directive::MetricAlias(decl) => Some(decl.name.clone()),
            Directive::Exercise(decl) => Some(decl.name.clone()),
            Directive::Activity(decl) => Some(decl.name.clone()),
            // Intentionally explicitly naming non-symbol directives here so
            // the compiler flags if we ever need to handle new ones.
            Directive::Include(_) | Directive::Entry(_) => None,
        } {
            if symbols.contains_key(&symbol_ident.text) {
                diagnostics.push(Diagnostic {
                    severity: crate::assembler::Severity::Error,
                    msg: format!("duplicate symbol `{}`", symbol_ident.text),
                    location: Location {
                        source: *s,
                        span: symbol_ident.span,
                    },
                });
            } else {
                symbols.insert(symbol_ident.text, (*s, d.clone()));
            }
        }
    }
    symbols
}

fn check_aliases(
    symbols: &SymbolTable,
    directives: &[(SourceId, Directive)],
    diagnostics: &mut Vec<Diagnostic>,
) {
    for d in directives.iter().filter_map(|(src, d)| match d {
        Directive::MetricAlias(m) => Some((*src, m)),
        _ => None,
    }) {
        for s in &d.1.composed_metric_names {
            if !matches!(symbols.get(&s.text), Some((_, Directive::Metric(_)))) {
                diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    msg: format!("unknown metric `{}` referenced in metric alias", s.text),
                    location: Location {
                        source: d.0,
                        span: s.span.clone(),
                    },
                });
            }
        }
    }
}

fn check_exercises(directives: &[(SourceId, Directive)], diagnostics: &mut Vec<Diagnostic>) {
    for d in directives.iter().filter_map(|(src, d)| match d {
        Directive::Exercise(e) => Some((*src, e)),
        _ => None,
    }) {
        let mut slots: HashSet<&ExerciseSlotKind> = HashSet::new();
        for s in &d.1.slots {
            if !slots.insert(s) {
                diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    msg: format!("duplicate exercise slot value"),
                    location: Location {
                        source: d.0,
                        span: d.1.span.clone(),
                    },
                });
            }
        }
    }
}

struct EntryOutput {
    events: Vec<Event>,
    observations: Vec<Observation>,
    sets: Vec<Set>,
    diagnostics: Vec<Diagnostic>,
}

struct EntryValidator<'s> {
    symbols: &'s SymbolTable,
    events: Vec<Event>,
    observations: Vec<Observation>,
    sets: Vec<Set>,
    diagnostics: Vec<Diagnostic>,
}

impl<'s> EntryValidator<'s> {
    fn new(symbols: &'s SymbolTable) -> Self {
        Self {
            symbols,
            events: Vec::new(),
            observations: Vec::new(),
            sets: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn run(mut self, directives: &[(SourceId, Directive)]) -> EntryOutput {
        for (src, d) in directives {
            if let Directive::Entry(e) = d {
                self.entry(*src, e);
            }
        }
        EntryOutput {
            events: self.events,
            observations: self.observations,
            sets: self.sets,
            diagnostics: self.diagnostics,
        }
    }

    fn entry(&mut self, src: SourceId, e: &Entry) {
        let event_id = EventId(self.events.len() as u32);
        self.events.push(Event {
            id: event_id,
            date: e.date,
            time: e.time,
            activity: e.activity.as_ref().map(|a| a.name.text.clone()),
            description: e.activity.as_ref().and_then(|a| a.description.clone()),
            tags: e.tags.clone(),
            metadata: e
                .metadata
                .iter()
                .map(|m| (m.key.text.clone(), m.value.clone()))
                .collect(),
            location: Location {
                source: src,
                span: e.span.clone(),
            },
        });

        let symbols = self.symbols;
        let mut next_set_num: u32 = 0;
        for line in &e.records {
            for g in group_segments(line) {
                match symbols.get(&g.name.text) {
                    Some((_, Directive::Metric(m))) => {
                        match metric_observation(src, event_id, m, &g) {
                            Ok(o) => self.observations.push(o),
                            Err(d) => self.diagnostics.push(d),
                        }
                    }
                    Some((_, Directive::MetricAlias(m))) => {
                        match metric_alias_observation(src, event_id, m, symbols, &g) {
                            Ok(o) => self.observations.extend(o),
                            Err(d) => self.diagnostics.push(d),
                        }
                    }
                    Some((_, Directive::Exercise(e))) => {
                        match metric_exercise_set(src, event_id, e, &g, &mut next_set_num) {
                            Ok(s) => self.sets.extend(s),
                            Err(d) => self.diagnostics.push(d),
                        }
                    }
                    Some(_) => {
                        self.diagnostics.push(error(
                            src,
                            g.name.span.clone(),
                            format!("symbol `{}` is of incorrect type", g.name.text),
                        ));
                    }
                    None => {
                        self.diagnostics.push(error(
                            src,
                            g.name.span.clone(),
                            format!("could not find declaration for symbol `{}`", g.name.text),
                        ));
                    }
                }
            }
        }
    }
}

/// Record lines can have multiple segments. For convenience, names may be
/// omitted from a segment, which will assume the name of a previous segment.
/// This function groups segments together that belong to the same name.
fn group_segments(line: &RecordLine) -> Vec<SegmentGroup> {
    let mut groups: Vec<SegmentGroup> = Vec::new();
    for seg in &line.segments {
        match &seg.name {
            Some(n) => groups.push(SegmentGroup {
                name: n,
                segments: vec![seg],
            }),
            None => match groups.last_mut() {
                Some(g) => g.segments.push(seg),
                None => groups.push(SegmentGroup {
                    name: &line.name,
                    segments: vec![seg],
                }),
            },
        }
    }
    groups
}

fn metric_observation(
    src: SourceId,
    event: EventId,
    m: &MetricDecl,
    g: &SegmentGroup,
) -> Result<Observation, Diagnostic> {
    // Every metric segment group should only have one segment by this point,
    // since we group by name, and all measurement segments must have a name.
    // VALID ex. 2026-08-05 weight 178.4 lb, bodyfat 18.2 %
    // INVALID ex. 2026-08-05 weight 178.4 lb, 180.1 lb
    let [seg] = g.segments[..] else {
        return Err(error(
            src,
            g.segments[1].span.clone(),
            format!("every measurement value must be named"),
        ));
    };
    if seg.values.len() != 1 {
        return Err(error(
            src,
            seg.span.clone(),
            format!("segments must have only one value"),
        ));
    }
    let RecordValueKind::Single(n) = seg.values[0].value else {
        return Err(error(
            src,
            seg.values[0].span.clone(),
            format!("`{}` takes a single value", m.name.text),
        ));
    };
    if let Some(u) = &seg.values[0].unit {
        if u.text != m.unit.text {
            return Err(error(
                src,
                u.span.clone(),
                format!(
                    "`{}` stated with `{}` units, expected `{}`",
                    m.name.text, m.unit.text, u.text
                ),
            ));
        }
    }
    Ok(Observation {
        event_id: event,
        location: Location {
            source: src,
            span: seg.span.clone(),
        },
        metric: m.name.text.clone(),
        unit: m.unit.text.clone(),
        value: n,
    })
}

fn metric_alias_observation(
    src: SourceId,
    event: EventId,
    a: &MetricAliasDecl,
    symbols: &SymbolTable,
    g: &SegmentGroup,
) -> Result<Vec<Observation>, Diagnostic> {
    let [seg] = g.segments[..] else {
        return Err(error(
            src,
            g.segments[1].span.clone(),
            format!("every measurement value must be named"),
        ));
    };

    let [value] = &seg.values[..] else {
        return Err(error(
            src,
            seg.span.clone(),
            format!(
                "metric alias `{}` expects a single slash-separated value, ex. `118/76`",
                a.name.text
            ),
        ));
    };

    let RecordValueKind::List(nums) = &value.value else {
        return Err(error(
            src,
            value.span.clone(),
            format!(
                "metric alias `{}` expects {} values. Separate the values with `/`",
                a.name.text,
                a.composed_metric_names.len()
            ),
        ));
    };

    if nums.len() != a.composed_metric_names.len() {
        return Err(error(
            src,
            value.span.clone(),
            format!(
                "metric alias `{}` expects {} values, found {}",
                a.name.text,
                a.composed_metric_names.len(),
                nums.len()
            ),
        ));
    }

    a.composed_metric_names
        .iter()
        .zip(nums)
        .map(|(name, &n)| {
            let Some((_, Directive::Metric(m))) = symbols.get(&name.text) else {
                return Err(error(
                    src,
                    value.span.clone(),
                    format!(
                        "metric alias `{}` refers to `{}`, which is not a declared metric",
                        a.name.text, name.text
                    ),
                ));
            };
            if let Some(u) = &value.unit {
                if u.text != m.unit.text {
                    return Err(error(
                        src,
                        value.span.clone(),
                        format!(
                            "`{}` stated with `{}` units, expected `{}`",
                            name.text, u.text, m.unit.text
                        ),
                    ));
                }
            }
            Ok(Observation {
                event_id: event,
                metric: name.text.clone(),
                value: n,
                location: Location {
                    source: src,
                    span: seg.span.clone(),
                },
                unit: m.unit.text.clone(),
            })
        })
        .collect()
}

/// Translates a RecordValue into an ExerciseSlotKind (load, duration, distance,
/// reps) based off of units
fn exercise_slot_for(src: SourceId, v: &RecordValue) -> Result<ExerciseSlotKind, Diagnostic> {
    let Some(u) = &v.unit else {
        return Ok(ExerciseSlotKind::Reps);
    };
    match u.text.as_str() {
        "lb" | "kg" => Ok(ExerciseSlotKind::Load),
        "sec" | "min" => Ok(ExerciseSlotKind::Duration),
        "m" | "km" | "mi" | "ft" | "yd" => Ok(ExerciseSlotKind::Distance),
        other => Err(error(
            src,
            u.span.clone(),
            format!(
                "unknown unit `{other}` for exercise (expected lb, kg, sec, min, m, km, mi, ft, or yd)"
            ),
        )),
    }
}

/// Used to determine exercise values when expanding slash-lists. Slash-listed
/// values are returned based on index, other values are simply repeated. See
/// README.md for more information on slash-listing.
fn exercise_value_at(v: &RecordValue, i: usize) -> f64 {
    match &v.value {
        RecordValueKind::Single(n) => *n,
        RecordValueKind::List(ns) => ns[i],
    }
}

/// Returns unit text for a given exercise record value.
/// Reps values, which are unitless, should not be passed to this function.
fn exercise_unit_text(v: &RecordValue) -> String {
    v.unit
        .as_ref()
        .expect("non-reps slot has a unit")
        .text
        .clone()
}

/// Returns the string name for a given exercise slot kind
fn exercise_slot_name(s: &ExerciseSlotKind) -> &'static str {
    match &s {
        ExerciseSlotKind::Distance => "distance",
        ExerciseSlotKind::Duration => "duration",
        ExerciseSlotKind::Load => "load",
        ExerciseSlotKind::Reps => "reps",
    }
}

fn metric_exercise_set(
    src: SourceId,
    event: EventId,
    e: &ExerciseDecl,
    g: &SegmentGroup,
    next_set: &mut u32,
) -> Result<Vec<Set>, Diagnostic> {
    let start = *next_set;
    let mut out = Vec::new();

    for seg in &g.segments {
        // Categorize segments to slots by their units
        let mut assigned: HashMap<ExerciseSlotKind, &RecordValue> = HashMap::new();
        for v in &seg.values {
            let slot = exercise_slot_for(src, v)?;
            if !e.slots.contains(&slot) {
                return Err(error(
                    src,
                    v.span.clone(),
                    format!(
                        "`{}` is not defined with a `{}` slot",
                        e.name.text,
                        exercise_slot_name(&slot)
                    ),
                ));
            }
            if assigned.insert(slot, v).is_some() {
                return Err(error(
                    src,
                    v.span.clone(),
                    format!("`{}` slot used more than once", exercise_slot_name(&slot)),
                ));
            }
        }

        // Ensure every slot declared by the exercise has been assigned
        for &slot in &e.slots {
            if !assigned.contains_key(&slot) {
                return Err(error(
                    src,
                    seg.span.clone(),
                    format!(
                        "`{}` is missing `{}`",
                        e.name.text,
                        exercise_slot_name(&slot)
                    ),
                ));
            }
        }

        // Ensure there is at most one slash-listed value set
        // first, filter to only lists
        let mut lists = seg.values.iter().filter_map(|v| match &v.value {
            RecordValueKind::List(ns) => Some((v, ns.len())),
            RecordValueKind::Single(_) => None,
        });
        // determine how many "sets" we will need to iterate through, depending
        // on how many lists we've found
        let count = match (lists.next(), lists.next()) {
            (None, _) => 1,            // no lists - only one set
            (Some((_, n)), None) => n, // 1 list - n sets based on list length
            (Some(_), Some((second, _))) => {
                // multiple lists - invalid
                return Err(error(
                    src,
                    second.span.clone(),
                    "only one value per group can be slash-listed".to_string(),
                ));
            }
        };

        // Expand sets
        // The pick function will return the value and units of the appropriate
        // index for a slash-listed slot, but will repeat values for a non-
        // slash-listed slot.
        let pick = |slot, i| assigned.get(&slot).map(|v| (exercise_value_at(v, i), v));
        for i in 0..count {
            out.push(Set {
                event_id: event,
                exercise: e.name.text.clone(),
                set_number: start + out.len() as u32 + 1,
                load: pick(ExerciseSlotKind::Load, i).map(|(n, v)| (n, exercise_unit_text(v))),
                reps: pick(ExerciseSlotKind::Reps, i).map(|(n, _)| n),
                duration: pick(ExerciseSlotKind::Duration, i)
                    .map(|(n, v)| (n, exercise_unit_text(v))),
                distance: pick(ExerciseSlotKind::Distance, i)
                    .map(|(n, v)| (n, exercise_unit_text(v))),
                location: Location {
                    source: src,
                    span: seg.span.clone(),
                },
            });
        }
    }

    *next_set += out.len() as u32;
    Ok(out)
}
