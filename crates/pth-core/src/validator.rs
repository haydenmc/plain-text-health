use std::{
    collections::{HashMap, HashSet, hash_map},
    hash::Hash,
};

use crate::{
    assembler::{Assembled, Diagnostic, Location, Severity, SourceId, SourceMap},
    directives::{
        Directive, Entry, ExerciseDecl, ExerciseSlotKind, Ident, MetricAliasDecl, MetricDecl,
        RecordLine, RecordSegment, RecordValue, RecordValueKind, Span,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventId(u32);

impl EventId {
    pub fn get(self) -> u32 {
        self.0
    }
}

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
    #[expect(dead_code, reason = "diagnostic data, not yet used/exposed")]
    pub location: Location,
}

/// A metric that has been recorded as part of an event
pub struct Observation {
    pub event_id: EventId,
    pub metric: String,
    pub value: f64,
    pub unit: String,
    #[expect(dead_code, reason = "diagnostic data, not yet used/exposed")]
    pub location: Location,
}

/// An exercise set that has been recorded as part of an event
pub struct Set {
    pub event_id: EventId,
    pub exercise: String,
    pub set_number: u32, // 1-based, the order of the set in the event, across all exercises
    pub load: Option<(f64, String)>,
    pub reps: Option<f64>,
    pub duration: Option<(f64, String)>,
    pub distance: Option<(f64, String)>,
    #[expect(dead_code, reason = "diagnostic data, not yet used/exposed")]
    pub location: Location,
}

fn error(src: SourceId, span: Span, message: String) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        msg: message,
        location: Location { source: src, span },
    }
}

