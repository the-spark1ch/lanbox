use futures::TryStreamExt;
use axum::{
    body::{to_bytes, Body, Bytes},
    extract::State,
    http::{
        header::{self, HeaderMap, HeaderValue},
        Method, Request, StatusCode, Uri,
    },
    response::IntoResponse,
    routing::any,
    Json, Router,
};
use get_if_addrs::get_if_addrs;
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    fs,
    io::{self, AsyncWriteExt},
};
use tokio_util::io::ReaderStream;

static URI_ENCODE_SET: AsciiSet = NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

#[derive(Clone)]
struct AppState {
    root: PathBuf,
    upload_dir: PathBuf,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

fn json_error(code: StatusCode, msg: &str) -> impl IntoResponse {
    (code, Json(ErrorBody { error: msg.to_string() }))
}

fn mime_type_for(p: &Path) -> &'static str {
    match p.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    }
}

fn decode_pathname(uri: &Uri) -> Option<String> {
    let raw = uri.path();
    let decoded = percent_decode_str(raw).decode_utf8().ok()?;
    Some(decoded.into_owned())
}

fn normalize_dir_param(input: Option<&String>) -> Option<String> {
    let mut d = input.cloned().unwrap_or_default();
    if !d.is_empty() {
        let dec = percent_decode_str(&d).decode_utf8().ok()?;
        d = dec.into_owned();
    }
    d = d.replace('\\', "/");
    d = d.trim().to_string();
    if d.is_empty() || d == "/" {
        return Some("/".to_string());
    }
    if !d.starts_with('/') {
        d.insert(0, '/');
    }
    while d.contains("//") {
        d = d.replace("//", "/");
    }
    while d.ends_with('/') {
        d.pop();
    }
    if d.is_empty() {
        d = "/".to_string();
    }
    if d.contains('\0') {
        return None;
    }
    Some(d)
}

fn join_posix(a: &str, b: &str) -> String {
    let mut a = a.to_string();
    if a.ends_with('/') && a != "/" {
        a.pop();
    }
    let mut b = b.to_string();
    if !b.starts_with('/') {
        b.insert(0, '/');
    }
    let mut out = format!("{a}{b}");
    while out.contains("//") {
        out = out.replace("//", "/");
    }
    if out.is_empty() { "/".to_string() } else { out }
}

fn safe_segment(name: &str) -> Option<String> {
    let s = name.trim();
    if s.is_empty() {
        return None;
    }
    if s == "." || s == ".." {
        return None;
    }
    if s.contains('/') || s.contains('\\') || s.contains('\0') {
        return None;
    }
    let mut cleaned = String::with_capacity(s.len());
    for ch in s.chars() {
        let ok = ch.is_ascii_alphanumeric()
            || ch == '_'
            || ch == '.'
            || ch == '-'
            || ch == '('
            || ch == ')'
            || ch == '@'
            || ch.is_ascii_whitespace();
        cleaned.push(if ok { ch } else { '_' });
    }
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() {
        return None;
    }
    Some(cleaned.chars().take(180).collect())
}

fn basename_string(s: &str) -> String {
    let s2 = s.replace('\\', "/");
    s2.rsplit('/').next().unwrap_or("").to_string()
}

fn split_ext(name: &str) -> (String, String) {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => {
            let (stem, ext) = name.split_at(i);
            (stem.to_string(), ext.to_string())
        }
        _ => (name.to_string(), "".to_string()),
    }
}

fn safe_final_file_name(original: &str) -> String {
    let base = basename_string(original);
    let cleaned = {
        let mut tmp = String::new();
        for ch in base.chars() {
            let ok = ch.is_ascii_alphanumeric()
                || ch == '_'
                || ch == '.'
                || ch == '-'
                || ch == '('
                || ch == ')'
                || ch == '@'
                || ch.is_ascii_whitespace();
            tmp.push(if ok { ch } else { '_' });
        }
        let tmp = tmp.trim().to_string();
        if tmp.is_empty() {
            "file".to_string()
        } else {
            tmp.chars().take(180).collect()
        }
    };
    let (stem0, ext0) = split_ext(&cleaned);
    let ext = ext0.chars().take(16).collect::<String>();
    let mut stem = stem0.chars().take(120).collect::<String>();
    if stem.trim().is_empty() {
        stem = "file".to_string();
    }
    let mut id = [0u8; 6];
    rand::thread_rng().fill_bytes(&mut id);
    let hex = id.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    format!("{}-{}{}", stem.trim(), hex, ext)
}

