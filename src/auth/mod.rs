mod api;
mod browser;
mod credential_store;
mod developer_token;

use std::{fmt, path::Path};

use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::config::{AppConfig, ConfigError, default_config_path, home_directory};

pub use api::{AppleApiVerifier, AuthVerifier, Verification};
pub use browser::BrowserAuthorization;
pub use credential_store::{CredentialStore, KeychainCredentialStore};
pub use developer_token::{DeveloperTokenProvider, DeveloperTokenService};

pub const KEYCHAIN_SERVICE: &str = "apple-music-tui";
pub const KEYCHAIN_ACCOUNT: &str = "music-user-token";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthStatus {
    NotConfigured,
    DeveloperTokenReady,
    AuthorizationRequired,
    CredentialsStored,
    Authenticated { storefront: String },
    ExpiredOrRevoked,
    Error(String),
}

impl AuthStatus {
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::NotConfigured => "Not configured".to_owned(),
            Self::DeveloperTokenReady => "Developer token ready".to_owned(),
            Self::AuthorizationRequired => "Authorization required".to_owned(),
            Self::CredentialsStored => "Credentials stored".to_owned(),
            Self::Authenticated { storefront } => format!("Authenticated ({storefront})"),
            Self::ExpiredOrRevoked => "Authorization expired or revoked".to_owned(),
            Self::Error(_) => "Authentication error".to_owned(),
        }
    }

    #[must_use]
    pub fn from_verification(result: &Result<Verification, AuthError>) -> Self {
        match result {
            Ok(verification) => Self::Authenticated {
                storefront: verification.storefront.clone(),
            },
            Err(AuthError::UserTokenRejected(_)) => Self::ExpiredOrRevoked,
            Err(error) => Self::Error(error.to_string()),
        }
    }
}

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretToken(String);

impl SecretToken {
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(value)
    }

    #[must_use]
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretToken([REDACTED])")
    }
}

impl fmt::Display for SecretToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

#[derive(Debug, Error)]
pub enum AuthError {
    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error("Apple developer credentials are not configured; create {0}")]
    NotConfigured(String),

    #[error("could not read the Apple private key: {0}")]
    PrivateKeyRead(#[source] std::io::Error),

    #[error("the Apple private key is not a valid ES256 PKCS#8 PEM key")]
    InvalidPrivateKey,

    #[error("could not sign the Apple developer token")]
    Signing,

    #[error("system clock is before the Unix epoch")]
    InvalidSystemClock,

    #[error("macOS Keychain is unavailable on this platform")]
    KeychainUnsupported,

    #[error("macOS Keychain operation failed: {0}")]
    Keychain(String),

    #[error("the Music User Token in Keychain is not valid UTF-8")]
    KeychainValueInvalid,

    #[error("Music User Token is not present in Keychain; run `apple-music-tui auth`")]
    UserTokenMissing,

    #[error("could not start the loopback authorization helper: {0}")]
    BrowserServer(#[source] std::io::Error),

    #[error("browser authorization timed out; run `apple-music-tui auth` again")]
    BrowserTimeout,

    #[error("browser authorization failed: {0}")]
    BrowserAuthorization(String),

    #[error("MusicKit authorization was cancelled or rejected; run `apple-music-tui auth` again")]
    BrowserRejected,

    #[error("Apple Music API request failed: {0}")]
    Network(String),

    #[error("Apple rejected the Developer Token (HTTP {0})")]
    DeveloperTokenRejected(u16),

    #[error(
        "Apple rejected or revoked the Music User Token (HTTP {0}); run `apple-music-tui auth` again"
    )]
    UserTokenRejected(u16),

    #[error("Apple Music API returned an unexpected response (HTTP {0})")]
    UnexpectedApiResponse(u16),
}

pub fn load_apple_config(path: &Path) -> Result<crate::config::AppleConfig, AuthError> {
    let config = AppConfig::load(path)?;
    let home = home_directory()?;
    config
        .validated_apple(&home)?
        .ok_or_else(|| AuthError::NotConfigured(path.display().to_string()))
}

#[must_use]
pub fn local_auth_status<S: CredentialStore>(store: &S) -> AuthStatus {
    let path = match default_config_path() {
        Ok(path) => path,
        Err(error) => return AuthStatus::Error(error.to_string()),
    };
    let config = match load_apple_config(&path) {
        Ok(config) => config,
        Err(AuthError::NotConfigured(_)) => return AuthStatus::NotConfigured,
        Err(error) => return AuthStatus::Error(error.to_string()),
    };
    if let Err(error) = DeveloperTokenService::new(config).token(None) {
        return AuthStatus::Error(error.to_string());
    }
    auth_status_from_store(store.load())
}

fn auth_status_from_store(result: Result<Option<SecretToken>, AuthError>) -> AuthStatus {
    match result {
        Ok(Some(_)) => AuthStatus::CredentialsStored,
        Ok(None) => AuthStatus::AuthorizationRequired,
        Err(error) => AuthStatus::Error(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthError, AuthStatus, SecretToken, Verification, auth_status_from_store};

    #[test]
    fn secret_tokens_are_redacted_from_debug_and_display() {
        let token = SecretToken::new("sensitive-value".to_owned());

        assert_eq!(token.to_string(), "[REDACTED]");
        assert_eq!(format!("{token:?}"), "SecretToken([REDACTED])");
        assert!(!format!("{token:?}").contains("sensitive-value"));
    }

    #[test]
    fn auth_status_is_not_collapsed_to_a_boolean() {
        assert_eq!(AuthStatus::NotConfigured.label(), "Not configured");
        assert_eq!(
            AuthStatus::AuthorizationRequired.label(),
            "Authorization required"
        );
        assert_eq!(
            AuthStatus::Authenticated {
                storefront: "de".to_owned()
            }
            .label(),
            "Authenticated (de)"
        );
    }

    #[test]
    fn credential_events_drive_auth_state_transitions() {
        assert_eq!(
            auth_status_from_store(Ok(None)),
            AuthStatus::AuthorizationRequired
        );
        assert_eq!(
            auth_status_from_store(Ok(Some(SecretToken::new("token".to_owned())))),
            AuthStatus::CredentialsStored
        );
        assert!(matches!(
            auth_status_from_store(Err(AuthError::Keychain("denied".to_owned()))),
            AuthStatus::Error(message) if message.contains("Keychain")
        ));
    }

    #[test]
    fn api_verification_drives_authenticated_and_revoked_states() {
        assert_eq!(
            AuthStatus::from_verification(&Ok(Verification {
                storefront: "de".to_owned(),
            })),
            AuthStatus::Authenticated {
                storefront: "de".to_owned()
            }
        );
        assert_eq!(
            AuthStatus::from_verification(&Err(AuthError::UserTokenRejected(403))),
            AuthStatus::ExpiredOrRevoked
        );
        assert!(matches!(
            AuthStatus::from_verification(&Err(AuthError::DeveloperTokenRejected(401))),
            AuthStatus::Error(message) if message.contains("Developer Token")
        ));
    }
}
