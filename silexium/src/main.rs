use anyhow::Context;
use axum::routing::get;
use axum::{Json, Router};
use clap::{Arg, Command};
use tower_http::cors::CorsLayer;

pub mod silex;

fn cli() -> Command {
    Command::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .subcommand(
            Command::new("serve")
                .about("Run the hub service")
                .arg(
                    Arg::new("host")
                        .long("host")
                        .short('h')
                        .help("Host address to bind to")
                        .default_value("0.0.0.0")
                        .default_missing_value("0.0.0.0")
                        .help("Host address to bind to"),
                )
                .arg(
                    Arg::new("port")
                        .long("port")
                        .short('p')
                        .help("Port to bind to")
                        .default_missing_value("8080")
                        .default_value("8080")
                        .help("Port to bind to"),
                ),
        )
}

async fn health() -> Json<&'static str> {
    Json("OK")
}
async fn resolve() -> Json<&'static str> {
    Json("OK")
}

#[tokio::main]
async fn main() {
    let matches = cli().get_matches();
    if let Some(matches) = matches.subcommand_matches("serve") {
        let host = matches.get_one::<String>("host").expect("missing host");
        let port = matches.get_one::<String>("port").expect("missing port");
        let app = Router::new()
            .route("/health", get(health))
            .route("/resolve", get(resolve))
            .layer(CorsLayer::very_permissive());

        let bind_addr = format!("{host}:{port}");
        println!("Listening on https://{bind_addr}");
        let listener = tokio::net::TcpListener::bind(&bind_addr)
            .await
            .with_context(|| format!("failed to bind {bind_addr}"))
            .expect("failed to bind");

        axum::serve(listener, app)
            .await
            .context("server error")
            .expect("server error");
    }
}
