use axum::{
    body::{to_bytes, Body, BodyDataStream, Bytes},
    http::{
        header::{self, HeaderMap, HeaderValue},
        Method, StatusCode, Uri,
    },
    response::IntoResponse,
    Json,
};
use futures::StreamExt;
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use rand::RngCore;
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tokio::{
    fs,
    io::{self, AsyncWriteExt},
};
use tokio_util::io::ReaderStream;

pub static URI_ENCODE_SET: AsciiSet = NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

#[derive(Serialize)]
pub struct ErrorBody {
    pub error: String,
}

pub fn json_error(code: StatusCode, msg: &str) -> impl IntoResponse {
    (code, Json(ErrorBody { error: msg.to_string() }))
}

pub fn mime_type_for(p: &Path) -> &'static str {
    match p.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
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

pub fn decode_pathname(uri: &Uri) -> Option<String> {
    let raw = uri.path();
    let decoded = percent_decode_str(raw).decode_utf8().ok()?;
    Some(decoded.into_owned())
}

pub fn normalize_dir_param(input: Option<&String>) -> Option<String> {
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

pub fn join_posix(a: &str, b: &str) -> String {
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
    if out.is_empty() {
        "/".to_string()
    } else {
        out
    }
}

pub fn safe_segment(name: &str) -> Option<String> {
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

pub fn basename_string(s: &str) -> String {
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

pub fn safe_final_file_name(original: &str) -> String {
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

pub fn to_safe_absolute_path(root: &Path, pathname: &str) -> Option<PathBuf> {
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

pub fn encode_uri_component(s: &str) -> String {
    utf8_percent_encode(s, &URI_ENCODE_SET).to_string()
}

pub async fn read_body_limited(body: Body, limit: usize) -> Result<Bytes, ()> {
    to_bytes(body, limit).await.map_err(|_| ())
}

#[derive(Serialize, Clone)]
pub struct ListItem {
    pub name: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub size: u64,
    pub mtimeMs: f64,
    pub path: String,
    pub url: Option<String>,
}

pub async fn list_dir(abs_dir: &Path, rel_dir: &str) -> io::Result<Vec<ListItem>> {
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

pub async fn serve_static_file(
    method: &Method,
    abs_in: &Path,
    force_download: bool,
) -> axum::response::Response {
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
    let ct = HeaderValue::from_str(mime_type_for(&abs))
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    headers.insert(header::CONTENT_TYPE, ct);
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&st.len().to_string()).unwrap(),
    );

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

pub fn parse_query(uri: &Uri) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let q = uri.query().unwrap_or("");
    for pair in q.split('&').filter(|s| !s.is_empty()) {
        let mut it = pair.splitn(2, '=');
        let k = it.next().unwrap_or("");
        let v = it.next().unwrap_or("");
        let k = percent_decode_str(k)
            .decode_utf8()
            .ok()
            .map(|s| s.into_owned())
            .unwrap_or_else(|| k.to_string());
        let v = percent_decode_str(v)
            .decode_utf8()
            .ok()
            .map(|s| s.into_owned())
            .unwrap_or_else(|| v.to_string());
        map.insert(k, v);
    }
    map
}

pub async fn write_body_stream_to_new_file(
    mut file: fs::File,
    mut stream: BodyDataStream,
    out_path: &Path,
) -> bool {
    let mut ok = true;

    while let Some(item) = stream.next().await {
        match item {
            Ok(bytes) => {
                if file.write_all(&bytes).await.is_err() {
                    ok = false;
                    break;
                }
            }
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
        let _ = fs::remove_file(out_path).await;
    }
    ok
}