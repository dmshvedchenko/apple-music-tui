use std::fmt;

#[cfg(target_os = "macos")]
use std::time::Duration;

use super::script::{ScriptRequest, build_script};

#[cfg(target_os = "macos")]
const AUTOMATION_TIMEOUT: Duration = Duration::from_secs(10);

pub trait AutomationRunner: Send + Sync {
    fn is_installed(&self) -> bool;
    fn run(&self, request: ScriptRequest) -> Result<String, AutomationError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemAutomationRunner;

impl AutomationRunner for SystemAutomationRunner {
    fn is_installed(&self) -> bool {
        music_app_is_installed()
    }

    fn run(&self, request: ScriptRequest) -> Result<String, AutomationError> {
        run_osascript(&build_script(&request))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AutomationError {
    #[cfg(not(target_os = "macos"))]
    UnsupportedPlatform,
    #[cfg(target_os = "macos")]
    Spawn(String),
    #[cfg(target_os = "macos")]
    Timeout,
    #[cfg(target_os = "macos")]
    Failed { code: Option<i32>, stderr: String },
    #[cfg(target_os = "macos")]
    InvalidUtf8,
}

impl fmt::Display for AutomationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(not(target_os = "macos"))]
            Self::UnsupportedPlatform => formatter.write_str("Music.app requires macOS"),
            #[cfg(target_os = "macos")]
            Self::Spawn(message) => write!(formatter, "could not start osascript: {message}"),
            #[cfg(target_os = "macos")]
            Self::Timeout => formatter.write_str("Music.app automation timed out"),
            #[cfg(target_os = "macos")]
            Self::Failed { code, stderr } => {
                write!(formatter, "osascript failed with status {code:?}: {stderr}")
            }
            #[cfg(target_os = "macos")]
            Self::InvalidUtf8 => formatter.write_str("osascript returned non-UTF-8 output"),
        }
    }
}

#[cfg(target_os = "macos")]
fn music_app_is_installed() -> bool {
    ["/System/Applications/Music.app", "/Applications/Music.app"]
        .iter()
        .any(|path| std::path::Path::new(path).is_dir())
}

#[cfg(not(target_os = "macos"))]
const fn music_app_is_installed() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn run_osascript(script: &str) -> Result<String, AutomationError> {
    use std::{
        io::Read,
        process::{Command, Stdio},
        thread,
        time::Instant,
    };

    let mut child = Command::new("/usr/bin/osascript")
        .args(["-l", "JavaScript", "-e", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| AutomationError::Spawn(error.to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AutomationError::Spawn("osascript stdout was not captured".to_owned()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AutomationError::Spawn("osascript stderr was not captured".to_owned()))?;
    let stdout_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut pipe = stdout;
        pipe.read_to_end(&mut bytes).map(|_| bytes)
    });
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut pipe = stderr;
        pipe.read_to_end(&mut bytes).map(|_| bytes)
    });
    let deadline = Instant::now() + AUTOMATION_TIMEOUT;

    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| AutomationError::Spawn(error.to_string()))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(AutomationError::Timeout);
        }
        thread::sleep(Duration::from_millis(20));
    };

    let stdout = stdout_reader
        .join()
        .map_err(|_| AutomationError::Spawn("osascript stdout reader panicked".to_owned()))?
        .map_err(|error| AutomationError::Spawn(error.to_string()))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| AutomationError::Spawn("osascript stderr reader panicked".to_owned()))?
        .map_err(|error| AutomationError::Spawn(error.to_string()))?;
    if !status.success() {
        return Err(AutomationError::Failed {
            code: status.code(),
            stderr: String::from_utf8_lossy(&stderr).trim().to_owned(),
        });
    }
    String::from_utf8(stdout).map_err(|_| AutomationError::InvalidUtf8)
}

#[cfg(not(target_os = "macos"))]
fn run_osascript(_script: &str) -> Result<String, AutomationError> {
    Err(AutomationError::UnsupportedPlatform)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{AutomationRunner, SystemAutomationRunner};
    use crate::backend::macos::{
        parser::parse_output,
        script::{ScriptRequest, build_script},
    };

    #[test]
    #[ignore = "requires a running local Music.app and Automation consent"]
    fn reads_live_music_state() {
        for request in [ScriptRequest::FullState, ScriptRequest::Poll] {
            eprintln!("{request:?} script:\n{}", build_script(&request));
            let output = SystemAutomationRunner
                .run(request.clone())
                .expect("Music.app automation response");
            eprintln!("{request:?} output:\n{output}");
            let state = parse_output(&output).expect("structured Music.app response");

            assert!(state.running, "Music.app must be running for this test");
        }
    }
}
