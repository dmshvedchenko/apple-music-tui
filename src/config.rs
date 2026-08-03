use std::{
    env,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use thiserror::Error;

pub const CONFIG_DIRECTORY: &str = "apple-music-tui";
pub const CONFIG_FILENAME: &str = "config.toml";

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub apple: Option<AppleSettings>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AppleSettings {
    pub team_id: Option<String>,
    pub key_id: Option<String>,
    pub private_key: Option<String>,
    pub storefront: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppleConfig {
    pub team_id: String,
    pub key_id: String,
    pub private_key: PathBuf,
    pub storefront: Option<String>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not determine the home directory for Apple Music configuration")]
    HomeDirectoryUnavailable,

    #[error("could not read configuration at {path}: {source}")]
    Read { path: PathBuf, source: io::Error },

    #[error("configuration at {path} is not valid TOML: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("Apple configuration field '{0}' is missing or empty")]
    MissingField(&'static str),

    #[error("Apple configuration field '{field}' is invalid: {message}")]
    InvalidField {
        field: &'static str,
        message: &'static str,
    },

    #[error("private key path uses unsupported home expansion: {0}")]
    UnsupportedHomeExpansion(String),

    #[error("private key does not exist at {0}")]
    PrivateKeyMissing(PathBuf),

    #[error("private key is not a readable file at {path}: {source}")]
    PrivateKeyUnreadable { path: PathBuf, source: io::Error },
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match fs::read_to_string(path) {
            Ok(contents) => Self::parse_at(&contents, path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Read {
                path: path.to_owned(),
                source,
            }),
        }
    }

    pub fn parse_at(contents: &str, path: &Path) -> Result<Self, ConfigError> {
        toml::from_str(contents).map_err(|source| ConfigError::Parse {
            path: path.to_owned(),
            source,
        })
    }

    pub fn validated_apple(&self, home: &Path) -> Result<Option<AppleConfig>, ConfigError> {
        self.apple
            .as_ref()
            .map(|settings| settings.validate(home))
            .transpose()
    }
}

impl AppleSettings {
    pub fn validate(&self, home: &Path) -> Result<AppleConfig, ConfigError> {
        let team_id = required_identifier(self.team_id.as_deref(), "team_id")?;
        let key_id = required_identifier(self.key_id.as_deref(), "key_id")?;
        let configured_path = required(self.private_key.as_deref(), "private_key")?;
        let private_key = expand_home(Path::new(configured_path), home)?;
        validate_private_key_path(&private_key)?;
        let storefront = self
            .storefront
            .as_deref()
            .map(validate_storefront)
            .transpose()?
            .map(str::to_owned);

        Ok(AppleConfig {
            team_id: team_id.to_owned(),
            key_id: key_id.to_owned(),
            private_key,
            storefront,
        })
    }
}

pub fn default_config_path() -> Result<PathBuf, ConfigError> {
    config_path_from(env::var_os("XDG_CONFIG_HOME"), env::var_os("HOME"))
}

pub fn home_directory() -> Result<PathBuf, ConfigError> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or(ConfigError::HomeDirectoryUnavailable)
}

fn config_path_from(
    xdg_config_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Result<PathBuf, ConfigError> {
    if let Some(path) = xdg_config_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return Ok(path.join(CONFIG_DIRECTORY).join(CONFIG_FILENAME));
    }

    home.filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|path| {
            path.join(".config")
                .join(CONFIG_DIRECTORY)
                .join(CONFIG_FILENAME)
        })
        .ok_or(ConfigError::HomeDirectoryUnavailable)
}

pub fn expand_home(path: &Path, home: &Path) -> Result<PathBuf, ConfigError> {
    let value = path.to_string_lossy();
    if value == "~" {
        return Ok(home.to_owned());
    }
    if let Some(relative) = value.strip_prefix("~/") {
        return Ok(home.join(relative));
    }
    if value.starts_with('~') {
        return Err(ConfigError::UnsupportedHomeExpansion(value.into_owned()));
    }
    Ok(path.to_owned())
}

