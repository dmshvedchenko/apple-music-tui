use std::path::Path;

use crate::{
    auth::{
        AppleApiVerifier, AuthVerifier, CredentialStore, DeveloperTokenProvider,
        DeveloperTokenService, KeychainCredentialStore,
    },
    backend::{MusicBackend, macos::MacOsMusicBackend},
    config::{
        AppConfig, default_config_path, expand_home, home_directory, validate_identifier,
        validate_private_key_path, validate_storefront,
    },
    domain::BackendAvailability,
    ui::artwork,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckStatus {
    Pass,
    Warning,
    Failure,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DoctorCheck {
    pub name: &'static str,
    pub status: CheckStatus,
    pub detail: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    fn push(&mut self, name: &'static str, status: CheckStatus, detail: impl Into<String>) {
        self.checks.push(DoctorCheck {
            name,
            status,
            detail: detail.into(),
        });
    }

    #[must_use]
    pub fn has_failures(&self) -> bool {
        self.checks
            .iter()
            .any(|check| check.status == CheckStatus::Failure)
    }

    pub fn print(&self) {
        println!("apple-music-tui doctor\n");
        for check in &self.checks {
            let marker = match check.status {
                CheckStatus::Pass => "✓",
                CheckStatus::Warning => "⚠",
                CheckStatus::Failure => "✗",
            };
            println!("{marker} {}: {}", check.name, check.detail);
        }
    }
}

pub async fn run() -> DoctorReport {
    let mut report = DoctorReport::default();
    check_release_and_local_paths(&mut report);
    check_terminal_artwork(&mut report);
    check_platform_and_music_app(&mut report).await;
    check_authentication(&mut report).await;
    report
}

fn check_release_and_local_paths(report: &mut DoctorReport) {
    report.push(
        "Version",
        CheckStatus::Pass,
        format!("{} (Cargo package version)", env!("CARGO_PKG_VERSION")),
    );
    report.push(
        "Local backend",
        CheckStatus::Pass,
        "Music.app is the primary local-only backend (--backend macos)",
    );
    match default_config_path() {
        Ok(path) => report.push("Config path", CheckStatus::Pass, path.display().to_string()),
        Err(error) => report.push("Config path", CheckStatus::Failure, error.to_string()),
    }
    let cache = MacOsMusicBackend::local_cache_status();
    let path = cache.path.as_ref().map_or_else(
        || "unavailable".to_owned(),
        |path| path.display().to_string(),
    );
    let detail = if cache.readable {
        format!(
            "{path}; schema {}; {} tracks; {} playlists; updated {}",
            cache.schema_version.unwrap_or_default(),
            cache.tracks.unwrap_or_default(),
            cache.playlists.unwrap_or_default(),
            cache
                .last_updated_unix_seconds
                .map_or_else(|| "unknown".to_owned(), |seconds| format!("Unix {seconds}"))
        )
    } else {
        format!("{path}; no readable local metadata cache")
    };
    report.push(
        "Library cache",
        if cache.readable {
            CheckStatus::Pass
        } else {
            CheckStatus::Warning
        },
        detail,
    );
    report.push(
        "Logging",
        CheckStatus::Pass,
        "stderr only; set RUST_LOG=apple_music_tui=debug for diagnostics",
    );
}

fn check_terminal_artwork(report: &mut DoctorReport) {
    let diagnostics = artwork::renderer_diagnostics();
    report.push(
        "Artwork TERM",
        CheckStatus::Pass,
        diagnostics.term.as_deref().unwrap_or("unset"),
    );
    report.push(
        "Artwork TERM_PROGRAM",
        CheckStatus::Pass,
        diagnostics.term_program.as_deref().unwrap_or("unset"),
    );
    report.push(
        "Artwork tmux",
        CheckStatus::Pass,
        if diagnostics.tmux { "yes" } else { "no" },
    );
    report.push(
        "Artwork outer terminal",
        if diagnostics.selection.outer_terminal().is_some() {
            CheckStatus::Pass
        } else {
            CheckStatus::Warning
        },
        diagnostics.selection.outer_terminal().unwrap_or("unknown"),
    );
    report.push(
        "Artwork renderer",
        CheckStatus::Pass,
        format!(
            "{} via {}",
            diagnostics.selection.protocol.label(),
            diagnostics.selection.source.label()
        ),
    );
    let passthrough = if diagnostics.selection.tmux_passthrough {
        "Kitty graphics are wrapped for tmux passthrough (tmux must enable allow-passthrough)"
    } else if diagnostics.tmux {
        "not active; set APPLE_MUSIC_TUI_ARTWORK_RENDERER=kitty when the outer terminal supports Kitty graphics"
    } else {
        "not applicable"
    };
    report.push("Artwork passthrough", CheckStatus::Pass, passthrough);
}

async fn check_platform_and_music_app(report: &mut DoctorReport) {
    if cfg!(target_os = "macos") {
        report.push("Platform", CheckStatus::Pass, "macOS");
    } else {
        report.push(
            "Platform",
            CheckStatus::Warning,
            "Music.app playback and Keychain authentication require macOS",
        );
    }

    let mut backend = MacOsMusicBackend::new();
    match backend.snapshot().await {
        Ok(snapshot) => match snapshot.availability {
            BackendAvailability::Available => report.push(
                "Music.app / Automation",
                CheckStatus::Pass,
                "Music.app is reachable",
            ),
            BackendAvailability::NotRunning => report.push(
                "Music.app / Automation",
                CheckStatus::Warning,
                "Music.app is installed but not running",
            ),
            BackendAvailability::Unavailable => report.push(
                "Music.app / Automation",
                CheckStatus::Warning,
                "Music.app is unavailable",
            ),
            BackendAvailability::PermissionDenied => report.push(
                "Music.app / Automation",
                CheckStatus::Failure,
                "permission denied; enable Music under the launching app in Privacy & Security → Automation",
            ),
            BackendAvailability::Error(message) => report.push(
                "Music.app / Automation",
                CheckStatus::Failure,
                safe_music_app_error(message),
            ),
        },
        Err(error) => report.push(
            "Music.app / Automation",
            CheckStatus::Failure,
            error.to_string(),
        ),
    }
}

fn safe_music_app_error(message: String) -> String {
    tracing::debug!(%message, "Music.app doctor query failed");
    "Music.app query failed; ensure Music.app is installed/running and Automation is allowed"
        .to_owned()
}

async fn check_authentication(report: &mut DoctorReport) {
    let path = match default_config_path() {
        Ok(path) => path,
        Err(error) => {
            report.push("Config path", CheckStatus::Failure, error.to_string());
            return;
        }
    };
    if !path.is_file() {
        report.push(
            "Apple config",
            CheckStatus::Warning,
            format!(
                "not found at {}; local playback remains available",
                path.display()
            ),
        );
        report.push(
            "User authorization",
            CheckStatus::Warning,
            "not checked; create config, then run `apple-music-tui auth`",
        );
        return;
    }
    report.push(
        "Apple config",
        CheckStatus::Pass,
        format!("found at {}", path.display()),
    );

    let config = match AppConfig::load(&path) {
        Ok(config) => config,
        Err(error) => {
            report.push("Config syntax", CheckStatus::Failure, error.to_string());
            return;
        }
    };
    let Some(settings) = config.apple.as_ref() else {
        report.push(
            "Apple configuration",
            CheckStatus::Warning,
            "[apple] section is missing",
        );
        return;
    };

    check_identifier(report, "Team ID", "team_id", settings.team_id.as_deref());
    check_identifier(report, "Key ID", "key_id", settings.key_id.as_deref());
    if let Some(storefront) = settings.storefront.as_deref() {
        match validate_storefront(storefront) {
            Ok(_) => report.push(
                "Configured storefront",
                CheckStatus::Pass,
                "valid country code",
            ),
            Err(error) => report.push(
                "Configured storefront",
                CheckStatus::Failure,
                error.to_string(),
            ),
        }
    } else {
        report.push(
            "Configured storefront",
            CheckStatus::Warning,
            "not set; the account storefront will be detected after authorization",
        );
    }

    let home = match home_directory() {
        Ok(home) => home,
        Err(error) => {
            report.push("Private key", CheckStatus::Failure, error.to_string());
            return;
        }
    };
    check_private_key(report, settings.private_key.as_deref(), &home);

    let apple = match config.validated_apple(&home) {
        Ok(Some(apple)) => apple,
        Ok(None) => return,
        Err(error) => {
            report.push(
                "Developer configuration",
                CheckStatus::Failure,
                error.to_string(),
            );
            return;
        }
    };
    let tokens = DeveloperTokenService::new(apple);
    let developer_token = match tokens.token(None) {
        Ok(token) => {
            report.push(
                "Developer Token generation",
                CheckStatus::Pass,
                "ES256 signing succeeded",
            );
            token
        }
        Err(error) => {
            report.push(
                "Developer Token generation",
                CheckStatus::Failure,
                error.to_string(),
            );
            return;
        }
    };

    let store = KeychainCredentialStore;
    let user_token = match store.load() {
        Ok(Some(token)) => {
            report.push(
                "Music User Token",
                CheckStatus::Pass,
                "present in macOS Keychain",
            );
            token
        }
        Ok(None) => {
            report.push(
                "Music User Token",
                CheckStatus::Warning,
                "not found; run `apple-music-tui auth`",
            );
            return;
        }
        Err(error) => {
            report.push("Music User Token", CheckStatus::Failure, error.to_string());
            return;
        }
    };

    let verifier = match AppleApiVerifier::new() {
        Ok(verifier) => verifier,
        Err(error) => {
            report.push("Apple API client", CheckStatus::Failure, error.to_string());
            return;
        }
    };
    match verifier.verify(&developer_token, &user_token).await {
        Ok(verification) => report.push(
            "Apple API authentication",
            CheckStatus::Pass,
            format!("verified for storefront {}", verification.storefront),
        ),
        Err(error) => report.push(
            "Apple API authentication",
            CheckStatus::Failure,
            error.to_string(),
        ),
    }
}

fn check_identifier(
    report: &mut DoctorReport,
    name: &'static str,
    field: &'static str,
    value: Option<&str>,
) {
    match validate_identifier(value, field) {
        Ok(_) => report.push(name, CheckStatus::Pass, "configured"),
        Err(error) => report.push(name, CheckStatus::Failure, error.to_string()),
    }
}

fn check_private_key(report: &mut DoctorReport, configured: Option<&str>, home: &Path) {
    let Some(configured) = configured.filter(|value| !value.trim().is_empty()) else {
        report.push(
            "Private key",
            CheckStatus::Failure,
            "private_key is missing",
        );
        return;
    };
    match expand_home(Path::new(configured), home).and_then(|path| {
        validate_private_key_path(&path)?;
        Ok(path)
    }) {
        Ok(_) => report.push("Private key", CheckStatus::Pass, "readable regular file"),
        Err(error) => report.push("Private key", CheckStatus::Failure, error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{CheckStatus, DoctorReport};

    #[test]
    fn warnings_and_failures_remain_distinct() {
        let mut report = DoctorReport::default();
        report.push(
            "missing optional auth",
            CheckStatus::Warning,
            "not configured",
        );
        assert!(!report.has_failures());

        report.push("bad key", CheckStatus::Failure, "invalid");
        assert!(report.has_failures());
        assert_eq!(report.checks[0].status, CheckStatus::Warning);
        assert_eq!(report.checks[1].status, CheckStatus::Failure);
    }

    #[test]
    fn release_diagnostics_expose_version_and_local_paths_without_secrets() {
        let mut report = DoctorReport::default();
        super::check_release_and_local_paths(&mut report);
        let text = format!("{:#?}", report.checks);
        assert!(text.contains(env!("CARGO_PKG_VERSION")));
        assert!(text.contains("Config path"));
        assert!(text.contains("Library cache"));
        assert!(!text.contains("Authorization: Bearer"));
    }

    #[test]
    fn doctor_sanitizes_automation_failure_detail() {
        let detail =
            super::safe_music_app_error("osascript error -1743: private detail".to_owned());
        assert!(detail.contains("Music.app query failed"));
        assert!(!detail.contains("-1743"));
        assert!(!detail.contains("private detail"));
    }
}
