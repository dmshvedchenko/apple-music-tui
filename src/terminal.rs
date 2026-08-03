use std::{
    io::{Stdout, stdout},
    panic,
};

use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        cursor::{Hide, Show},
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
};

use crate::error::AppError;

pub type AppTerminal = Terminal<CrosstermBackend<Stdout>>;

pub struct TerminalSession {
    terminal: AppTerminal,
    active: bool,
}

impl TerminalSession {
    pub fn enter() -> Result<Self, AppError> {
        enable_raw_mode().map_err(AppError::Terminal)?;
        let mut output = stdout();
        if let Err(error) = execute!(output, EnterAlternateScreen, Hide) {
            let _ = restore_terminal();
            return Err(AppError::Terminal(error));
        }

        let backend = CrosstermBackend::new(output);
        let terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(error) => {
                let _ = restore_terminal();
                return Err(AppError::Terminal(error));
            }
        };

        Ok(Self {
            terminal,
            active: true,
        })
    }

    pub fn terminal_mut(&mut self) -> &mut AppTerminal {
        &mut self.terminal
    }

    pub fn restore(&mut self) -> Result<(), AppError> {
        if !self.active {
            return Ok(());
        }

        restore_terminal().map_err(AppError::Terminal)?;
        self.active = false;
        Ok(())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if self.active {
            let _ = restore_terminal();
        }
    }
}

pub fn restore_terminal() -> std::io::Result<()> {
    let mut output = stdout();
    let raw_mode_result = disable_raw_mode();
    let screen_result = execute!(output, LeaveAlternateScreen, Show);

    match (screen_result, raw_mode_result) {
        (Err(error), _) | (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
    }
}

pub fn install_panic_hook() {
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let _ = restore_terminal();
        original_hook(panic_info);
    }));
}
