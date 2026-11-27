use jade_match_server::{AllowedOrigins, build_app_state, build_router};
use std::net::SocketAddr;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "jade_match_server=info".into()),
        )
        .init();

    let db_url = if let Ok(path) = std::env::var("DATABASE_URL_FILE")
        && !path.trim().is_empty()
    {
        std::fs::read_to_string(path.trim())
            .expect("read DATABASE_URL_FILE")
            .trim()
            .to_string()
    } else {
        std::env::var("DATABASE_URL")
            .or_else(|_| std::env::var("JADE_SQLITE_URL"))
            .unwrap_or_else(|_| "sqlite:jade_dev.db?mode=rwc".into())
    };
    let bind_addr: SocketAddr = std::env::var("JADE_BIND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:4180".into())
        .parse()
        .expect("valid JADE_BIND_ADDR");

    // A hosted deployment names its own browser origins; local development keeps the local defaults. The setting is
    // checked before the database is touched, so a mistyped one changes no stored state.
    let allowed_origins = match std::env::var("JADE_ALLOWED_ORIGINS") {
        Ok(setting) => AllowedOrigins::parse(&setting).expect("valid JADE_ALLOWED_ORIGINS"),
        Err(std::env::VarError::NotPresent) => AllowedOrigins::local(),
        Err(error) => panic!("JADE_ALLOWED_ORIGINS cannot be read: {error}"),
    };

    // Bind the listening socket BEFORE acquiring DB ownership or running outage recovery!
    let listener = TcpListener::bind(bind_addr)
        .await
        .expect("bind match-server listener");
    let state = build_app_state(&db_url)
        .await
        .expect("initialize match-server SQLx state")
        .with_allowed_origins(allowed_origins);
    let app = build_router(state);

    tracing::info!("Jade authoritative match server listening on http://{bind_addr}");
    axum::serve(listener, app).await.expect("run match-server");
}
