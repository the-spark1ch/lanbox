use get_if_addrs::get_if_addrs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;

#[derive(Clone)]
pub struct AppState {
    pub root: PathBuf,
    pub upload_dir: PathBuf,
}

#[derive(Clone)]
pub struct Config {
    pub root: PathBuf,
    pub upload_dir: PathBuf,
    pub host: String,
    pub port: u16,
}

impl Config {
    pub fn bind_addr(&self) -> SocketAddr {
        format!("{}:{}", self.host, self.port).parse().unwrap()
    }
}

pub async fn load() -> (Config, Arc<AppState>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = std::env::var("ROOT")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| cwd.join("public"));
    let upload_dir = std::env::var("UPLOAD_DIR")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| cwd.join("uploads"));

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);
    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());

    let _ = fs::create_dir_all(&root).await;
    let _ = fs::create_dir_all(&upload_dir).await;

    let state = Arc::new(AppState {
        root: root.canonicalize().unwrap_or(root.clone()),
        upload_dir: upload_dir.canonicalize().unwrap_or(upload_dir.clone()),
    });

    (
        Config {
            root,
            upload_dir,
            host,
            port,
        },
        state,
    )
}

pub fn get_lan_ipv4() -> Option<IpAddr> {
    let ifaces = get_if_addrs().ok()?;
    for iface in ifaces {
        if iface.is_loopback() {
            continue;
        }
        if let IpAddr::V4(v4) = iface.ip() {
            return Some(IpAddr::V4(v4));
        }
    }
    Some(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)))
}