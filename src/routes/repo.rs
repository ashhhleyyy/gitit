use std::path::PathBuf;

use axum::{extract::{Path, OriginalUri}, response::{Html, IntoResponse}, http::header, Extension};
use git2::{Repository, Sort, Tree, Blob};

use crate::{errors::{Result, GititError}, utils::{templates, ObjectId, HtmlOrRaw, safe_mime}, config::{Config, RepoConfig}};

fn repo_from_name<'config>(repo_name: &str, config: &'config Config) -> Result<(&'config RepoConfig, Repository)> {
    let repo_config = config.repos.get(repo_name).ok_or(GititError::NotFound)?;
    let mut path = PathBuf::new();
    path.push("repos");
    path.push(format!("{}.git", repo_name));
    let repo = Repository::open_bare(path)
        .map_err(|e| {
            match e.code() {
                git2::ErrorCode::NotFound => GititError::NotFound,
                _ => e.into(),
            }
        })?;
    Ok((repo_config, repo))
}

#[tracing::instrument]
pub(crate) async fn list(Extension(config): Extension<Config>) -> Result<Html<String>> {
    let template = liquid::ParserBuilder::with_stdlib()
        .build()?
        .parse(include_str!("templates/repo/list.html.liquid"))?;

    let mut repos = Vec::with_capacity(config.repos.len());
    for (slug, repo) in config.repos {
        repos.push(liquid::object!({
            "slug": slug,
            "title": repo.title,
            "upstream_url": repo.url,
        }));
    }

    Ok(Html(template.render(&liquid::object!({
        "repos": repos,
    }))?))
}

#[tracing::instrument]
pub(crate) async fn index(Path(repo_name): Path<String>, Extension(config): Extension<Config>) -> Result<Html<String>> {
    let template = liquid::ParserBuilder::with_stdlib()
        .build()?
        .parse(include_str!("templates/repo/index.html.liquid"))?;

    let (repo_config, repo) = repo_from_name(&repo_name, &config)?;
    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?;
    revwalk.set_sorting(Sort::TIME)?;

    let mut commits = Vec::with_capacity(100);
    for commit in revwalk.take(500) {
        let commit_id = commit?;
        let commit = repo.find_commit(commit_id)?;
        commits.push(templates::commit_to_object(&repo, &commit)?);
    }

    let head = repo.head()?.target().unwrap().to_string();

    let repo = liquid::object!({
        "name": repo_config.title,
        "recent_commits": commits,
        "head": head,
    });

    Ok(Html(template.render(&liquid::object!({
        "repo": repo,
    }))?))
}

#[tracing::instrument]
pub(crate) async fn commit(Path((repo_name, ObjectId(commit))): Path<(String, ObjectId)>, Extension(config): Extension<Config>) -> Result<impl IntoResponse> {
    let template = liquid::ParserBuilder::with_stdlib()
        .build()?
        .parse(include_str!("templates/repo/commit.html.liquid"))?;

    let (repo_config, repo) = repo_from_name(&repo_name, &config)?;
    let commit = repo.find_commit(commit)?;
    let repo_data = liquid::object!({
        "name": repo_config.title,
    });
    Ok(Html(template.render(&liquid::object!({
        "repo": repo_data,
        "commit": templates::commit_to_object(&repo, &commit)?,
        "diff": templates::full_diff(&repo, &commit, false)?,
    }))?))
}

#[tracing::instrument]
pub(crate) async fn commit_raw(Path((repo_name, ObjectId(commit))): Path<(String, ObjectId)>, Extension(config): Extension<Config>) -> Result<impl IntoResponse> {
    let (_, repo) = repo_from_name(&repo_name, &config)?;
    let commit = repo.find_commit(commit)?;
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], templates::full_diff(&repo, &commit, true)?))
}

#[tracing::instrument]
pub(crate) async fn commit_tree(Path((repo_name, ObjectId(commit), path)): Path<(String, ObjectId, String)>, OriginalUri(full_uri): OriginalUri, Extension(config): Extension<Config>) -> Result<HtmlOrRaw> {
    let (_, repo) = repo_from_name(&repo_name, &config)?;
    let commit = repo.find_commit(commit)?;
    let tree = commit.tree()?;

    if path.len() <= 1 {
        return render_tree(&commit.id().to_string(), path, &tree);
    };

    let subtree = tree.get_path(&std::path::Path::new(&path[1..]))?;

    match subtree.kind().unwrap() {
        git2::ObjectType::Tree => {
            if !path.ends_with("/") {
                let path_and_query = full_uri.path_and_query().unwrap();
                let target = if let Some(query) = path_and_query.query() {
                    format!("{}/?{}", path_and_query.path(), query)
                } else {
                    format!("{}/", path_and_query.path())
                };
                return Err(GititError::Redirect(target));
            }

            if let Some(subtree) = subtree.to_object(&repo)?.as_tree() {
               render_tree(&commit.id().to_string(), path, subtree)
            } else {
                Err(GititError::NotFound)
            }
        },
        git2::ObjectType::Blob => {
            if let Some(blob) = subtree.to_object(&repo)?.as_blob() {
                render_file(&commit.id().to_string(), path, &blob)
            } else {
                Err(GititError::NotFound)
            }
        },
        _ => Err(GititError::NotFound)
    }
}

fn render_file(commit: &str, path: String, blob: &Blob) -> Result<HtmlOrRaw> {
    let template = liquid::ParserBuilder::with_stdlib()
        .build()?
        .parse(include_str!("templates/repo/text_file.html.liquid"))?;

    if blob.is_binary() {
        Ok(HtmlOrRaw::Raw(safe_mime(mime_guess::from_path(path).first_or_octet_stream()).to_string(), blob.content().to_owned()))
    } else {
        let string_content = std::str::from_utf8(blob.content()).unwrap().to_owned();
        let extension = std::path::Path::new(&path).extension().map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "txt".to_owned());
        Ok(HtmlOrRaw::Html(template.render(&liquid::object!({
            "commit": {
                "hash": commit,
            },
            "file": {
                "path": path,
                "content": templates::syntax_highlight(&extension, &string_content)?,
            },
        }))?))
    }
}

#[tracing::instrument]
fn render_tree(commit: &str, path: String, subtree: &Tree<'_>) -> Result<HtmlOrRaw> {
    let template = liquid::ParserBuilder::with_stdlib()
        .build()?
        .parse(include_str!("templates/repo/commit_tree.html.liquid"))?;
    let mut files = vec![];
    for file in subtree.iter() {
        files.push(liquid::object!({
            "filename": file.name(),
            "kind": match file.kind().unwrap() {
                git2::ObjectType::Tree => "tree",
                git2::ObjectType::Blob => "blob",
                _ => {
                    tracing::warn!("strange kind in tree");
                    continue
                }
            },
        }));
    }
    Ok(HtmlOrRaw::Html(template.render(&liquid::object!({
        "commit": {
            "hash": commit,
        },
        "path": path,
        "files": files,
    }))?))
}
