use rusqlite::{Connection, params, types::Value};

use crate::validator::Validated;

const SCHEMA: &str = "
CREATE TABLE events (
    id          INTEGER PRIMARY KEY,
    date        TEXT NOT NULL,      -- 'YYYY-MM-DD'
    time        TEXT,               -- 'HH:MM'
    activity    TEXT,
    description TEXT
) STRICT;

CREATE TABLE event_tags (
    event_id INTEGER NOT NULL REFERENCES events(id),
    tag      TEXT NOT NULL
) STRICT;

CREATE TABLE event_metadata (
    event_id INTEGER NOT NULL REFERENCES events(id),
    key      TEXT NOT NULL,
    value    TEXT NOT NULL
) STRICT;

CREATE TABLE observations (
    event_id INTEGER NOT NULL REFERENCES events(id),
    metric   TEXT NOT NULL,
    value    REAL NOT NULL,
    unit     TEXT NOT NULL
) STRICT;

CREATE TABLE sets (
    event_id      INTEGER NOT NULL REFERENCES events(id),
    exercise      TEXT NOT NULL,
    set_number    INTEGER NOT NULL,
    load          REAL,
    load_unit     TEXT,
    reps          REAL,
    duration      REAL,
    duration_unit TEXT,
    distance      REAL,
    distance_unit TEXT
) STRICT;
";

/// Creates an ISO8601 date string from the date tuple used by our data model
fn fmt_date((y, m, d): (u16, u8, u8)) -> String {
    format!("{y:04}-{m:02}-{d:02}")
}

/// Creates a time string compatible with SQLite from the data tuple used by
/// our data model
fn fmt_time((h, m): (u8, u8)) -> String {
    format!("{h:02}:{m:02}")
}

/// Creates an in-memory database and populates it with the validated/parsed
/// data from fitlog files
pub fn build_database(v: &Validated) -> rusqlite::Result<Connection> {
    let mut conn = Connection::open_in_memory()?;

    conn.execute_batch(SCHEMA)?;

    let tx = conn.transaction()?;
    {
        let mut events = tx.prepare(
            "INSERT INTO events (id, date, time, activity, description) VALUES (?1, ?2, ?3, ?4, ?5)"
        )?;
        let mut tags = tx.prepare("INSERT INTO event_tags (event_id, tag) VALUES (?1, ?2)")?;
        let mut metadata =
            tx.prepare("INSERT INTO event_metadata (event_id, key, value) VALUES (?1, ?2, ?3)")?;
        for e in &v.events {
            let id = e.id.get();
            events.execute(params![
                id,
                fmt_date(e.date),
                e.time.map(fmt_time),
                e.activity,
                e.description
            ])?;
            for t in &e.tags {
                tags.execute(params![id, t])?;
            }
            for (k, val) in &e.metadata {
                metadata.execute(params![id, k, val])?;
            }
        }

        let mut obs = tx.prepare(
            "INSERT INTO observations (event_id, metric, value, unit) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for o in &v.observations {
            obs.execute(params![o.event_id.get(), o.metric, o.value, o.unit])?;
        }

        let mut sets = tx.prepare(
            "INSERT INTO sets (event_id, exercise, set_number, load, load_unit, reps,
                               duration, duration_unit, distance, distance_unit)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        fn split(slot: &Option<(f64, String)>) -> (Option<f64>, Option<&str>) {
            (
                slot.as_ref().map(|(n, _)| *n),
                slot.as_ref().map(|(_, u)| u.as_str()),
            )
        }
        for s in &v.sets {
            let (load, load_unit) = split(&s.load);
            let (duration, duration_unit) = split(&s.duration);
            let (distance, distance_unit) = split(&s.distance);
            sets.execute(params![
                s.event_id.get(),
                s.exercise,
                s.set_number,
                load,
                load_unit,
                s.reps,
                duration,
                duration_unit,
                distance,
                distance_unit
            ])?;
        }
    }
    tx.commit()?;

    Ok(conn)
}

/// Stores results of an arbitrary SQL query
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

