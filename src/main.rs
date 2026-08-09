use std::{
    io::{Write as _, stdout},
    process::ExitCode,
    thread,
    time::Duration,
};

use apple_music_tui::{
    auth::{
        AppleApiVerifier, AuthError, AuthVerifier, BrowserAuthorization, CredentialStore,
        DeveloperTokenProvider, DeveloperTokenService, KeychainCredentialStore, load_apple_config,
        local_auth_status,
    },
    backend::{
        MusicBackend,
        macos::{LocalCacheClearResult, MacOsMusicBackend},
        mock::MockMusicBackend,
    },
    cli::{AuthCommand, BackendChoice, CliAction},
    config::default_config_path,
    doctor,
    error::AppError,
    runtime,
    terminal::{TerminalSession, install_panic_hook},
    ui::artwork,
};

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();
    install_panic_hook();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("apple-music-tui: {error}");
            ExitCode::FAILURE
        }
    }
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("off"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .try_init();
}

async fn run() -> Result<(), AppError> {
    match CliAction::parse_env()? {
        CliAction::Help => {
            println!("{}", CliAction::help_text());
            Ok(())
        }
        CliAction::Version => {
            println!("apple-music-tui {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        CliAction::ConfigPath => {
            println!(
                "{}",
                default_config_path().map_err(AuthError::from)?.display()
            );
            Ok(())
        }
        CliAction::Doctor => {
            doctor::run().await.print();
            Ok(())
        }
        CliAction::CacheStatus => {
            print_cache_status();
            Ok(())
        }
        CliAction::CacheClear => clear_cache(),
        CliAction::ArtworkTest => run_artwork_test(),
        CliAction::Auth(command) => run_auth(command).await,
        CliAction::Run(BackendChoice::Mock) => run_tui(MockMusicBackend::new()).await,
        CliAction::Run(BackendChoice::Macos) => run_tui(MacOsMusicBackend::new()).await,
        CliAction::Run(BackendChoice::Auto) => run_auto_backend().await,
        CliAction::Run(backend @ BackendChoice::Apple) => {
            Err(AppError::UnavailableBackend(backend.to_string()))
        }
    }
}

fn print_cache_status() {
    let cache = MacOsMusicBackend::local_cache_status();
    println!("Music.app local library cache\n");
    println!(
        "path: {}",
        cache.path.as_ref().map_or_else(
            || "unavailable".to_owned(),
            |path| path.display().to_string()
        )
    );
    println!(
        "schema: {}",
        cache
            .schema_version
            .map_or_else(|| "unknown".to_owned(), |version| version.to_string())
    );
    println!(
        "tracks: {}",
        cache
            .tracks
            .map_or_else(|| "unknown".to_owned(), |count| count.to_string())
    );
    println!(
        "playlists: {}",
        cache
            .playlists
            .map_or_else(|| "unknown".to_owned(), |count| count.to_string())
    );
    println!(
        "last updated: {}",
        cache
            .last_updated_unix_seconds
            .map_or_else(|| "unknown".to_owned(), |seconds| format!("Unix {seconds}"))
    );
    println!("readable: {}", if cache.readable { "yes" } else { "no" });
}

fn clear_cache() -> Result<(), AppError> {
    match MacOsMusicBackend::clear_local_cache().map_err(AppError::Terminal)? {
        LocalCacheClearResult::Removed => println!("Removed local Music.app metadata cache."),
        LocalCacheClearResult::NotFound => {
            println!("No local Music.app metadata cache was present.")
        }
        LocalCacheClearResult::Unavailable => {
            println!("Local Music.app metadata cache path is unavailable.")
        }
    }
    Ok(())
}

fn run_artwork_test() -> Result<(), AppError> {
    let mut output = stdout();
    writeln!(
        output,
        "Artwork Kitty probe: the opaque red 1x1 PNG appears below this line for two seconds."
    )
    .and_then(|_| output.flush())
    .map_err(AppError::Terminal)?;
    let report = artwork::write_kitty_probe(&mut output).map_err(AppError::Terminal)?;
    if report.flush_succeeded {
        thread::sleep(Duration::from_secs(2));
        output
            .write_all(b"\x1b[2B\r")
            .and_then(|_| output.flush())
            .map_err(AppError::Terminal)?;
    }
    writeln!(output, "\nArtwork Kitty probe report").map_err(AppError::Terminal)?;
    for line in report.lines() {
        writeln!(output, "{line}").map_err(AppError::Terminal)?;
    }
    Ok(())
}

async fn run_tui<B: MusicBackend>(backend: B) -> Result<(), AppError> {
    let auth_status = local_auth_status(&KeychainCredentialStore);
    let mut terminal = TerminalSession::enter()?;
    let app_result = runtime::run(terminal.terminal_mut(), backend, auth_status).await;
    let restore_result = terminal.restore();
    app_result.and(restore_result)
}

async fn run_auth(command: AuthCommand) -> Result<(), AppError> {
    let store = KeychainCredentialStore;
    match command {
        AuthCommand::Login => authenticate(&store).await,
        AuthCommand::Status => authentication_status(&store).await,
        AuthCommand::Logout => {
            if store.delete()? {
                println!("Apple Music authorization removed from macOS Keychain.");
            } else {
                println!("No Apple Music authorization was stored.");
            }
            Ok(())
        }
    }
}

async fn authenticate<S: CredentialStore>(store: &S) -> Result<(), AppError> {
    println!("Apple Music authentication\n");
    let path = default_config_path().map_err(AuthError::from)?;
    let config = load_apple_config(&path)?;
    println!("✓ Developer configuration");
    println!("✓ Private key readable");

    let tokens = DeveloperTokenService::new(config);
    let developer_token = tokens.token(None)?;
    println!("✓ Developer Token generated");
    println!("• Complete authorization in the browser window.");
    let user_token = BrowserAuthorization::authorize(&tokens).await?;
    println!("✓ User authorization");

    let verification = AppleApiVerifier::new()?
        .verify(&developer_token, &user_token)
        .await?;
    println!(
        "✓ Apple Music API authentication verified (storefront {})",
        verification.storefront
    );
    store.save(&user_token)?;
    println!("✓ Music User Token stored in macOS Keychain");
    Ok(())
}

async fn authentication_status<S: CredentialStore>(store: &S) -> Result<(), AppError> {
    println!("Apple Music authentication status\n");
    let path = default_config_path().map_err(AuthError::from)?;
    let config = load_apple_config(&path)?;
    let tokens = DeveloperTokenService::new(config);
    let developer_token = tokens.token(None)?;
    println!("✓ Developer Token generation");
    let user_token = store
        .load()?
        .ok_or(apple_music_tui::auth::AuthError::UserTokenMissing)?;
    println!("✓ Music User Token in macOS Keychain");
    let verification = AppleApiVerifier::new()?
        .verify(&developer_token, &user_token)
        .await?;
    println!(
        "✓ Apple Music API authentication verified (storefront {})",
        verification.storefront
    );
    Ok(())
}

#[cfg(target_os = "macos")]
async fn run_auto_backend() -> Result<(), AppError> {
    run_tui(MacOsMusicBackend::new()).await
}

#[cfg(not(target_os = "macos"))]
async fn run_auto_backend() -> Result<(), AppError> {
    run_tui(MockMusicBackend::new()).await
}