fn to_safe_absolute_path(root: &Path, pathname: &str) -> Option<PathBuf> {
    let mut p = pathname.to_string();
    if !p.starts_with('/') {
        p.insert(0, '/');
    }
    if p.contains('\0') {
        return None;
    }
    let rel = p.trim_start_matches('/');

    let mut parts: Vec<&str> = Vec::new();
    for seg in rel.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            if parts.is_empty() {
                return None;
            }
            parts.pop();
            continue;
        }
        if seg.contains('\0') {
            return None;
        }
        parts.push(seg);
    }

    let mut out = PathBuf::from(root);
    for seg in parts {
        out.push(seg);
    }

    if !out.starts_with(root) {
        return None;
    }
    Some(out)
}

fn encode_uri_component(s: &str) -> String {
    utf8_percent_encode(s, &URI_ENCODE_SET).to_string()
}

async fn read_body_limited(body: Body, limit: usize) -> Result<Bytes, ()> {
    to_bytes(body, limit).await.map_err(|_| ())
}

#[derive(Serialize)]
struct ListItem {
    name: String,
    #[serde(rename = "type")]
    item_type: String,
    size: u64,
    mtimeMs: f64,
    path: String,
    url: Option<String>,
}

async fn list_dir(abs_dir: &Path, rel_dir: &str) -> io::Result<Vec<ListItem>> {
    let mut rd = fs::read_dir(abs_dir).await?;
    let mut out: Vec<ListItem> = Vec::new();

    while let Some(ent) = rd.next_entry().await? {
        let name_os = ent.file_name();
        let name = match name_os.to_str() {
            Some(s) => s.to_string(),
            None => continue,
        };
        if name.starts_with('.') {
            continue;
        }
        let abs = ent.path();
        let st = match fs::metadata(&abs).await {
            Ok(m) => m,
            Err(_) => continue,
        };
        let is_dir = st.is_dir();
        let rel_path = join_posix(rel_dir, &name);
        let url = if is_dir {
            None
        } else {
            let mut u = String::from("/uploads");
            for seg in rel_path.split('/').filter(|s| !s.is_empty()) {
                u.push('/');
                u.push_str(&encode_uri_component(seg));
            }
            Some(u)
        };
        let mtime_ms = st
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);

        out.push(ListItem {
            name,
            item_type: if is_dir { "dir" } else { "file" }.to_string(),
            size: if is_dir { 0 } else { st.len() },
            mtimeMs: mtime_ms,
            path: rel_path,
            url,
        });
    }

    out.sort_by(|a, b| {
        if a.item_type != b.item_type {
            return if a.item_type == "dir" {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
        a.name.cmp(&b.name)
    });

    Ok(out)
}

async fn serve_static_file(method: &Method, abs_in: &Path, force_download: bool) -> axum::response::Response {
    let mut abs = abs_in.to_path_buf();

    let st0 = match fs::metadata(&abs).await {
        Ok(m) => m,
        Err(_) => return json_error(StatusCode::NOT_FOUND, "Not found").into_response(),
    };

    if st0.is_dir() {
        let index = abs.join("index.html");
        match fs::metadata(&index).await {
            Ok(m) if m.is_file() => abs = index,
            _ => return json_error(StatusCode::FORBIDDEN, "Directory").into_response(),
        }
    }

    let st = match fs::metadata(&abs).await {
        Ok(m) => m,
        Err(_) => return json_error(StatusCode::NOT_FOUND, "Not found").into_response(),
    };

    if !st.is_file() {
        return json_error(StatusCode::FORBIDDEN, "Forbidden").into_response();
    }

    let mut headers = HeaderMap::new();
    let ct = HeaderValue::from_str(mime_type_for(&abs)).unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    headers.insert(header::CONTENT_TYPE, ct);
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from_str(&st.len().to_string()).unwrap());

    if force_download {
        let fname = abs
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file")
            .replace(['\r', '\n', '"'], "_");
        let v = format!("attachment; filename=\"{}\"", fname);
        if let Ok(hv) = HeaderValue::from_str(&v) {
            headers.insert(header::CONTENT_DISPOSITION, hv);
        }
    }

    if method == Method::HEAD {
        return (StatusCode::OK, headers, Body::empty()).into_response();
    }

    let file = match fs::File::open(&abs).await {
        Ok(f) => f,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Read error").into_response(),
    };

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);
    (StatusCode::OK, headers, body).into_response()
}

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

fn parse_query(uri: &Uri) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let q = uri.query().unwrap_or("");
    for pair in q.split('&').filter(|s| !s.is_empty()) {
        let mut it = pair.splitn(2, '=');
        let k = it.next().unwrap_or("");
        let v = it.next().unwrap_or("");
        let k = percent_decode_str(k).decode_utf8().ok().map(|s| s.into_owned()).unwrap_or_else(|| k.to_string());
        let v = percent_decode_str(v).decode_utf8().ok().map(|s| s.into_owned()).unwrap_or_else(|| v.to_string());
        map.insert(k, v);
    }
    map
}

