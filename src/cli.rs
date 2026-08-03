use std::{env, fmt};

use crate::error::AppError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendChoice {
    Auto,
    Mock,
    Macos,
    Apple,
}

impl BackendChoice {
    fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "auto" => Ok(Self::Auto),
            "mock" => Ok(Self::Mock),
            "macos" => Ok(Self::Macos),
            "apple" => Ok(Self::Apple),
            other => Err(AppError::InvalidArguments(format!(
                "unknown backend '{other}'; expected auto, mock, macos, or apple"
            ))),
        }
    }
}

impl fmt::Display for BackendChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Auto => "auto",
            Self::Mock => "mock",
            Self::Macos => "macos",
            Self::Apple => "apple",
        };
        formatter.write_str(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthCommand {
    Login,
    Status,
    Logout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CliAction {
    Run(BackendChoice),
    Auth(AuthCommand),
    Doctor,
    ConfigPath,
    Help,
    Version,
}

impl CliAction {
    pub fn parse_env() -> Result<Self, AppError> {
        Self::parse_from(env::args().skip(1))
    }

    pub fn parse_from<I, S>(arguments: I) -> Result<Self, AppError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut arguments = arguments.into_iter().map(Into::into).peekable();
        let mut backend = BackendChoice::Auto;

        if arguments.peek().map(String::as_str) == Some("auth") {
            arguments.next();
            let command = match arguments.next().as_deref() {
                None => AuthCommand::Login,
                Some("status") => AuthCommand::Status,
                Some("logout") => AuthCommand::Logout,
                Some(other) => {
                    return Err(AppError::InvalidArguments(format!(
                        "unknown auth command '{other}'; expected status or logout"
                    )));
                }
            };
            if let Some(extra) = arguments.next() {
                return Err(AppError::InvalidArguments(format!(
                    "unexpected argument '{extra}' after auth command"
                )));
            }
            return Ok(Self::Auth(command));
        }

        if arguments.peek().map(String::as_str) == Some("doctor") {
            arguments.next();
            reject_trailing(arguments)?;
            return Ok(Self::Doctor);
        }

        if arguments.peek().map(String::as_str) == Some("config-path") {
            arguments.next();
            reject_trailing(arguments)?;
            return Ok(Self::ConfigPath);
        }

        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "-h" | "--help" => return Ok(Self::Help),
                "-V" | "--version" | "version" => return Ok(Self::Version),
                "--backend" => {
                    let value = arguments.next().ok_or_else(|| {
                        AppError::InvalidArguments("--backend requires a value".to_owned())
                    })?;
                    backend = BackendChoice::parse(&value)?;
                }
                value if value.starts_with("--backend=") => {
                    let value = value.trim_start_matches("--backend=");
                    backend = BackendChoice::parse(value)?;
                }
                other => {
                    return Err(AppError::InvalidArguments(format!(
                        "unknown argument '{other}'"
                    )));
                }
            }
        }

        Ok(Self::Run(backend))
    }

    #[must_use]
    pub const fn help_text() -> &'static str {
        "apple-music-tui\n\nUSAGE:\n    apple-music-tui --backend <auto|mock|macos>\n    apple-music-tui auth [status|logout]\n    apple-music-tui doctor\n    apple-music-tui config-path\n\nOPTIONS:\n    --backend <auto|mock|macos|apple>\n    -h, --help\n    -V, --version\n\nOn macOS, auto selects the local Music.app playback backend.\nThe apple HTTP content backend is reserved for Milestone 4."
    }
}

fn reject_trailing<I>(mut arguments: I) -> Result<(), AppError>
where
    I: Iterator<Item = String>,
{
    if let Some(extra) = arguments.next() {
        Err(AppError::InvalidArguments(format!(
            "unexpected argument '{extra}'"
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthCommand, BackendChoice, CliAction};

    #[test]
    fn parses_mock_backend() {
        assert_eq!(
            CliAction::parse_from(["--backend", "mock"]).expect("valid arguments"),
            CliAction::Run(BackendChoice::Mock)
        );
    }

    #[test]
    fn parses_macos_and_auto_backends() {
        assert_eq!(
            CliAction::parse_from(["--backend=macos"]).expect("valid arguments"),
            CliAction::Run(BackendChoice::Macos)
        );
        assert_eq!(
            CliAction::parse_from(std::iter::empty::<&str>()).expect("valid arguments"),
            CliAction::Run(BackendChoice::Auto)
        );
    }

    #[test]
    fn rejects_unknown_arguments() {
        assert!(CliAction::parse_from(["--wat"]).is_err());
    }

    #[test]
    fn routes_auth_doctor_and_config_path_commands() {
        assert_eq!(
            CliAction::parse_from(["auth"]).expect("auth"),
            CliAction::Auth(AuthCommand::Login)
        );
        assert_eq!(
            CliAction::parse_from(["auth", "status"]).expect("auth status"),
            CliAction::Auth(AuthCommand::Status)
        );
        assert_eq!(
            CliAction::parse_from(["auth", "logout"]).expect("auth logout"),
            CliAction::Auth(AuthCommand::Logout)
        );
        assert_eq!(
            CliAction::parse_from(["doctor"]).expect("doctor"),
            CliAction::Doctor
        );
        assert_eq!(
            CliAction::parse_from(["config-path"]).expect("config path"),
            CliAction::ConfigPath
        );
    }

    #[test]
    fn rejects_unknown_auth_subcommands() {
        assert!(CliAction::parse_from(["auth", "wat"]).is_err());
    }
}
