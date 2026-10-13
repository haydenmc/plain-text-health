use std::{
    io::{self},
    path::PathBuf,
    process::ExitCode,
    sync::{Arc, Mutex},
};

use askama::Template;
use axum::{
    Router,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use clap::Parser;
use pth_core::QueryResult;
use rusqlite::Connection;
use serde::Deserialize;

const APP_JS: &str = include_str!("../static/app.js");
const STYLE_CSS: &str = include_str!("../static/style.css");
const INTER: &[u8] = include_bytes!("../static/fonts/InterVariable.woff2");
const INTER_ITALIC: &[u8] = include_bytes!("../static/fonts/InterVariable-Italic.woff2");

// built-in dashboards
const BUILTIN_SOURCES: &[(&str, &str)] = &[
    ("recent", include_str!("../builtin/recent.toml")),
    ("metrics", include_str!("../builtin/metrics.toml")),
    ("activities", include_str!("../builtin/activities.toml")),
    ("data", include_str!("../builtin/data.toml")),
];

#[derive(Parser, Debug)]
#[command(name = "pth-dashboard", version)]
struct Args {
    #[arg(short = 'f', long = "fitlog-file")]
    fitlog_file: PathBuf,

    #[arg(short = 'd', long = "dashboard", num_args = 1..)]
    dashboard_files: Vec<PathBuf>,
}

pub struct AppState {
    pub dashboards: Vec<Dashboard>,
    pub db: Mutex<Connection>,
}

#[derive(Deserialize)]
pub struct Dashboard {
    pub title: String,
    #[serde(default)]
    pub slug: String,
    #[serde(rename = "panel")]
    pub panels: Vec<Panel>,
}

#[derive(Deserialize)]
pub struct Panel {
    pub title: String,
    pub sql: String,
    // pub chart: Option<Chart>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Chart {
    Line { x: String, y: OneOrMany },
    Bar { x: String, y: String },
    Table,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

pub struct NavLink {
    pub slug: String,
    pub title: String,
}

pub struct Nav {
    pub dashboards: Vec<NavLink>,
    pub current: String,
}

#[derive(Template)]
#[template(path = "dashboard.html")]
pub struct DashboardPage {
    pub nav: Nav,
    pub title: String,
    pub panels: Vec<PanelView>,
}

pub struct PanelView {
    pub title: String,
    //pub chart_spec: Option<String>, // Vega-Lite spec, as JSON text
    pub table: Option<QueryResult>,
    pub error: Option<String>,
}

fn load_dashboard(dashboard_file: &PathBuf) -> std::io::Result<Dashboard> {
    let content = std::fs::read_to_string(dashboard_file)?;
    let mut dashboard: Dashboard =
        toml::from_str(&content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if dashboard.slug.is_empty() {
        dashboard.slug = dashboard_file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
    Ok(dashboard)
}

fn build_panel_view(conn: &Connection, panel: &Panel) -> PanelView {
    match pth_core::run_query(conn, &panel.sql) {
        Ok(table) => PanelView {
            title: panel.title.clone(),
            table: Some(table),
            error: None,
        },
        Err(e) => PanelView {
            title: panel.title.clone(),
            table: None,
            error: Some(e.to_string()),
        },
    }
}

fn build_nav(dashboards: &Vec<Dashboard>, slug: &String) -> Nav {
    Nav {
        dashboards: dashboards
            .into_iter()
            .map(|d| NavLink {
                title: d.title.clone(),
                slug: d.slug.clone(),
            })
            .collect(),
        current: slug.clone(),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();

    // load + parse dashboard files
    let mut dashboards = match args
        .dashboard_files
        .iter()
        .map(|p| load_dashboard(p))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(d) => d,
        Err(e) => {
            eprintln!("pth-dashboard: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Add built-in dashboards
    for (name, text) in BUILTIN_SOURCES {
        let mut dash: Dashboard =
            toml::from_str(text).expect("built-in dashboards are tested to parse");
        dash.slug = name.to_string();
        dashboards.push(dash);
    }

    // load fitlog files and spin up database
    let validated = match pth_core::load(&args.fitlog_file) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "pth-dashboard: cannot read `{}`: {e}",
                args.fitlog_file.display()
            );
            return ExitCode::FAILURE;
        }
    };
    let conn = match pth_core::build_database(&validated) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("pth-dashboard: could not build database: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = conn.pragma_update(None, "query_only", true) {
        eprintln!("pth-dashboard: could not set query_only on database: {e}");
    }

    let state = Arc::new(AppState {
        dashboards,
        db: Mutex::new(conn),
    });

    let app = Router::new()
        .route("/dashboards/{slug}", get(dashboard_page))
        // Static assets:
        .route(
            "/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    APP_JS,
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    STYLE_CSS,
                )
            }),
        )
        .route(
            "/fonts/InterVariable.woff2",
            get(|| async {
                (
                    [
                        (header::CONTENT_TYPE, "font/woff2"),
                        (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
                    ],
                    INTER,
                )
            }),
        )
        .route(
            "/fonts/InterVariable-Italic.woff2",
            get(|| async {
                (
                    [
                        (header::CONTENT_TYPE, "font/woff2"),
                        (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
                    ],
                    INTER_ITALIC,
                )
            }),
        )
        .with_state(state);

    let listener = match tokio::net::TcpListener::bind("127.0.0.1:8080").await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("pth-dashboard: cannot listen on 127.0.0.1:8080: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("pth-dashboard: server error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

async fn dashboard_page(State(state): State<Arc<AppState>>, Path(slug): Path<String>) -> Response {
    let Some(dash) = state.dashboards.iter().find(|d| d.slug == slug) else {
        return (
            StatusCode::NOT_FOUND,
            format!("no dashboard named `{slug}`"),
        )
            .into_response();
    };

    let panels = {
        let conn = state.db.lock().expect("database lock poisoned");
        dash.panels
            .iter()
            .map(|p| build_panel_view(&conn, p))
            .collect()
    };

    let page = DashboardPage {
        nav: build_nav(&state.dashboards, &slug),
        title: dash.title.clone(),
        panels,
    };

    match page.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("template error: {e}"),
        )
            .into_response(),
    }
}
