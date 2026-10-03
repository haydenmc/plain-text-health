use chrono::NaiveDate;
use datafusion::arrow::{
    array::{
        ArrayRef, Date32Array, Float64Array, RecordBatch, StringArray, Time32SecondArray,
        UInt32Array,
    },
    datatypes::{DataType, Date32Type, Field, Schema, TimeUnit},
    error::ArrowError,
};
use std::sync::Arc;

use crate::validator::{Event, Observation, Set, Validated};

/// Constructs a set of data tables from validated fitlog data
pub fn build_tables(v: &Validated) -> Result<Vec<(&'static str, RecordBatch)>, ArrowError> {
    Ok(vec![
        ("events", events_batch(&v.events)?),
        ("event_tags", event_tags_batch(&v.events)?),
        ("event_metadata", event_metadata_batch(&v.events)?),
        ("observations", observations_batch(&v.observations)?),
        ("sets", sets_batch(&v.sets)?),
    ])
}

fn events_batch(events: &[Event]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::UInt32, false),
        Field::new("date", DataType::Date32, false),
        Field::new("time", DataType::Time32(TimeUnit::Second), true), // nullable, time may not be defined
        Field::new("activity", DataType::Utf8, true), // nullable, activity may be empty
        Field::new("description", DataType::Utf8, true), // nullable, description may be empty
    ]));

    let to_date32 = |d: (u16, u8, u8)| -> i32 {
        Date32Type::from_naive_date(
            NaiveDate::from_ymd_opt(d.0 as i32, d.1 as u32, d.2 as u32)
                .expect("validator guarantees valid dates"),
        )
    };
    let to_time32 = |t: (u8, u8)| -> i32 { t.0 as i32 * 3600 + t.1 as i32 * 60 };

    let columns: Vec<ArrayRef> = vec![
        Arc::new(UInt32Array::from_iter_values(
            events.iter().map(|e| e.id.get()),
        )),
        Arc::new(Date32Array::from_iter_values(
            events.iter().map(|e| to_date32(e.date)),
        )),
        Arc::new(Time32SecondArray::from_iter(events.iter().map(
            |e| match e.time {
                Some(t) => Some(to_time32(t)),
                None => None,
            },
        ))),
        Arc::new(StringArray::from_iter(
            events.iter().map(|e| e.activity.as_deref()),
        )),
        Arc::new(StringArray::from_iter(
            events.iter().map(|e| e.description.as_deref()),
        )),
    ];

    RecordBatch::try_new(schema, columns)
}

fn event_tags_batch(events: &[Event]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("event_id", DataType::UInt32, false),
        Field::new("tag", DataType::Utf8, false),
    ]));
    let rows: Vec<(u32, &str)> = events
        .iter()
        .flat_map(|e| e.tags.iter().map(move |t| (e.id.get(), t.as_str())))
        .collect();
    let columns: Vec<ArrayRef> = vec![
        Arc::new(UInt32Array::from_iter_values(
            rows.iter().map(|(id, _)| *id),
        )),
        Arc::new(StringArray::from_iter_values(rows.iter().map(|(_, t)| *t))),
    ];
    RecordBatch::try_new(schema, columns)
}

fn event_metadata_batch(events: &[Event]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("event_id", DataType::UInt32, false),
        Field::new("key", DataType::Utf8, false),
        Field::new("value", DataType::Utf8, false),
    ]));
    let rows: Vec<(u32, &str, &str)> = events
        .iter()
        .flat_map(|e| {
            e.metadata
                .iter()
                .map(move |(k, v)| (e.id.get(), k.as_str(), v.as_str()))
        })
        .collect();
    let columns: Vec<ArrayRef> = vec![
        Arc::new(UInt32Array::from_iter_values(
            rows.iter().map(|(id, _, _)| *id),
        )),
        Arc::new(StringArray::from_iter_values(
            rows.iter().map(|(_, key, _)| *key),
        )),
        Arc::new(StringArray::from_iter_values(
            rows.iter().map(|(_, _, val)| *val),
        )),
    ];
    RecordBatch::try_new(schema, columns)
}

fn observations_batch(obs: &[Observation]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("event_id", DataType::UInt32, false),
        Field::new("metric", DataType::Utf8, false),
        Field::new("value", DataType::Float64, false),
        Field::new("unit", DataType::Utf8, false),
    ]));
    let columns: Vec<ArrayRef> = vec![
        Arc::new(UInt32Array::from_iter_values(
            obs.iter().map(|o| o.event_id.get()),
        )),
        Arc::new(StringArray::from_iter_values(
            obs.iter().map(|o| o.metric.as_str()),
        )),
        Arc::new(Float64Array::from_iter_values(obs.iter().map(|o| o.value))),
        Arc::new(StringArray::from_iter_values(
            obs.iter().map(|o| o.unit.as_str()),
        )),
    ];
    RecordBatch::try_new(schema, columns)
}

fn sets_batch(sets: &[Set]) -> Result<RecordBatch, ArrowError> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("event_id", DataType::UInt32, false),
        Field::new("exercise", DataType::Utf8, false),
        Field::new("set_number", DataType::UInt32, false),
        Field::new("load", DataType::Float64, true),
        Field::new("load_unit", DataType::Utf8, true),
        Field::new("reps", DataType::Float64, true),
        Field::new("duration", DataType::Float64, true),
        Field::new("duration_unit", DataType::Utf8, true),
        Field::new("distance", DataType::Float64, true),
        Field::new("distance_unit", DataType::Utf8, true),
    ]));
    let columns: Vec<ArrayRef> = vec![
        Arc::new(UInt32Array::from_iter_values(
            sets.iter().map(|s| s.event_id.get()),
        )),
        Arc::new(StringArray::from_iter_values(
            sets.iter().map(|s| s.exercise.as_str()),
        )),
        Arc::new(UInt32Array::from_iter_values(
            sets.iter().map(|s| s.set_number),
        )),
        Arc::new(Float64Array::from_iter(
            sets.iter().map(|s| s.load.as_ref().map(|(n, _)| *n)),
        )),
        Arc::new(StringArray::from_iter(
            sets.iter()
                .map(|s| s.load.as_ref().map(|(_, u)| u.as_str())),
        )),
        Arc::new(Float64Array::from_iter(
            sets.iter().map(|s| s.reps.as_ref().map(|n| *n)),
        )),
        Arc::new(Float64Array::from_iter(
            sets.iter().map(|s| s.duration.as_ref().map(|(n, _)| *n)),
        )),
        Arc::new(StringArray::from_iter(
            sets.iter()
                .map(|s| s.duration.as_ref().map(|(_, u)| u.as_str())),
        )),
        Arc::new(Float64Array::from_iter(
            sets.iter().map(|s| s.distance.as_ref().map(|(n, _)| *n)),
        )),
        Arc::new(StringArray::from_iter(
            sets.iter()
                .map(|s| s.distance.as_ref().map(|(_, u)| u.as_str())),
        )),
    ];
    RecordBatch::try_new(schema, columns)
}
