use axum::{Router, http::header, routing::get};

const INDEX_HTML: &str = include_str!("../static/index.html");
const APP_JS: &str = include_str!("../static/app.js");
const STYLE_CSS: &str = include_str!("../static/style.css");
const INTER: &[u8] = include_bytes!("../static/fonts/InterVariable.woff2");
const INTER_ITALIC: &[u8] = include_bytes!("../static/fonts/InterVariable-Italic.woff2");

#[tokio::main]
async fn main() {
    // build our application with a single route
    let app = Router::new()
        .route(
            "/",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                    INDEX_HTML,
                )
            }),
        )
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
        );

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