async fn handle_upload_stream(st: Arc<AppState>, uri: Uri, req: Request<Body>) -> axum::response::Response {
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

    let mut file = match fs::OpenOptions::new().create_new(true).write(true).open(&out_path).await {
        Ok(f) => f,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Cannot create file").into_response(),
    };

    let mut stream = req.into_body().into_data_stream();

    let mut ok = true;
    loop {
        match stream.try_next().await {
            Ok(Some(bytes)) => {
                if file.write_all(&bytes).await.is_err() {
                    ok = false;
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => {
                ok = false;
                break;
            }
        }
    }

    if file.flush().await.is_err() {
        ok = false;
    }
    drop(file);

    if !ok {
        let _ = fs::remove_file(&out_path).await;
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
    }

    let rel_path = join_posix(&dir, &final_name);
    let mut url = String::from("/uploads");
    for seg in rel_path.split('/').filter(|s| !s.is_empty()) {
        url.push('/');
        url.push_str(&encode_uri_component(seg));
    }

    (StatusCode::CREATED, Json(UploadResponse {
        files: vec![UploadFileItem { name: final_name, path: rel_path, url }],
    }))
        .into_response()
}

async fn handle_request(State(st): State<Arc<AppState>>, req: Request<Body>) -> axum::response::Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let headers = req.headers().clone();

    let pathname = match decode_pathname(&uri) {
        Some(p) => p,
        None => return json_error(StatusCode::BAD_REQUEST, "Bad URL encoding").into_response(),
    };

    if pathname.starts_with("/api/") {
        let allowed = matches!(method, Method::GET | Method::POST | Method::PUT | Method::DELETE | Method::HEAD);
        if !allowed {
            return json_error(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed").into_response();
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
                Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Cannot list").into_response(),
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
                    return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Cannot create").into_response();
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
                if out.is_empty() { "/".to_string() } else { out }
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
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Cannot rename").into_response();
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

            return (StatusCode::OK, Json(RenameResponse {
                ok: true,
                from: p,
                to: new_rel.clone(),
                item: RenameItem {
                    name: new_name,
                    item_type: if is_dir { "dir" } else { "file" }.to_string(),
                    path: new_rel,
                    url,
                },
            }))
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
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Cannot delete").into_response();
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
                let req2 = Request::builder().method(method).uri(uri.clone()).body(req.into_body()).unwrap();
                return handle_upload_stream(st.clone(), uri, req2).await;
            }
            return json_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "Use application/octet-stream").into_response();
        }

        return json_error(StatusCode::NOT_FOUND, "Unknown API").into_response();
    }

    if pathname.starts_with("/uploads/") || pathname == "/uploads" {
        let sub = if pathname == "/uploads" { "/".to_string() } else { pathname["/uploads".len()..].to_string() };
        let abs = match to_safe_absolute_path(&st.upload_dir, &sub) {
            Some(v) => v,
            None => return json_error(StatusCode::FORBIDDEN, "Forbidden").into_response(),
        };
        if !(method == Method::GET || method == Method::HEAD) {
            return json_error(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed").into_response();
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

fn get_lan_ipv4() -> Option<IpAddr> {
    let ifaces = get_if_addrs().ok()?;
    for iface in ifaces {
        if iface.is_loopback() {
            continue;
        }
        if let IpAddr::V4(v4) = iface.ip() {
            return Some(IpAddr::V4(v4));
        }
    }
    None
}

#[tokio::main]
async fn main() {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = std::env::var("ROOT").ok().map(PathBuf::from).unwrap_or_else(|| cwd.join("public"));
    let upload_dir = std::env::var("UPLOAD_DIR").ok().map(PathBuf::from).unwrap_or_else(|| cwd.join("uploads"));

    let port: u16 = std::env::var("PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(8080);
    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());

    let _ = fs::create_dir_all(&root).await;
    let _ = fs::create_dir_all(&upload_dir).await;

    let state = Arc::new(AppState {
        root: root.canonicalize().unwrap_or(root),
        upload_dir: upload_dir.canonicalize().unwrap_or(upload_dir),
    });

    let app = Router::new().fallback(any(handle_request)).with_state(state.clone());

    let lan_ip = get_lan_ipv4().unwrap_or(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
    println!("LAN:  http://{}:{}/", lan_ip, port);
    println!("Bind: http://{}:{}/", host, port);
    println!("public: {}", state.root.display());
    println!("uploads: {}", state.upload_dir.display());

    let addr: SocketAddr = format!("{}:{}", host, port).parse().unwrap();
    axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app).await.unwrap();
}