/// Runs an SQL query against the given database connection and returns the result
pub fn run_query(conn: &Connection, sql: &str) -> rusqlite::Result<QueryResult> {
    let mut statement = conn.prepare(sql)?;
    let columns: Vec<String> = statement
        .column_names()
        .iter()
        .map(|s| s.to_string())
        .collect();
    let ncols = columns.len();

    let rows = statement
        .query_map([], |row| {
            (0..ncols).map(|i| row.get::<_, Value>(i)).collect()
        })?
        .collect::<rusqlite::Result<Vec<Vec<Value>>>>()?;

    Ok(QueryResult { columns, rows })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembler::{MapSourceTextProvider, Severity, assemble_with};
    use crate::validator::validate;
    use rusqlite::types::FromSql;
    use std::path::Path;

    const DECLS: &str = "\
metric weight lb
metric bp_sys mmHg
metric bp_dia mmHg
metric bp = bp_sys / bp_dia
activity lift
activity hike
exercise bench_press load reps
";

    /// Runs `body` (plus DECLS) through the whole pipeline and builds the database.
    fn db(body: &str) -> Connection {
        let main = format!("{DECLS}{body}\n");
        let provider = MapSourceTextProvider::new(&[("main.fitlog", &main)]);
        let asm = assemble_with(Path::new("main.fitlog"), &provider).expect("entrypoint readable");
        let v = validate(asm);
        let errs: Vec<_> = v
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, Severity::Error))
            .collect();
        assert!(errs.is_empty(), "fixture has errors: {errs:#?}");
        build_database(&v).expect("database builds")
    }

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    /// Collects the first column of every row a query returns.
    fn column<T: FromSql>(conn: &Connection, sql: &str) -> Vec<T> {
        let mut stmt = conn.prepare(sql).unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn empty_input_creates_all_tables() {
        let conn = db("");
        for table in [
            "events",
            "event_tags",
            "event_metadata",
            "observations",
            "sets",
        ] {
            assert_eq!(count(&conn, table), 0, "table {table}");
        }
    }

    #[test]
    fn event_formats_date_and_time() {
        let conn = db("2026-08-05 08:12 weight 178.4 lb");
        let row: (String, Option<String>, Option<String>) = conn
            .query_row("SELECT date, time, activity FROM events", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!(row, ("2026-08-05".into(), Some("08:12".into()), None));
    }

    #[test]
    fn untimed_event_has_null_time() {
        let conn = db("2026-08-05 weight 178.4");
        assert_eq!(
            column::<Option<String>>(&conn, "SELECT time FROM events"),
            [None]
        );
    }

    #[test]
    fn observation_row() {
        let conn = db("2026-08-05 weight 178.4");
        let row: (String, f64, String) = conn
            .query_row("SELECT metric, value, unit FROM observations", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!(row, ("weight".into(), 178.4, "lb".into()));
    }

    #[test]
    fn alias_produces_one_row_per_component() {
        let conn = db("2026-08-05 bp 118/76");
        let metrics: Vec<String> = column(&conn, "SELECT metric FROM observations ORDER BY metric");
        assert_eq!(metrics, ["bp_dia", "bp_sys"]);
    }

    #[test]
    fn activity_with_description_tags_and_metadata() {
        let conn = db(r#"2026-08-05 hike "Cougar Mountain" #pnw #summer
  note: "muddy""#);
        let row: (Option<String>, Option<String>) = conn
            .query_row("SELECT activity, description FROM events", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(row, (Some("hike".into()), Some("Cougar Mountain".into())));

        let tags: Vec<String> = column(&conn, "SELECT tag FROM event_tags ORDER BY tag");
        assert_eq!(tags, ["pnw", "summer"]);

        let meta: (String, String) = conn
            .query_row("SELECT key, value FROM event_metadata", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(meta, ("note".into(), "muddy".into()));
    }

    #[test]
    fn sets_store_values_units_and_nulls() {
        let conn = db("2026-08-05 lift\n  bench_press 185 lb 5/5/4");
        assert_eq!(count(&conn, "sets"), 3);
        type SetRow = (
            i64,
            Option<f64>,
            Option<String>,
            Option<f64>,
            Option<f64>,
            Option<String>,
        );
        let row: SetRow = conn
            .query_row(
                "SELECT set_number, load, load_unit, reps, duration, distance_unit
                 FROM sets WHERE set_number = 3",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            row,
            (3, Some(185.0), Some("lb".into()), Some(4.0), None, None)
        );
    }

    #[test]
    fn every_child_row_references_an_event() {
        let conn = db(r#"2026-08-05 weight 178.4
2026-08-06 lift #upper
  bench_press 185 lb 5
  note: "felt good""#);
        for table in ["observations", "sets", "event_tags", "event_metadata"] {
            let orphans: i64 = conn
                .query_row(
                    &format!(
                        "SELECT count(*) FROM {table} WHERE event_id NOT IN (SELECT id FROM events)"
                    ),
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(orphans, 0, "orphaned rows in {table}");
        }
    }

    #[test]
    fn run_query_returns_columns_and_typed_values() {
        let conn = db("2026-08-05 weight 178.4\n2026-08-06 weight 177.9");
        let r = run_query(
            &conn,
            "SELECT metric, value FROM observations ORDER BY value",
        )
        .unwrap();
        assert_eq!(r.columns, ["metric", "value"]);
        assert_eq!(
            r.rows,
            [
                vec![Value::Text("weight".into()), Value::Real(177.9)],
                vec![Value::Text("weight".into()), Value::Real(178.4)],
            ]
        );
    }
}
