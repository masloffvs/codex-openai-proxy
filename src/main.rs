use anyhow::Result;
use clap::Parser;
use log::info;

mod cli;
mod logging;
mod proxy;
mod routes;
mod types;

use cli::Args;
use proxy::ProxyServer;

#[tokio::main]
async fn main() -> Result<()> {
    logging::init();
    let args = Args::parse();

    info!(target: "startup", "service.initializing name=codex-openai-proxy");

    let proxy = ProxyServer::new(&args.auth_path).await?;
    info!(target: "startup", "auth.loaded path={}", args.auth_path);

    let routes = routes::create_routes(proxy);

    info!(
        target: "startup",
        "service.listening bind=0.0.0.0:{} health_path=/health chat_path=/v1/chat/completions",
        args.port
    );
    info!(
        target: "startup",
        "client.config base_url=http://localhost:{} model=gpt-5 api_key=any-value",
        args.port
    );
    info!(
        target: "startup",
        "diagnostics.hint enable_verbose_logging=RUST_LOG=debug"
    );

    warp::serve(routes).run(([0, 0, 0, 0], args.port)).await;

    Ok(())
}
