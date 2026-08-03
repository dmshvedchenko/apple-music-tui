use std::{
    fs,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::config::AppleConfig;

use super::{AuthError, SecretToken};

const TOKEN_LIFETIME: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const REFRESH_MARGIN: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeveloperTokenClaims {
    pub iss: String,
    pub iat: u64,
    pub exp: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

struct CachedToken {
    origin: Option<String>,
    expires_at: u64,
    token: SecretToken,
}

pub trait DeveloperTokenProvider {
    fn token(&self, origin: Option<&str>) -> Result<SecretToken, AuthError>;
}

pub struct DeveloperTokenService {
    config: AppleConfig,
    cache: Mutex<Vec<CachedToken>>,
}

impl DeveloperTokenService {
    #[must_use]
    pub fn new(config: AppleConfig) -> Self {
        Self {
            config,
            cache: Mutex::new(Vec::new()),
        }
    }

    fn now() -> Result<u64, AuthError> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .map_err(|_| AuthError::InvalidSystemClock)
    }

    fn generate_at(&self, origin: Option<&str>, now: u64) -> Result<CachedToken, AuthError> {
        let key_bytes =
            Zeroizing::new(fs::read(&self.config.private_key).map_err(AuthError::PrivateKeyRead)?);
        let key = EncodingKey::from_ec_pem(&key_bytes).map_err(|_| AuthError::InvalidPrivateKey)?;
        let expires_at = now + TOKEN_LIFETIME.as_secs();
        let claims = DeveloperTokenClaims {
            iss: self.config.team_id.clone(),
            iat: now,
            exp: expires_at,
            origin: origin.map(str::to_owned),
        };
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(self.config.key_id.clone());
        header.typ = None;
        let token = encode(&header, &claims, &key).map_err(|_| AuthError::Signing)?;
        Ok(CachedToken {
            origin: origin.map(str::to_owned),
            expires_at,
            token: SecretToken::new(token),
        })
    }

    #[cfg(test)]
    fn token_at(&self, origin: Option<&str>, now: u64) -> Result<SecretToken, AuthError> {
        self.token_at_inner(origin, now)
    }

    fn token_at_inner(&self, origin: Option<&str>, now: u64) -> Result<SecretToken, AuthError> {
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(cached) = cache.iter().find(|cached| {
            cached.origin.as_deref() == origin
                && now.saturating_add(REFRESH_MARGIN.as_secs()) < cached.expires_at
        }) {
            return Ok(cached.token.clone());
        }

        let generated = self.generate_at(origin, now)?;
        let token = generated.token.clone();
        cache.retain(|cached| cached.origin.as_deref() != origin);
        cache.push(generated);
        Ok(token)
    }
}

impl DeveloperTokenProvider for DeveloperTokenService {
    fn token(&self, origin: Option<&str>) -> Result<SecretToken, AuthError> {
        self.token_at_inner(origin, Self::now()?)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use jsonwebtoken::{Algorithm, dangerous::insecure_decode, decode_header};

    use crate::config::AppleConfig;

    use super::{DeveloperTokenClaims, DeveloperTokenService};

    fn fixture_config() -> AppleConfig {
        AppleConfig {
            team_id: "ABCDEFGHIJ".to_owned(),
            key_id: "12345ABCDE".to_owned(),
            private_key: PathBuf::from("tests/fixtures/test_auth_private_key.pem"),
            storefront: Some("de".to_owned()),
        }
    }

    #[test]
    fn signs_es256_token_with_required_header_and_claims() {
        let service = DeveloperTokenService::new(fixture_config());
        let token = service
            .token_at(Some("http://127.0.0.1:4321"), 1_800_000_000)
            .expect("signed token");
        let header = decode_header(token.expose()).expect("header");
        let claims = insecure_decode::<DeveloperTokenClaims>(token.expose())
            .expect("claims")
            .claims;

        assert_eq!(header.alg, Algorithm::ES256);
        assert_eq!(header.kid.as_deref(), Some("12345ABCDE"));
        assert_eq!(claims.iss, "ABCDEFGHIJ");
        assert_eq!(claims.iat, 1_800_000_000);
        assert_eq!(claims.exp, 1_802_592_000);
        assert_eq!(claims.origin.as_deref(), Some("http://127.0.0.1:4321"));
    }

    #[test]
    fn caches_until_the_refresh_safety_margin() {
        let service = DeveloperTokenService::new(fixture_config());
        let first = service.token_at(None, 1_800_000_000).expect("first");
        let cached = service.token_at(None, 1_802_591_699).expect("cached");
        let refreshed = service.token_at(None, 1_802_591_700).expect("refreshed");

        assert_eq!(first.expose(), cached.expose());
        assert_ne!(first.expose(), refreshed.expose());
    }

    #[test]
    fn invalid_key_format_fails_without_exposing_contents() {
        let path = std::env::temp_dir().join(format!(
            "apple-music-tui-invalid-key-{}",
            std::process::id()
        ));
        fs::write(&path, "definitely-not-a-private-key").expect("write invalid fixture");
        let mut config = fixture_config();
        config.private_key = path.clone();

        let error = DeveloperTokenService::new(config)
            .token_at(None, 1_800_000_000)
            .expect_err("invalid key");

        assert_eq!(
            error.to_string(),
            "the Apple private key is not a valid ES256 PKCS#8 PEM key"
        );
        let _ = fs::remove_file(path);
    }
}
