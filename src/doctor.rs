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
    check_platform_and_music_app(&mut report).await;
    check_authentication(&mut report).await;
    report
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
                format!("query failed: {message}"),
            ),
        },
        Err(error) => report.push(
            "Music.app / Automation",
            CheckStatus::Failure,
            error.to_string(),
        ),
    }
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
}
