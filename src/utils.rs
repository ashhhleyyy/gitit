use axum::{response::{IntoResponse, Html}, http::header};
use git2::Oid;
use serde::{Deserializer, de::Visitor};

pub enum HtmlOrRaw {
    String(String),
    Html(String),
    Raw(String, Vec<u8>),
}

impl IntoResponse for HtmlOrRaw {
    fn into_response(self) -> axum::response::Response {
        match self {
            HtmlOrRaw::String(s) => s.into_response(),
            HtmlOrRaw::Html(s) => Html(s).into_response(),
            HtmlOrRaw::Raw(content_type, data) => ([(header::CONTENT_TYPE, content_type)], data).into_response(),
        }
    }
}

pub fn safe_mime(mime: mime_guess::Mime) -> mime_guess::Mime {
    if mime.essence_str().starts_with("application/") {
        return mime::APPLICATION_OCTET_STREAM;
    } else {
        return mime;
    }
}

#[derive(serde::Deserialize)]
pub struct ObjectId(#[serde(deserialize_with = "deserialize_oid")] pub Oid);

// deserialize_with
fn deserialize_oid<'de, D>(deserializer: D) -> Result<Oid, D::Error> where D: Deserializer<'de> {
    deserializer.deserialize_str(OidVisitor)
}

struct OidVisitor;

impl<'de> Visitor<'de> for OidVisitor {
    type Value = Oid;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a Git OID")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> where E: serde::de::Error {
        Oid::from_str(v).map_err(|_| serde::de::Error::custom(format!("invalid OID: {}", v)))
    }
}

pub mod templates {
    use git2::{Commit, Repository, DiffFormat, DiffOptions, Diff};
    use syntect::{parsing::SyntaxSet, highlighting::ThemeSet};
    use std::fmt::Write;

    use crate::errors::Result;

    #[tracing::instrument]
    pub fn syntax_highlight(extension: &str, code: &str) -> Result<String> {
        let ss = SyntaxSet::load_defaults_newlines();
        let ts = ThemeSet::load_defaults();
        let theme = &ts.themes["base16-ocean.dark"];
        let syntax = ss.find_syntax_by_extension(extension).unwrap_or(ss.find_syntax_plain_text());
        let html = syntect::html::highlighted_html_for_string(code, &ss, syntax, theme)?;

        Ok(html)
    }

    pub fn commit_to_object(repo: &Repository, commit: &Commit) -> Result<liquid::Object> {
        let hash = commit.id().to_string();
        let short_hash = hash[..7].to_owned();
        let author_name = commit.author().name().map(|s| s.to_owned());
        let author_email = commit.author().email().map(|s| s.to_owned());
        
        let (diff_line, (added, removed)) = diff_info(repo, commit)?;

        let (summary, description) = commit.message().unwrap().split_once('\n')
            .unwrap_or_else(|| (commit.message().unwrap(), ""));

        Ok(liquid::object!({
            "hash": hash,
            "short_hash": short_hash,
            "summary": summary,
            "message": description,
            "author": {
                "name": author_name,
                "email": author_email,
            },
            "diff": {
                "added": added,
                "removed": removed,
                "summary": diff_line,
            },
        }))
    }

    pub fn full_diff(repo: &Repository, commit: &Commit, raw: bool) -> Result<String> {
        let diff = makediff(repo, commit)?;
        let mut output = String::new();
        diff.print(DiffFormat::Patch, |_delta, _hunk, line| {
            let c = match line.origin() {
                '+' | '-' => {
                    line.origin()
                },
                _ => ' ',
            };
            let line_str = std::str::from_utf8(line.content()).unwrap();
            output.push_str(&format_line(line_str, c, raw));
            true
        })?;
        if raw {
            Ok(output)
        } else {
            syntax_highlight("patch", &output)
        }
    }

    fn format_line(line: &str, c: char, raw: bool) -> String {
        if raw {
            line.to_owned()
        } else {
            format!("{} {}", c, line)
        }
    }

    fn diff_info(repo: &Repository, commit: &Commit) -> Result<(String, (i32, i32))> {
        let diff = makediff(repo, commit)?;
        let mut output = String::new();
        let mut added = 0;
        let mut removed = 0;
        diff.print(DiffFormat::Patch, |_delta, _hunk, line| {
            match line.origin() {
                ' ' | '+' | '-' => {
                    write!(&mut output, "{}", line.origin()).unwrap()
                },
                _ => {}
            }
            match line.origin() {
                '-' => { removed += 1; }
                '+' => { added += 1; }
                _ => { }
            }
            true
        })?;
        Ok((output, (added, removed)))
    }

    fn makediff<'repo>(repo: &'repo Repository, commit: &Commit) -> Result<Diff<'repo>> {
        let mut diffopts = DiffOptions::new();
        let a = if commit.parents().len() == 1 {
            let parent = commit.parent(0)?;
            Some(parent.tree()?)
        } else {
            None
        };
        let b = commit.tree()?;
        Ok(repo.diff_tree_to_tree(a.as_ref(), Some(&b), Some(&mut diffopts))?)
    }
}
