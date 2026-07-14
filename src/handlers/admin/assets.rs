//! GET /admin[/*path] — the embedded React dashboard.
//!
//! With the `ui` cargo feature the Vite build at `ui/dist/` is
//! embedded via rust-embed and served from memory (distroless-safe —
//! no filesystem assets). Unknown extension-less paths fall back to
//! index.html so client-side routing works.
//!
//! Without the feature (the default — `cargo test --all` must build
//! with no Node toolchain) the same routes serve a placeholder page,
//! keeping the route shape identical in both builds.

use axum::extract::Path;
#[cfg(feature = "ui")]
use axum::http::{StatusCode, header};
#[cfg(not(feature = "ui"))]
use axum::response::Html;
use axum::response::{IntoResponse, Response};

#[cfg(feature = "ui")]
#[derive(rust_embed::RustEmbed)]
#[folder = "ui/dist/"]
struct Assets;

#[cfg(not(feature = "ui"))]
const PLACEHOLDER: &str = r#"<!doctype html>
<html><head><title>mica admin</title></head><body style="font-family: system-ui; margin: 4rem auto; max-width: 40rem;">
<h1>mica admin</h1>
<p>This binary was built without the embedded dashboard.</p>
<p>Rebuild with <code>npm run build --prefix ui &amp;&amp; cargo build --features ui</code>,
or use the official Docker image, which includes it.</p>
<p>The admin API itself is live: <a href="/admin/api/state"><code>/admin/api/state</code></a></p>
</body></html>"#;

pub async fn index() -> Response {
    serve("index.html")
}

pub async fn asset(Path(path): Path<String>) -> Response {
    serve(path.trim_start_matches('/'))
}

#[cfg(feature = "ui")]
fn serve(path: &str) -> Response {
    let file = Assets::get(path).or_else(|| {
        // SPA fallback: only for navigations (no file extension), so a
        // missing hashed asset is a real 404 instead of index.html.
        if path.contains('.') {
            None
        } else {
            Assets::get("index.html")
        }
    });
    match file {
        Some(f) => {
            let mime = f.metadata.mimetype().to_string();
            ([(header::CONTENT_TYPE, mime)], f.data).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(not(feature = "ui"))]
fn serve(_path: &str) -> Response {
    Html(PLACEHOLDER).into_response()
}
