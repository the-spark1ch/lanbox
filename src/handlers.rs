use crate::config::AppState;
use crate::util::{
    basename_string, decode_pathname, encode_uri_component, json_error, join_posix, list_dir,
    normalize_dir_param, parse_query, read_body_limited, safe_final_file_name, safe_segment,
    serve_static_file, to_safe_absolute_path, write_body_stream_to_new_file, ListItem,
};
use axum::{
    body::Body,
    extract::State,
    http::{header, Method, Request, StatusCode, Uri},
    response::IntoResponse,
    Json,
};
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::{fs, io};

#[derive(Deserialize)]
struct MkdirBody {
    dir: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct RenameBody {
    path: Option<String>,
    name: Option<String>,
}

#[derive(Serialize)]
struct ListResponse {
    dir: String,
    parent: Option<String>,
    items: Vec<ListItem>,
}

#[derive(Serialize)]
struct OkResponse {
    ok: bool,
}

#[derive(Serialize)]
struct UploadFileItem {
    name: String,
    path: String,
    url: String,
}

#[derive(Serialize)]
struct UploadResponse {
    files: Vec<UploadFileItem>,
}

#[derive(Serialize)]
struct RenameItem {
    name: String,
    #[serde(rename = "type")]
    item_type: String,
    path: String,
    url: Option<String>,
}

#[derive(Serialize)]
struct RenameResponse {
    ok: bool,
    from: String,
    to: String,
    item: RenameItem,
}

async fn handle_upload_stream(
    st: Arc<AppState>,
    uri: Uri,
    req: Request<Body>,
) -> axum::response::Response {
    let q = parse_query(&uri);
    let dir = match normalize_dir_param(q.get("dir")) {
        Some(d) => d,
        None => return json_error(StatusCode::BAD_REQUEST, "Bad dir").into_response(),
    };

    let name_param = q.get("name").cloned().unwrap_or_default();
    let decoded_name = percent_decode_str(&name_param)
        .decode_utf8()
        .map(|s| s.into_owned())
        .unwrap_or(name_param);

    let abs_dir = match to_safe_absolute_path(&st.upload_dir, &dir) {
        Some(p) => p,
        None => return json_error(StatusCode::FORBIDDEN, "Forbidden").into_response(),
    };

    let md = match fs::metadata(&abs_dir).await {
        Ok(m) => m,
        Err(_) => return json_error(StatusCode::NOT_FOUND, "Not found").into_response(),
    };
    if !md.is_dir() {
        return json_error(StatusCode::NOT_FOUND, "Not found").into_response();
    }

    let original_base = basename_string(&decoded_name);
    let original = safe_segment(&original_base).unwrap_or_else(|| "file".to_string());
    let final_name = safe_final_file_name(&original);
    let out_path = abs_dir.join(&final_name);

    if !out_path.starts_with(&abs_dir) {
        return json_error(StatusCode::FORBIDDEN, "Forbidden").into_response();
    }

    let file = match fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&out_path)
        .await
    {
        Ok(f) => f,
        Err(_) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Cannot create file")
                .into_response();
        }
    };

    let stream = req.into_body().into_data_stream();
    let ok = write_body_stream_to_new_file(file, stream, &out_path).await;
    if !ok {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
    }

    let rel_path = join_posix(&dir, &final_name);
    let mut url = String::from("/uploads");
    for seg in rel_path.split('/').filter(|s| !s.is_empty()) {
        url.push('/');
        url.push_str(&encode_uri_component(seg));
    }

    (
        StatusCode::CREATED,
        Json(UploadResponse {
            files: vec![UploadFileItem {
                name: final_name,
                path: rel_path,
                url,
            }],
        }),
    )
        .into_response()
}