pub fn validate_identifier<'a>(
    value: Option<&'a str>,
    field: &'static str,
) -> Result<&'a str, ConfigError> {
    let value = required(value, field)?;
    if value.len() != 10 || !value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(ConfigError::InvalidField {
            field,
            message: "expected the 10-character alphanumeric value from Apple Developer",
        });
    }
    Ok(value)
}

fn required_identifier<'a>(
    value: Option<&'a str>,
    field: &'static str,
) -> Result<&'a str, ConfigError> {
    validate_identifier(value, field)
}

fn required<'a>(value: Option<&'a str>, field: &'static str) -> Result<&'a str, ConfigError> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ConfigError::MissingField(field))
}

pub fn validate_private_key_path(path: &Path) -> Result<(), ConfigError> {
    if !path.exists() {
        return Err(ConfigError::PrivateKeyMissing(path.to_owned()));
    }
    if !path.is_file() {
        return Err(ConfigError::PrivateKeyUnreadable {
            path: path.to_owned(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "path is not a regular file"),
        });
    }
    File::open(path).map_err(|source| ConfigError::PrivateKeyUnreadable {
        path: path.to_owned(),
        source,
    })?;
    Ok(())
}

pub fn validate_storefront(value: &str) -> Result<&str, ConfigError> {
    let value = value.trim();
    if value.len() != 2 || !value.bytes().all(|byte| byte.is_ascii_lowercase()) {
        return Err(ConfigError::InvalidField {
            field: "storefront",
            message: "expected a lowercase ISO 3166-1 alpha-2 code such as 'de'",
        });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, path::Path};

    use super::{AppConfig, ConfigError, config_path_from, expand_home};

    #[test]
    fn parses_apple_configuration_without_private_key_contents() {
        let config = AppConfig::parse_at(
            r#"
                [apple]
                team_id = "ABCDEFGHIJ"
                key_id = "12345ABCDE"
                private_key = "~/.config/apple-music-tui/AuthKey_12345ABCDE.p8"
                storefront = "de"
            "#,
            Path::new("config.toml"),
        )
        .expect("valid config");

        let apple = config.apple.expect("apple section");
        assert_eq!(apple.team_id.as_deref(), Some("ABCDEFGHIJ"));
        assert_eq!(apple.key_id.as_deref(), Some("12345ABCDE"));
        assert_eq!(apple.storefront.as_deref(), Some("de"));
    }

    #[test]
    fn missing_apple_section_is_safe_and_unconfigured() {
        let config = AppConfig::parse_at("", Path::new("config.toml")).expect("empty config");

        assert!(config.apple.is_none());
        assert!(
            config
                .validated_apple(Path::new("/Users/example"))
                .expect("unconfigured")
                .is_none()
        );
    }

    #[test]
    fn partial_apple_configuration_reports_the_exact_missing_field() {
        let config = AppConfig::parse_at(
            "[apple]\nteam_id = \"ABCDEFGHIJ\"",
            Path::new("config.toml"),
        )
        .expect("partial config parses");

        assert!(matches!(
            config.validated_apple(Path::new("/Users/example")),
            Err(ConfigError::MissingField("key_id"))
        ));
    }

    #[test]
    fn expands_only_current_users_home_syntax() {
        let home = Path::new("/Users/example");

        assert_eq!(
            expand_home(Path::new("~/.config/key.p8"), home).expect("home expansion"),
            Path::new("/Users/example/.config/key.p8")
        );
        assert!(expand_home(Path::new("~someone/key.p8"), home).is_err());
    }

    #[test]
    fn config_path_prefers_absolute_xdg_then_home_dot_config() {
        assert_eq!(
            config_path_from(
                Some(OsString::from("/tmp/xdg")),
                Some(OsString::from("/Users/example")),
            )
            .expect("xdg path"),
            Path::new("/tmp/xdg/apple-music-tui/config.toml")
        );
        assert_eq!(
            config_path_from(None, Some(OsString::from("/Users/example"))).expect("home path"),
            Path::new("/Users/example/.config/apple-music-tui/config.toml")
        );
    }
}
