use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ratatui::crossterm::event::{self, Event};
use tokio::sync::mpsc;

use crate::{
    app::{
        action::{Action, Command},
        reducer::reduce,
        state::AppState,
    },
    auth::AuthStatus,
    backend::{MusicBackend, spawn_worker},
    error::AppError,
    input::{map_key, map_search_key},
    terminal::AppTerminal,
    ui,
};

pub async fn run<B: MusicBackend>(
    terminal: &mut AppTerminal,
    backend: B,
    auth_status: AuthStatus,
) -> Result<(), AppError> {
    let terminal_size = ratatui::crossterm::terminal::size().map_err(AppError::Terminal)?;
    let mut state = AppState {
        terminal_size,
        auth_status,
        ..AppState::default()
    };

    let (input_sender, mut input_receiver) = mpsc::channel(32);
    let (command_sender, command_receiver) = mpsc::channel(32);
    let (backend_sender, mut backend_receiver) = mpsc::channel(32);
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancellation = InputCancellation(Arc::clone(&cancelled));

    let input_task = spawn_input_reader(input_sender, Arc::clone(&cancelled));
    let backend_task = spawn_worker(backend, command_receiver, backend_sender);
    let mut backend_open = true;

    loop {
        terminal
            .draw(|frame| ui::render(frame, &state))
            .map_err(AppError::Terminal)?;

        let action = tokio::select! {
            input = input_receiver.recv() => {
                match input {
                    Some(Event::Key(key)) if state.search_input_active => map_search_key(key),
                    Some(Event::Key(key)) => map_key(key),
                    Some(Event::Resize(width, height)) => Some(Action::Resize { width, height }),
                    Some(_) => None,
                    None => Some(Action::Quit),
                }
            }
            backend_event = backend_receiver.recv(), if backend_open => {
                match backend_event {
                    Some(event) => Some(Action::Backend(Box::new(event))),
                    None => {
                        backend_open = false;
                        Some(Action::Backend(Box::new(crate::backend::BackendEvent::Error(
                            "Backend worker stopped unexpectedly".to_owned(),
                        ))))
                    }
                }
            }
        };

        if let Some(action) = action {
            let commands = reduce(&mut state, action);
            for command in commands {
                match command {
                    Command::Backend(command) => {
                        if command_sender.send(command).await.is_err() {
                            state.notification =
                                Some("Backend worker stopped unexpectedly".to_owned());
                        }
                    }
                }
            }

            if state.should_quit {
                break;
            }
        }
    }

    cancellation.cancel();
    drop(command_sender);
    input_task.await??;
    backend_task.await?;
    Ok(())
}

struct InputCancellation(Arc<AtomicBool>);

impl InputCancellation {
    fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
}

impl Drop for InputCancellation {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn spawn_input_reader(
    sender: mpsc::Sender<Event>,
    cancelled: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<Result<(), AppError>> {
    tokio::task::spawn_blocking(move || {
        while !cancelled.load(Ordering::Acquire) {
            if event::poll(Duration::from_millis(100)).map_err(AppError::Input)? {
                let input = event::read().map_err(AppError::Input)?;
                if sender.blocking_send(input).is_err() {
                    break;
                }
            }
        }
        Ok(())
    })
}
