use axum::{Router, routing::get};

mod assets;
mod repo;

pub fn build_router() -> Router {
    Router::new()
        .route("/", get(repo::list))
        .route("/:repo/", get(repo::index))
        .route("/:repo/commit/:commit_id/", get(repo::commit))
        .route("/:repo/commit/:commit_id/contents/*tree_path", get(repo::commit_tree))
        .route("/:repo/commit/:commit_id/diff", get(repo::commit_raw))
        .route("/assets/*path", get(assets::get))
}
