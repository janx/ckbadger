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

/// The directory Vite writes the build's content-hashed files into:
/// `build.assetsDir`, which `frontend/vite.config.ts` leaves at Vite's default.
/// The only namespace the build owns, so the only place a missing path is a
/// missing file rather than an SPA route (a unit test pins the config).
pub(crate) const VITE_ASSETS_DIR: &str = "assets/";

/// Where a request that matches no file goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MissingPath {
    /// A content-hashed build file that is not there (a stale chunk after a
    /// redeploy): 404, never the SPA shell served as JavaScript.
    BuildAsset,
    /// Anything else is a client-side route — including a deep link whose last
    /// segment has a dot, such as `/identities/dotcell/alice.cell`.
    SpaRoute,
}

/// Classify a path (without its leading `/`) that matched no file. Whether a
/// path IS a file is never guessed from its shape: the caller has already
/// looked it up.
pub(crate) fn classify_missing_path(path: &str) -> MissingPath {
    if path.starts_with(VITE_ASSETS_DIR) {
        MissingPath::BuildAsset
    } else {
        MissingPath::SpaRoute
    }
}

/// Cache policy for a served file: content-hashed build files are immutable,
/// everything else (HTML pages, root-level files) must revalidate.
pub(crate) fn cache_control_for(path: &str) -> &'static str {
    if path.starts_with(VITE_ASSETS_DIR) {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

/// Axum handler that serves embedded frontend assets.
///
/// Serving strategy:
/// 1. A path in the embedded asset set is served as itself.
/// 2. A missing path under the build's asset directory is a 404.
/// 3. Everything else falls back to `index.html` (SPA routing).
pub async fn embedded_frontend_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    if let Some(resp) = serve_embedded(path) {
        return resp;
    }

    if classify_missing_path(path) == MissingPath::SpaRoute {
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

    Some(
        (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, mime.as_ref()),
                (header::CACHE_CONTROL, cache_control_for(path)),
            ],
            asset.data,
        )
            .into_response(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The contract the removed `path_looks_like_file` heuristic was meant to
    /// serve, on its own inputs plus the dotted deep link it got wrong: only a
    /// path in the build's asset namespace may 404; every other missing path
    /// is a route.
    #[test]
    fn missing_paths_are_routes_except_in_the_build_asset_namespace() {
        assert_eq!(
            classify_missing_path("assets/app.js"),
            MissingPath::BuildAsset
        );
        assert_eq!(
            classify_missing_path("assets/app-stale.js"),
            MissingPath::BuildAsset
        );

        for route in [
            "",
            "blocks",
            "address/ckb1qz",
            "script/0x1234",
            "identities/.bit",
            "identities/did:ckb",
            "identities/dotbit",
            ".hidden",
            "identities/dotcell/alice.cell",
            "identities/dotcell/shop.alice.cell",
            "mainnet/identities/dotcell/alice.cell",
            "assets",
            "mainnet/assets/whatever.js",
            // A root-level file is served when it exists (the caller looked it
            // up first); when it does not, it is not a build asset.
            "favicon.ico",
            "images/logo.png",
        ] {
            assert_eq!(
                classify_missing_path(route),
                MissingPath::SpaRoute,
                "{route}"
            );
        }
    }

    #[test]
    fn only_build_assets_are_cached_immutably() {
        assert_eq!(
            cache_control_for("assets/app-abc.js"),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(cache_control_for("index.html"), "no-cache");
        assert_eq!(cache_control_for("favicon.ico"), "no-cache");
    }

    /// `VITE_ASSETS_DIR` is Vite's default `build.assetsDir`. The config sets
    /// none; if it ever does, this constant must follow it.
    #[test]
    fn vite_assets_dir_matches_the_build_config() {
        let config = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../frontend/vite.config.ts"),
        )
        .expect("frontend/vite.config.ts is readable");
        assert!(
            !config.contains("assetsDir"),
            "vite.config.ts overrides build.assetsDir; update VITE_ASSETS_DIR to match"
        );
        assert_eq!(VITE_ASSETS_DIR, "assets/");
    }
}