pub async fn handle_request(
    State(st): State<Arc<AppState>>,
    req: Request<Body>,
) -> axum::response::Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let headers = req.headers().clone();

    let pathname = match decode_pathname(&uri) {
        Some(p) => p,
        None => return json_error(StatusCode::BAD_REQUEST, "Bad URL encoding").into_response(),
    };

    if pathname.starts_with("/api/") {
        let allowed = matches!(
            method,
            Method::GET | Method::POST | Method::PUT | Method::DELETE | Method::HEAD
        );
        if !allowed {
            return json_error(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed")
                .into_response();
        }

        if method == Method::GET && pathname == "/api/list" {
            let q = parse_query(&uri);
            let dir = match normalize_dir_param(q.get("dir")) {
                Some(d) => d,
                None => return json_error(StatusCode::BAD_REQUEST, "Bad dir").into_response(),
            };
            let abs_dir = match to_safe_absolute_path(&st.upload_dir, &dir) {
                Some(p) => p,
                None => return json_error(StatusCode::FORBIDDEN, "Forbidden").into_response(),
            };
            let md = match fs::metadata(&abs_dir).await {
                Ok(m) => m,
                Err(_) => return json_error(StatusCode::NOT_FOUND, "Not found").into_response(),
            };
            if !md.is_dir() {
                return json_error(StatusCode::BAD_REQUEST, "Not a directory").into_response();
            }
            let items = match list_dir(&abs_dir, &dir).await {
                Ok(v) => v,
                Err(_) => {
                    return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Cannot list")
                        .into_response();
                }
            };
            let parent = if dir == "/" {
                None
            } else {
                let mut parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
                parts.pop();
                let p = format!("/{}", parts.join("/"));
                Some(if p.is_empty() { "/".to_string() } else { p })
            };
            return (StatusCode::OK, Json(ListResponse { dir, parent, items })).into_response();
        }

        if method == Method::POST && pathname == "/api/mkdir" {
            let body = match read_body_limited(req.into_body(), 64 * 1024).await {
                Ok(b) => b,
                Err(_) => return json_error(StatusCode::BAD_REQUEST, "Bad body").into_response(),
            };
            let j: MkdirBody = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(_) => return json_error(StatusCode::BAD_REQUEST, "Bad json").into_response(),
            };
            let dir = match normalize_dir_param(j.dir.as_ref()) {
                Some(d) => d,
                None => return json_error(StatusCode::BAD_REQUEST, "Bad dir").into_response(),
            };
            let name = match j.name.as_deref().and_then(safe_segment) {
                Some(n) => n,
                None => return json_error(StatusCode::BAD_REQUEST, "Bad name").into_response(),
            };
            let abs_dir = match to_safe_absolute_path(&st.upload_dir, &dir) {
                Some(p) => p,
                None => return json_error(StatusCode::FORBIDDEN, "Forbidden").into_response(),
            };
            let target = abs_dir.join(&name);
            if !target.starts_with(&abs_dir) && target != abs_dir {
                return json_error(StatusCode::FORBIDDEN, "Forbidden").into_response();
            }
            match fs::create_dir(&target).await {
                Ok(_) => return (StatusCode::CREATED, Json(OkResponse { ok: true })).into_response(),
                Err(e) => {
                    if e.kind() == io::ErrorKind::AlreadyExists {
                        return json_error(StatusCode::CONFLICT, "Already exists").into_response();
                    }
                    return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Cannot create")
                        .into_response();
                }
            }
        }

        if method == Method::POST && pathname == "/api/rename" {
            let body = match read_body_limited(req.into_body(), 64 * 1024).await {
                Ok(b) => b,
                Err(_) => return json_error(StatusCode::BAD_REQUEST, "Bad body").into_response(),
            };
            let j: RenameBody = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(_) => return json_error(StatusCode::BAD_REQUEST, "Bad json").into_response(),
            };
            let p = match normalize_dir_param(j.path.as_ref()) {
                Some(v) => v,
                None => return json_error(StatusCode::BAD_REQUEST, "Bad path").into_response(),
            };
            if p == "/" {
                return json_error(StatusCode::BAD_REQUEST, "Cannot rename root").into_response();
            }
            let new_name = match j.name.as_deref().and_then(safe_segment) {
                Some(v) => v,
                None => return json_error(StatusCode::BAD_REQUEST, "Bad name").into_response(),
            };

            let abs_old = match to_safe_absolute_path(&st.upload_dir, &p) {
                Some(v) => v,
                None => return json_error(StatusCode::FORBIDDEN, "Forbidden").into_response(),
            };
            let old_md = match fs::metadata(&abs_old).await {
                Ok(m) => m,
                Err(_) => return json_error(StatusCode::NOT_FOUND, "Not found").into_response(),
            };

            let parent_rel = {
                let mut parts: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
                parts.pop();
                let out = format!("/{}", parts.join("/"));
                if out.is_empty() {
                    "/".to_string()
                } else {
                    out
                }
            };

            let abs_parent = match to_safe_absolute_path(&st.upload_dir, &parent_rel) {
                Some(v) => v,
                None => return json_error(StatusCode::FORBIDDEN, "Forbidden").into_response(),
            };

            let abs_new = abs_parent.join(&new_name);
            if !abs_new.starts_with(&abs_parent) {
                return json_error(StatusCode::FORBIDDEN, "Forbidden").into_response();
            }

            if fs::metadata(&abs_new).await.is_ok() {
                return json_error(StatusCode::CONFLICT, "Already exists").into_response();
            }

            if fs::rename(&abs_old, &abs_new).await.is_err() {
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Cannot rename")
                    .into_response();
            }

            let new_rel = join_posix(&parent_rel, &new_name);
            let is_dir = old_md.is_dir();
            let url = if is_dir {
                None
            } else {
                let mut u = String::from("/uploads");
                for seg in new_rel.split('/').filter(|s| !s.is_empty()) {
                    u.push('/');
                    u.push_str(&encode_uri_component(seg));
                }
                Some(u)
            };

            return (
                StatusCode::OK,
                Json(RenameResponse {
                    ok: true,
                    from: p,
                    to: new_rel.clone(),
                    item: RenameItem {
                        name: new_name,
                        item_type: if is_dir { "dir" } else { "file" }.to_string(),
                        path: new_rel,
                        url,
                    },
                }),
            )
                .into_response();
        }

        if method == Method::DELETE && pathname == "/api/delete" {
            let q = parse_query(&uri);
            let p = match normalize_dir_param(q.get("path")) {
                Some(v) => v,
                None => return json_error(StatusCode::BAD_REQUEST, "Bad path").into_response(),
            };
            if p == "/" {
                return json_error(StatusCode::BAD_REQUEST, "Cannot delete root").into_response();
            }
            let abs = match to_safe_absolute_path(&st.upload_dir, &p) {
                Some(v) => v,
                None => return json_error(StatusCode::FORBIDDEN, "Forbidden").into_response(),
            };
            let md = match fs::metadata(&abs).await {
                Ok(m) => m,
                Err(_) => return json_error(StatusCode::NOT_FOUND, "Not found").into_response(),
            };

            let del_res = if md.is_dir() {
                fs::remove_dir_all(&abs).await
            } else {
                fs::remove_file(&abs).await
            };

            if del_res.is_err() {
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Cannot delete")
                    .into_response();
            }
            return (StatusCode::OK, Json(OkResponse { ok: true })).into_response();
        }

        if (method == Method::POST || method == Method::PUT) && pathname == "/api/upload" {
            let ct = headers
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_ascii_lowercase();

            if ct.starts_with("application/octet-stream") || method == Method::PUT {
                let req2 = Request::builder()
                    .method(method)
                    .uri(uri.clone())
                    .body(req.into_body())
                    .unwrap();
                return handle_upload_stream(st.clone(), uri, req2).await;
            }
            return json_error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Use application/octet-stream",
            )
            .into_response();
        }

        return json_error(StatusCode::NOT_FOUND, "Unknown API").into_response();
    }

    if pathname.starts_with("/uploads/") || pathname == "/uploads" {
        let sub = if pathname == "/uploads" {
            "/".to_string()
        } else {
            pathname["/uploads".len()..].to_string()
        };
        let abs = match to_safe_absolute_path(&st.upload_dir, &sub) {
            Some(v) => v,
            None => return json_error(StatusCode::FORBIDDEN, "Forbidden").into_response(),
        };
        if !(method == Method::GET || method == Method::HEAD) {
            return json_error(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed")
                .into_response();
        }
        return serve_static_file(&method, &abs, true).await;
    }

    let abs = match to_safe_absolute_path(&st.root, &pathname) {
        Some(v) => v,
        None => return json_error(StatusCode::FORBIDDEN, "Forbidden").into_response(),
    };
    if !(method == Method::GET || method == Method::HEAD) {
        return json_error(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed").into_response();
    }
    serve_static_file(&method, &abs, false).await
}
