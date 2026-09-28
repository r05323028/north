use axum::routing::get;
use north_persistence::{ConnectOptions, Database, DatabaseConnection, OtpKey};
use std::{env, net::SocketAddr, process::ExitCode, sync::Arc, time::Duration};
use tokio::{net::TcpListener, time::timeout};

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:8080";
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug)]
enum StartupFailure {
    Configuration,
    Database,
    Migration,
    ServerState,
    Bind,
    Serve,
    ShutdownTimeout,
}

impl StartupFailure {
    const fn label(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::Database => "database connection",
            Self::Migration => "migration (run `north-server migrate` if schema is behind)",
            Self::ServerState => "server state",
            Self::Bind => "bind",
            Self::Serve => "server",
            Self::ShutdownTimeout => "shutdown timeout",
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    if let Some(code) = handle_command_line().await {
        return code;
    }

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("north-server startup failed: {}", failure.label());
            ExitCode::FAILURE
        }
    }
}

async fn handle_command_line() -> Option<ExitCode> {
    match env::args().nth(1).as_deref() {
        Some("migrate") => Some(run_migration_command().await),
        Some("--version") | Some("-V") => {
            println!("north-server {}", env!("CARGO_PKG_VERSION"));
            Some(ExitCode::SUCCESS)
        }
        Some("--help") | Some("-h") => {
            println!("North server\n\nUsage: north-server [migrate]\n\nCommands:\n  migrate        Apply pending database migrations and exit\n\nOptions:\n  -V, --version  Print release version\n  -h, --help     Print this help");
            Some(ExitCode::SUCCESS)
        }
        Some(_) => {
            eprintln!("unknown command; use `north-server --help`");
            Some(ExitCode::FAILURE)
        }
        None => None,
    }
}

async fn run_migration_command() -> ExitCode {
    let database_url = match env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("north-server migrate failed: configuration");
            return ExitCode::FAILURE;
        }
    };
    let database = match connect_database(&database_url).await {
        Ok(database) => database,
        Err(_) => {
            eprintln!("north-server migrate failed: database connection");
            return ExitCode::FAILURE;
        }
    };

    let result = north_persistence::run_migrations(&database).await;
    let _ = database.close().await;
    match result {
        Ok(()) => {
            println!("database schema is current");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("north-server migrate failed: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn connect_database(url: &str) -> Result<DatabaseConnection, ()> {
    let mut options = ConnectOptions::new(url);
    options.max_connections(16);
    Database::connect(options).await.map_err(|_| ())
}

async fn run() -> Result<(), StartupFailure> {
    let bind_addr = env::var("NORTH_BIND_ADDR")
        .unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_owned())
        .parse::<SocketAddr>()
        .map_err(|_| StartupFailure::Configuration)?;
    OtpKey::from_env().map_err(|_| StartupFailure::Configuration)?;

    let database_url = env::var("DATABASE_URL").map_err(|_| StartupFailure::Configuration)?;
    let database = connect_database(&database_url)
        .await
        .map_err(|_| StartupFailure::Database)?;
    north_persistence::verify_migrations(&database)
        .await
        .map_err(|_| StartupFailure::Migration)?;

    let app =
        match north_server::build_app(database.clone(), Arc::new(north_server::LogCodeDelivery))
            .await
        {
            Ok(app) => app,
            Err(north_server::BuildAppError::Configuration(_)) => {
                return Err(StartupFailure::Configuration);
            }
            Err(north_server::BuildAppError::Startup(_)) => {
                return Err(StartupFailure::ServerState)
            }
        }
        .route("/healthz", get(healthz));

    let listener = TcpListener::bind(bind_addr)
        .await
        .map_err(|_| StartupFailure::Bind)?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let mut server = Box::pin(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        })
        .await
    });

    let result = tokio::select! {
        result = &mut server => result.map_err(|_| StartupFailure::Serve),
        _ = shutdown_signal() => {
            let _ = shutdown_tx.send(());
            match timeout(SHUTDOWN_TIMEOUT, server.as_mut()).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(_)) => Err(StartupFailure::Serve),
                Err(_) => Err(StartupFailure::ShutdownTimeout),
            }
        }
    };
    let _ = database.close().await;
    result
}

async fn healthz() -> &'static str {
    "ok\n"
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let ctrl_c = async {
            tokio::signal::ctrl_c()
                .await
                .expect("install Ctrl-C handler");
        };
        let terminate = async {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler")
                .recv()
                .await;
        };
        tokio::select! {
            _ = ctrl_c => {},
            _ = terminate => {},
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl-C handler");
    }
}
