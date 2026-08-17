use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;

use crate::{
    app::{
        action::{Action, Command},
        reducer::reduce,
        state::AppState,
    },
    auth::AuthStatus,
    backend::{MusicBackend, is_interactive_command, spawn_worker},
    error::AppError,
    input::{map_collection_filter_key, map_key, map_search_key},
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
    let (artwork_sender, mut artwork_receiver) = mpsc::channel(8);
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancellation = InputCancellation(Arc::clone(&cancelled));

    let input_task = spawn_input_reader(input_sender, Arc::clone(&cancelled));
    let backend_task = spawn_worker(backend, command_receiver, backend_sender);
    let mut backend_open = true;
    let mut awaiting_second_g = false;
    let mut artwork_renderer = ui::artwork::InlineArtworkRenderer::default();

    loop {
        let render_started = Instant::now();
        let mut artwork_layout = None;
        terminal
            .draw(|frame| artwork_layout = ui::render_with_artwork_layout(frame, &state))
            .map_err(AppError::Terminal)?;
        artwork_renderer
            .present(terminal, &state, artwork_layout)
            .map_err(AppError::Terminal)?;
        tracing::debug!(
            render_ms = render_started.elapsed().as_secs_f64() * 1_000.0,
            "TUI frame timing"
        );

        let action = tokio::select! {
            input = input_receiver.recv() => {
                match input {
                    Some(Event::Key(key)) if state.help_open || state.action_menu.is_some() || state.sort_menu.is_some() => {
                        map_navigation_key(key, &mut awaiting_second_g)
                    }
                    Some(Event::Key(key)) if state.filter_editor.is_some() => {
                        awaiting_second_g = false;
                        map_collection_filter_key(key)
                    }
                    Some(Event::Key(key)) if state.search_input_active => {
                        awaiting_second_g = false;
                        map_search_key(key)
                    }
                    Some(Event::Key(key)) => map_navigation_key(key, &mut awaiting_second_g),
                    Some(Event::Resize(width, height)) => Some(Action::Resize { width, height }),
                    Some(_) => None,
                    None => Some(Action::InputClosed),
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
            artwork_event = artwork_receiver.recv() => artwork_event,
        };

        if let Some(action) = action {
            let action_started = Instant::now();
            let commands = reduce(&mut state, action);
            for command in commands {
                match command {
                    Command::Backend(command) => {
                        if is_interactive_command(&command) {
                            tracing::debug!(
                                ?command,
                                input_to_dispatch_ms =
                                    action_started.elapsed().as_secs_f64() * 1_000.0,
                                "interactive command dispatched"
                            );
                        }
                        if command_sender.send(command).await.is_err() {
                            state.notification =
                                Some("Backend worker stopped unexpectedly".to_owned());
                        }
                    }
                    Command::ConvertArtwork {
                        key,
                        source_fingerprint,
                        source,
                    } => {
                        let sender = artwork_sender.clone();
                        tokio::task::spawn_blocking(move || {
                            let result = ui::artwork::prepare_kitty_renderable(&source);
                            let _ = sender.blocking_send(Action::ArtworkConversionCompleted {
                                key,
                                source_fingerprint,
                                result,
                            });
                        });
                    }
                    Command::RetryArtwork { key, track_id } => {
                        let sender = artwork_sender.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_millis(150)).await;
                            let _ = sender.send(Action::RetryArtwork { key, track_id }).await;
                        });
                    }
                }
            }

            if state.should_quit {
                break;
            }
        }
    }

    cancellation.cancel();
    if state.stop_playback_on_exit {
        let _ = tokio::time::timeout(
            Duration::from_millis(1_500),
            command_sender.send(crate::backend::BackendCommand::Stop),
        )
        .await;
        let _ = tokio::time::timeout(Duration::from_millis(1_500), async {
            while let Some(event) = backend_receiver.recv().await {
                if matches!(
                    event,
                    crate::backend::BackendEvent::Update(
                        crate::backend::BackendUpdate::Stopped { .. }
                    ) | crate::backend::BackendEvent::Error(_)
                ) {
                    break;
                }
            }
        })
        .await;
    }
    drop(command_sender);
    artwork_renderer
        .clear(terminal)
        .map_err(AppError::Terminal)?;
    input_task.await??;
    backend_task.await?;
    Ok(())
}

fn map_navigation_key(key: KeyEvent, awaiting_second_g: &mut bool) -> Option<Action> {
    let is_g = key.code == KeyCode::Char('g')
        && !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
    if is_g {
        if *awaiting_second_g {
            *awaiting_second_g = false;
            return Some(Action::JumpToStart);
        }
        *awaiting_second_g = true;
        return None;
    }
    *awaiting_second_g = false;
    map_key(key)
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

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::app::action::Action;

    use super::map_navigation_key;

    #[test]
    fn gg_is_a_navigation_prefix_while_uppercase_g_jumps_to_the_end() {
        let mut awaiting_second_g = false;
        let g = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE);
        assert_eq!(map_navigation_key(g, &mut awaiting_second_g), None);
        assert!(awaiting_second_g);
        assert_eq!(
            map_navigation_key(g, &mut awaiting_second_g),
            Some(Action::JumpToStart)
        );
        assert!(!awaiting_second_g);
        assert_eq!(
            map_navigation_key(
                KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT),
                &mut awaiting_second_g,
            ),
            Some(Action::JumpToEnd)
        );
    }

    #[test]
    fn modal_navigation_mapping_keeps_quit_semantic_for_the_reducer_to_close() {
        let mut awaiting_second_g = false;
        assert_eq!(
            map_navigation_key(
                KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
                &mut awaiting_second_g,
            ),
            Some(Action::Quit)
        );
        assert_eq!(
            map_navigation_key(
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                &mut awaiting_second_g,
            ),
            Some(Action::Back)
        );
    }
}
