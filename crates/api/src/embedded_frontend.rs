//! Embedded frontend assets compiled into the binary from `frontend/out/`.
//!
//! When the `frontend/out/` directory exists at compile time, its contents
//! are embedded into the binary. At runtime the embedded handler serves
//! these assets with correct content-types and caching headers.
//!
//! If the directory does not exist at compile time (e.g. during `cargo check`),
//! `#[allow_missing = true]` lets compilation succeed with zero embedded assets.

use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

/// Embedded frontend assets from `frontend/out/`.
///
/// `allow_missing = true` means the folder can be absent during development
/// builds — the binary will simply have no embedded assets.
#[derive(Embed)]
#[folder = "../../frontend/out/"]
#[allow_missing = true]
struct FrontendAssets;

/// Returns `true` if any frontend assets were embedded at compile time.
pub fn has_embedded_assets() -> bool {
    FrontendAssets::iter().next().is_some()
}

/// Axum handler that serves embedded frontend assets.
///
/// Serving strategy:
/// 1. Exact path match (e.g. `/favicon.ico` → `favicon.ico`)
/// 2. Path with trailing slash → try `{path}/index.html`
/// 3. Path without trailing slash → try `{path}/index.html` (Next.js static export)
/// 4. SPA fallback: non-file paths fall back to `index.html`
///
/// Cache headers:
/// - `_next/static/` files → immutable, 1 year (content-hashed)
/// - Everything else → no-cache (HTML pages, etc.)
pub async fn embedded_frontend_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // 1. Exact match
    if let Some(resp) = serve_embedded(path) {
        return resp;
    }

    // 2 & 3. Try {path}/index.html (handles both `/blocks/` and `/blocks`)
    let stripped = path.trim_end_matches('/');
    if !stripped.is_empty() {
        let index_path = format!("{}/index.html", stripped);
        if let Some(resp) = serve_embedded(&index_path) {
            return resp;
        }
    }

    // 4. SPA fallback for non-file paths
    if !path_looks_like_file(path) {
        if let Some(resp) = serve_embedded("index.html") {
            return resp;
        }
    }

    (StatusCode::NOT_FOUND, "not found").into_response()
}

/// Serve a single embedded asset by path, returning `None` if not found.
fn serve_embedded(path: &str) -> Option<Response> {
    let asset = FrontendAssets::get(path)?;

    let mime = mime_guess::from_path(path).first_or_octet_stream();

    let cache_control = if path.starts_with("_next/static/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    Some(
        (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, mime.as_ref()),
                (header::CACHE_CONTROL, cache_control),
            ],
            asset.data,
        )
            .into_response(),
    )
}

/// Returns `true` if the path looks like a file request (has an extension).
fn path_looks_like_file(path: &str) -> bool {
    match path.rsplit_once('/') {
        Some((_, segment)) => segment.contains('.'),
        None => path.contains('.'),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_looks_like_file() {
        assert!(path_looks_like_file("favicon.ico"));
        assert!(path_looks_like_file("_next/static/abc.js"));
        assert!(path_looks_like_file("images/logo.png"));

        assert!(!path_looks_like_file(""));
        assert!(!path_looks_like_file("blocks"));
        assert!(!path_looks_like_file("blocks/"));
        assert!(!path_looks_like_file("address/ckb1qz"));
    }

    #[test]
    fn test_has_embedded_assets_without_frontend_build() {
        // In test builds, frontend/out/ typically doesn't exist,
        // so we expect no embedded assets.
        // This test just verifies the function doesn't panic.
        let _has = has_embedded_assets();
    }
}
