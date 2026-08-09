#[cfg(test)]
use std::sync::Mutex;

use super::{AuthError, SecretToken};

#[cfg(target_os = "macos")]
use super::{KEYCHAIN_ACCOUNT, KEYCHAIN_SERVICE};

pub trait CredentialStore {
    fn load(&self) -> Result<Option<SecretToken>, AuthError>;
    fn save(&self, token: &SecretToken) -> Result<(), AuthError>;
    fn delete(&self) -> Result<bool, AuthError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct KeychainCredentialStore;

#[cfg(target_os = "macos")]
impl CredentialStore for KeychainCredentialStore {
    fn load(&self) -> Result<Option<SecretToken>, AuthError> {
        use security_framework::passwords::{PasswordOptions, generic_password};
        use zeroize::Zeroizing;

        match generic_password(PasswordOptions::new_generic_password(
            KEYCHAIN_SERVICE,
            KEYCHAIN_ACCOUNT,
        )) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(token) => Ok(Some(SecretToken::new(token))),
                Err(error) => {
                    let _invalid_secret = Zeroizing::new(error.into_bytes());
                    Err(AuthError::KeychainValueInvalid)
                }
            },
            Err(error) if error.code() == -25_300 => Ok(None),
            Err(error) => Err(AuthError::Keychain(error.to_string())),
        }
    }

    fn save(&self, token: &SecretToken) -> Result<(), AuthError> {
        security_framework::passwords::set_generic_password(
            KEYCHAIN_SERVICE,
            KEYCHAIN_ACCOUNT,
            token.expose().as_bytes(),
        )
        .map_err(|error| AuthError::Keychain(error.to_string()))
    }

    fn delete(&self) -> Result<bool, AuthError> {
        match security_framework::passwords::delete_generic_password(
            KEYCHAIN_SERVICE,
            KEYCHAIN_ACCOUNT,
        ) {
            Ok(()) => Ok(true),
            Err(error) if error.code() == -25_300 => Ok(false),
            Err(error) => Err(AuthError::Keychain(error.to_string())),
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl CredentialStore for KeychainCredentialStore {
    fn load(&self) -> Result<Option<SecretToken>, AuthError> {
        Err(AuthError::KeychainUnsupported)
    }

    fn save(&self, _token: &SecretToken) -> Result<(), AuthError> {
        Err(AuthError::KeychainUnsupported)
    }

    fn delete(&self) -> Result<bool, AuthError> {
        Err(AuthError::KeychainUnsupported)
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct MemoryCredentialStore {
    token: Mutex<Option<SecretToken>>,
}

#[cfg(test)]
impl CredentialStore for MemoryCredentialStore {
    fn load(&self) -> Result<Option<SecretToken>, AuthError> {
        Ok(self
            .token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone())
    }

    fn save(&self, token: &SecretToken) -> Result<(), AuthError> {
        *self
            .token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(token.clone());
        Ok(())
    }

    fn delete(&self) -> Result<bool, AuthError> {
        Ok(self
            .token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::{CredentialStore, MemoryCredentialStore};
    use crate::auth::SecretToken;

    #[test]
    fn fake_store_supports_missing_replace_and_delete() {
        let store = MemoryCredentialStore::default();
        assert!(store.load().expect("load").is_none());

        store
            .save(&SecretToken::new("first".to_owned()))
            .expect("save first");
        store
            .save(&SecretToken::new("replacement".to_owned()))
            .expect("replace");
        assert_eq!(
            store.load().expect("load").expect("token").expose(),
            "replacement"
        );
        assert!(store.delete().expect("delete"));
        assert!(!store.delete().expect("already absent"));
    }
}
