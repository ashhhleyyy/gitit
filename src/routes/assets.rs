use axum::{headers::{ETag, IfNoneMatch, HeaderMapExt}, http::{HeaderValue, StatusCode, header::CONTENT_TYPE}, TypedHeader, extract::Path, response::IntoResponse};
use hex::ToHex;
use rust_embed::{EmbeddedFile, RustEmbed};


// Yes I've shamelessly stolen this from the code from my own website.
#[derive(RustEmbed)]
#[folder = "assets/"]
struct Asset;

pub struct AutoContentType(String, ETag, EmbeddedFile);

impl IntoResponse for AutoContentType {
    fn into_response(self) -> axum::response::Response {
        let mut res = self.2.data.into_response();
        res.headers_mut().remove(CONTENT_TYPE);
        res.headers_mut().typed_insert(self.1);
        if let Some(mime) = mime_guess::from_path(&self.0).first_raw() {
            res.headers_mut()
                .append(CONTENT_TYPE, HeaderValue::from_static(mime));
        }
        res
    }
}

#[tracing::instrument]
pub async fn get(
    Path(path): Path<String>,
    if_none_match: Option<TypedHeader<IfNoneMatch>>,
) -> Result<AutoContentType, StatusCode> {
    match Asset::get(&path[1..]) {
        Some(asset) => {
            let hash = asset.metadata.sha256_hash().encode_hex::<String>();
            let etag = format!(r#"{:?}"#, hash).parse::<ETag>().unwrap();
            if let Some(if_none_match) = if_none_match {
                if !if_none_match.precondition_passes(&etag) {
                    return Err(StatusCode::NOT_MODIFIED);
                }
            }
            Ok(AutoContentType(path[1..].to_string(), etag, asset))
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}
