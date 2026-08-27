//! Embedded frontend assets compiled into the binary from `frontend/dist/`.
//!
//! When the `frontend/dist/` directory exists at compile time, its contents
//! are embedded into the binary. At runtime the embedded handler serves
//! these assets with correct content-types and caching headers.
//!
//! If the directory does not exist at compile time (e.g. during `cargo check`),
//! `#[allow_missing = true]` lets compilation succeed with zero embedded assets.

use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

/// Embedded frontend assets from `frontend/dist/`.
///
/// `allow_missing = true` means the folder can be absent during development
/// builds — the binary will simply have no embedded assets.
#[derive(Embed)]
#[folder = "../../frontend/dist/"]
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
/// 2. SPA fallback: non-file paths fall back to `index.html`
///
/// Cache headers:
/// - `assets/` files → immutable, 1 year (content-hashed)
/// - Everything else → no-cache (HTML pages, etc.)
pub async fn embedded_frontend_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // 1. Exact match
    if let Some(resp) = serve_embedded(path) {
        return resp;
    }

    // 2. SPA fallback for non-file paths
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

    let cache_control = if path.starts_with("assets/") {
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
/// Dot-prefixed segments like `.bit` are SPA route params, not files.
fn path_looks_like_file(path: &str) -> bool {
    let segment = match path.rsplit_once('/') {
        Some((_, s)) => s,
        None => path,
    };
    segment.contains('.') && !segment.starts_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_looks_like_file() {
        assert!(path_looks_like_file("favicon.ico"));
        assert!(path_looks_like_file("assets/app.js"));
        assert!(path_looks_like_file("images/logo.png"));

        assert!(!path_looks_like_file(""));
        assert!(!path_looks_like_file("blocks"));
        assert!(!path_looks_like_file("address/ckb1qz"));
        // Dot-prefixed segments are SPA route params, not files
        assert!(!path_looks_like_file("identities/.bit"));
        assert!(!path_looks_like_file(".hidden"));
    }
}