pub fn validate(asm: Assembled) -> Validated {
    let mut diagnostics: Vec<Diagnostic> = asm.diagnostics;
    let symbols = build_symbol_table(&asm.directives, &mut diagnostics);
    check_aliases(&symbols, &asm.directives, &mut diagnostics);
    check_exercises(&asm.directives, &mut diagnostics);
    let entries = EntryValidator::new(&symbols).run(&asm.directives);
    diagnostics.extend(entries.diagnostics);
    Validated {
        sources: asm.sources,
        events: entries.events,
        observations: entries.observations,
        sets: entries.sets,
        diagnostics,
    }
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
            match symbols.entry(symbol_ident.text) {
                hash_map::Entry::Occupied(existing) => {
                    diagnostics.push(error(
                        *s,
                        symbol_ident.span,
                        format!("duplicate symbol `{}`", existing.key()),
                    ));
                }
                hash_map::Entry::Vacant(slot) => {
                    slot.insert((*s, d.clone()));
                }
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
                    msg: "duplicate exercise slot value".to_string(),
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

/// A utility struct used to process directives into events with observations
/// and sets.
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

    /// Processes the given directives into events, observations, and sets.
    /// After this is called, the EntryValidator object is no longer valid.
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

    /// Processes the given Entry directive into an Event, along with any
    /// attached observations and sets.
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

        // If this is an activity, confirm it has been declared
        if let Some(a) = &e.activity
            && !matches!(
                self.symbols.get(&a.name.text),
                Some((_, Directive::Activity(_)))
            )
        {
            self.diagnostics.push(error(
                src,
                a.name.span.clone(),
                format!("`{}` is not a declared activity", a.name.text),
            ));
        }

        // Process each record line:
        // - group the segments by name (unnamed segments belong to the last
        //   name)
        // - look up the name and process the group based on the type (metric,
        //   metric alias, exercise)
        let mut next_set_num: u32 = 0;
        for line in &e.records {
            for g in group_segments(line) {
                match self.symbols.get(&g.name.text) {
                    Some((_, Directive::Metric(m))) => {
                        match metric_observation(src, event_id, m, &g) {
                            Ok(o) => self.observations.push(o),
                            Err(d) => self.diagnostics.push(d),
                        }
                    }
                    Some((_, Directive::MetricAlias(m))) => {
                        match metric_alias_observation(src, event_id, m, self.symbols, &g) {
                            Ok(o) => self.observations.extend(o),
                            Err(d) => self.diagnostics.push(d),
                        }
                    }
                    Some((_, Directive::Exercise(x))) => {
                        if e.activity.is_none() {
                            self.diagnostics.push(error(
                                src,
                                g.name.span.clone(),
                                format!(
                                    "exercise `{}` must be recorded within an activity",
                                    x.name.text
                                ),
                            ));
                        } else {
                            match metric_exercise_set(src, event_id, x, &g, &mut next_set_num) {
                                Ok(s) => self.sets.extend(s),
                                Err(d) => self.diagnostics.push(d),
                            }
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
fn group_segments(line: &RecordLine) -> Vec<SegmentGroup<'_>> {
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

/// Processes the given metric segment group into an observation instance.
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
            "every measurement value must be named".to_string(),
        ));
    };
    if seg.values.len() != 1 {
        return Err(error(
            src,
            seg.span.clone(),
            "segments must have only one value".to_string(),
        ));
    }
    let RecordValueKind::Single(n) = seg.values[0].value else {
        return Err(error(
            src,
            seg.values[0].span.clone(),
            format!("`{}` takes a single value", m.name.text),
        ));
    };
    if let Some(u) = &seg.values[0].unit
        && u.text != m.unit.text
    {
        return Err(error(
            src,
            u.span.clone(),
            format!(
                "`{}` stated with `{}` units, expected `{}`",
                m.name.text, u.text, m.unit.text
            ),
        ));
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

/// Processes the given metric alias segment into a set of observation instances
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
            "every measurement value must be named".to_string(),
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
            if let Some(u) = &value.unit
                && u.text != m.unit.text
            {
                return Err(error(
                    src,
                    u.span.clone(),
                    format!(
                        "`{}` stated with `{}` units, expected `{}`",
                        name.text, u.text, m.unit.text
                    ),
                ));
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
        "sec" | "min" | "hr" => Ok(ExerciseSlotKind::Duration),
        "m" | "km" | "mi" | "ft" | "yd" => Ok(ExerciseSlotKind::Distance),
        other => Err(error(
            src,
            u.span.clone(),
            format!(
                "unknown unit `{other}` for exercise (expected lb, kg, sec, min, hr, m, km, mi, ft, or yd)"
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

/// Processes the given exercise segment group into a set of Set instances
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembler::{MapSourceTextProvider, assemble_with};
    use std::path::Path;

    const DECLS: &str = "\
metric weight lb
metric bodyfat %
metric bp_sys mmHg
metric bp_dia mmHg
metric bp = bp_sys / bp_dia
activity lift
activity hike
exercise bench_press load reps
exercise dumbbell_press load reps
exercise dumbbell_curl load reps
exercise plank duration
exercise pullups reps
";

    fn run(body: &str) -> Validated {
        let main = format!("!include \"decls.fitlog\"\n{body}\n");
        let provider =
            MapSourceTextProvider::new(&[("main.fitlog", &main), ("decls.fitlog", DECLS)]);
        let asm = assemble_with(Path::new("main.fitlog"), &provider).expect("entrypoint readable");
        validate(asm)
    }

    fn errors(v: &Validated) -> Vec<&Diagnostic> {
        v.diagnostics
            .iter()
            .filter(|d| matches!(d.severity, Severity::Error))
            .collect()
    }

    fn assert_clean(v: &Validated) {
        let errs = errors(v);
        assert!(errs.is_empty(), "unexpected errors: {errs:#?}");
    }

    /// Asserts exactly one error whose message contains `needle`, and returns
    /// the source text the error points at.
    fn single_error<'v>(v: &'v Validated, needle: &str) -> &'v str {
        let errs = errors(v);
        let [d] = &errs[..] else {
            panic!("expected exactly one error, got {errs:#?}")
        };
        assert!(
            d.msg.contains(needle),
            "{:?} does not mention {:?}",
            d.msg,
            needle
        );
        &v.sources.get(d.location.source).text[d.location.span.clone()]
    }

    // ---- metrics ----

    #[test]
    fn metric_with_stated_unit() {
        let v = run("2026-08-05 weight 178.4 lb");
        assert_clean(&v);
        let [o] = &v.observations[..] else {
            panic!("expected one observation")
        };
        assert_eq!(
            (o.metric.as_str(), o.value, o.unit.as_str()),
            ("weight", 178.4, "lb")
        );
        assert_eq!(v.events.len(), 1);
        assert_eq!(v.events[0].activity, None);
    }

    #[test]
    fn metric_unit_inferred_from_declaration() {
        let v = run("2026-08-05 weight 178.4");
        assert_clean(&v);
        assert_eq!(v.observations[0].unit, "lb");
    }

    #[test]
    fn metric_unit_mismatch() {
        let v = run("2026-08-05 weight 80 kg");
        assert_eq!(
            single_error(&v, "stated with `kg` units, expected `lb`"),
            "kg"
        );
        assert!(v.observations.is_empty());
    }

    #[test]
    fn named_continuations_are_separate_measurements() {
        let v = run("2026-08-05 weight 178.4 lb, bodyfat 18.2 %, weight 180.1");
        assert_clean(&v);
        let names: Vec<_> = v.observations.iter().map(|o| o.metric.as_str()).collect();
        assert_eq!(names, ["weight", "bodyfat", "weight"]);
    }

    #[test]
    fn nameless_continuation_after_metric_is_an_error() {
        let v = run("2026-08-05 weight 178.4 lb, 180.1 lb");
        assert_eq!(single_error(&v, "must be named"), "180.1 lb");
        assert!(v.observations.is_empty());
    }

    #[test]
    fn unknown_metric() {
        let v = run("2026-08-05 wieght 178.4");
        assert_eq!(single_error(&v, "wieght"), "wieght");
    }

    // ---- aliases ----

    #[test]
    fn alias_expands_to_components() {
        let v = run("2026-08-05 bp 118/76");
        assert_clean(&v);
        let got: Vec<_> = v
            .observations
            .iter()
            .map(|o| (o.metric.as_str(), o.value, o.unit.as_str()))
            .collect();
        assert_eq!(got, [("bp_sys", 118.0, "mmHg"), ("bp_dia", 76.0, "mmHg")]);
    }

    #[test]
    fn alias_wrong_num_values() {
        let v = run("2026-08-05 bp 118/76/50");
        single_error(&v, "expects 2 values, found 3");
        assert!(v.observations.is_empty());
    }

    #[test]
    fn alias_with_single_value() {
        let v = run("2026-08-05 bp 118");
        single_error(&v, "Separate the values with `/`");
    }

    // ---- activities ----

    #[test]
    fn undeclared_activity() {
        let v = run("2026-08-05 hikee \"Cougar Mountain\"");
        assert_eq!(single_error(&v, "not a declared activity"), "hikee");
    }

    #[test]
    fn exercise_outside_activity() {
        let v = run("2026-08-05 bench_press 185 lb 5");
        single_error(&v, "within an activity");
        assert!(v.sets.is_empty());
    }

    // ---- exercises ----

    #[test]
    fn drop_set_expands_to_four_sets() {
        let v = run(r#"2026-08-05 lift
  dumbbell_press 6/5/4 25 lb, 3 20 lb"#);
        assert_clean(&v);
        let got: Vec<_> = v
            .sets
            .iter()
            .map(|s| {
                (
                    s.set_number,
                    s.load.as_ref().map(|(n, u)| (*n, u.as_str())),
                    s.reps,
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                (1, Some((25.0, "lb")), Some(6.0)),
                (2, Some((25.0, "lb")), Some(5.0)),
                (3, Some((25.0, "lb")), Some(4.0)),
                (4, Some((20.0, "lb")), Some(3.0)),
            ]
        );
    }

    #[test]
    fn set_numbers_follow_written_order_across_exercises() {
        let v = run(r#"2026-08-05 lift
  dumbbell_curl 30 lb 10
  bench_press 185 lb 5
  dumbbell_curl 30 lb 8"#);
        assert_clean(&v);
        let got: Vec<_> = v
            .sets
            .iter()
            .map(|s| (s.set_number, s.exercise.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (1, "dumbbell_curl"),
                (2, "bench_press"),
                (3, "dumbbell_curl")
            ]
        );
    }

    #[test]
    fn failed_group_does_not_consume_set_numbers() {
        let v = run(r#"2026-08-05 lift
  bench_press 185 lb 5, 175 lb
  dumbbell_curl 30 lb 10"#);
        single_error(&v, "missing `reps`");
        let [s] = &v.sets[..] else {
            panic!("expected one set")
        };
        assert_eq!((s.exercise.as_str(), s.set_number), ("dumbbell_curl", 1));
    }

    #[test]
    fn missing_slot() {
        let v = run("2026-08-05 lift\n  bench_press 185 lb");
        single_error(&v, "missing `reps`");
    }

    #[test]
    fn undeclared_slot() {
        let v = run("2026-08-05 lift\n  pullups 25 lb 8");
        assert_eq!(single_error(&v, "`load` slot"), "25 lb");
    }

    #[test]
    fn only_one_slash_list_per_group() {
        let v = run("2026-08-05 lift\n  bench_press 185/175 lb 5/5");
        assert_eq!(single_error(&v, "only one value"), "5/5");
    }

    #[test]
    fn all_duration_units_accepted() {
        let v = run("2026-08-05 lift\n  plank 90 sec\n  plank 2 min\n  plank 1 hr");
        assert_clean(&v);
        assert_eq!(v.sets.len(), 3);
    }

    // ---- declarations and plumbing ----

    #[test]
    fn duplicate_declaration() {
        let v = run("metric weight kg");
        single_error(&v, "duplicate symbol `weight`");
    }

    #[test]
    fn parse_errors_are_carried_through() {
        let v = run("metrc steps steps");
        single_error(&v, "metrc");
    }
}
