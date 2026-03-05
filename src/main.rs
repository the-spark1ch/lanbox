mod config;
mod handlers;
mod util;

use axum::routing::any;
use axum::Router;

#[tokio::main]
async fn main() {
    let (cfg, state) = config::load().await;

    let app = Router::new()
        .fallback(any(handlers::handle_request))
        .with_state(state.clone());

    let lan_ip = config::get_lan_ipv4()
        .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)));

    println!("LAN:  http://{}:{}/", lan_ip, cfg.port);
    println!("Bind: http://{}:{}/", cfg.host, cfg.port);
    println!("public: {}", state.root.display());
    println!("uploads: {}", state.upload_dir.display());

    axum::serve(
        tokio::net::TcpListener::bind(cfg.bind_addr()).await.unwrap(),
        app,
    )
    .await
    .unwrap();
}
