use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;

use super::{AuthError, SecretToken};

const USER_STOREFRONT_URL: &str = "https://api.music.apple.com/v1/me/storefront";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Verification {
    pub storefront: String,
}

#[async_trait]
pub trait AuthVerifier {
    async fn verify(
        &self,
        developer_token: &SecretToken,
        user_token: &SecretToken,
    ) -> Result<Verification, AuthError>;
}

pub struct AppleApiVerifier {
    client: reqwest::Client,
}

impl AppleApiVerifier {
    pub fn new() -> Result<Self, AuthError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("apple-music-tui/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| AuthError::Network(error.to_string()))?;
        Ok(Self { client })
    }
}

#[derive(Deserialize)]
struct StorefrontResponse {
    data: Vec<StorefrontResource>,
}

#[derive(Deserialize)]
struct StorefrontResource {
    id: String,
}

#[async_trait]
impl AuthVerifier for AppleApiVerifier {
    async fn verify(
        &self,
        developer_token: &SecretToken,
        user_token: &SecretToken,
    ) -> Result<Verification, AuthError> {
        let response = self
            .client
            .get(USER_STOREFRONT_URL)
            .bearer_auth(developer_token.expose())
            .header("Music-User-Token", user_token.expose())
            .send()
            .await
            .map_err(|error| AuthError::Network(error.without_url().to_string()))?;
        let status = response.status();
        match status.as_u16() {
            200 => {
                let body = response
                    .bytes()
                    .await
                    .map_err(|error| AuthError::Network(error.without_url().to_string()))?;
                let response = serde_json::from_slice::<StorefrontResponse>(&body)
                    .map_err(|_| AuthError::UnexpectedApiResponse(200))?;
                let storefront = response
                    .data
                    .into_iter()
                    .next()
                    .map(|resource| resource.id)
                    .filter(|id| !id.is_empty())
                    .ok_or(AuthError::UnexpectedApiResponse(200))?;
                Ok(Verification { storefront })
            }
            401 => Err(AuthError::DeveloperTokenRejected(401)),
            403 => Err(AuthError::UserTokenRejected(403)),
            code => Err(AuthError::UnexpectedApiResponse(code)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Verification;

    #[test]
    fn verification_retains_account_storefront() {
        assert_eq!(
            Verification {
                storefront: "de".to_owned()
            }
            .storefront,
            "de"
        );
    }
}
