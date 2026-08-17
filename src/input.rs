use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::app::{action::Action, state::Screen};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingGroup {
    Navigation,
    Playback,
    General,
}

#[derive(Clone, Copy, Debug)]
pub struct KeyBinding {
    pub keys: &'static str,
    pub description: &'static str,
    pub group: BindingGroup,
    matches: &'static [KeyMatch],
    action: BindingAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeyMatch {
    Character(char),
    Code(KeyCode),
    Control(char),
    ScreenNumber,
}

impl KeyMatch {
    fn matches(self, event: &KeyEvent) -> bool {
        match self {
            Self::Character(character) => {
                event.code == KeyCode::Char(character)
                    && !event
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            }
            Self::Code(code) => event.code == code,
            Self::Control(character) => {
                event.code == KeyCode::Char(character)
                    && event.modifiers.contains(KeyModifiers::CONTROL)
            }
            Self::ScreenNumber => {
                matches!(event.code, KeyCode::Char('1'..='9'))
                    && !event
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BindingAction {
    Quit,
    OpenNowPlaying,
    RefreshLibrary,
    OpenPlayer,
    MoveUp,
    MoveDown,
    JumpToStart,
    JumpToEnd,
    JumpToPlayingTrack,
    PageUp,
    PageDown,
    FocusLeft,
    FocusRight,
    OpenSelected,
    PlaySelectedCollection,
    StartSearch,
    GoToNumberedScreen,
    Play,
    Pause,
    PlayPause,
    NextTrack,
    PreviousTrack,
    SeekBackward,
    SeekForward,
    VolumeDown,
    VolumeUp,
    ToggleMute,
    ToggleShuffle,
    CycleRepeat,
    ToggleFavorite,
    ToggleHelp,
    OpenActions,
    RequestPlaylistTrackRemoval,
    OpenCollectionSort,
    StartCollectionFilter,
    ClearCollectionFilter,
    Back,
}

impl BindingAction {
    fn to_action(self, event: &KeyEvent) -> Option<Action> {
        match self {
            Self::Quit => Some(Action::Quit),
            Self::OpenNowPlaying => Some(Action::OpenNowPlaying),
            Self::RefreshLibrary => Some(Action::RefreshLibrary),
            Self::OpenPlayer => Some(Action::OpenPlayer),
            Self::MoveUp => Some(Action::MoveUp),
            Self::MoveDown => Some(Action::MoveDown),
            Self::JumpToStart => Some(Action::JumpToStart),
            Self::JumpToEnd => Some(Action::JumpToEnd),
            Self::JumpToPlayingTrack => Some(Action::JumpToPlayingTrack),
            Self::PageUp => Some(Action::PageUp),
            Self::PageDown => Some(Action::PageDown),
            Self::FocusLeft => Some(Action::FocusLeft),
            Self::FocusRight => Some(Action::FocusRight),
            Self::OpenSelected => Some(Action::OpenSelected),
            Self::PlaySelectedCollection => Some(Action::PlaySelectedCollection),
            Self::StartSearch => Some(Action::StartSearch),
            Self::GoToNumberedScreen => {
                let KeyCode::Char(number) = event.code else {
                    return None;
                };
                let index = number.to_digit(10)?.checked_sub(1)? as usize;
                Screen::ALL.get(index).copied().map(Action::GoTo)
            }
            Self::Play => Some(Action::Play),
            Self::Pause => Some(Action::Pause),
            Self::PlayPause => Some(Action::PlayPause),
            Self::NextTrack => Some(Action::NextTrack),
            Self::PreviousTrack => Some(Action::PreviousTrack),
            Self::SeekBackward => Some(Action::SeekBackward),
            Self::SeekForward => Some(Action::SeekForward),
            Self::VolumeDown => Some(Action::VolumeDown),
            Self::VolumeUp => Some(Action::VolumeUp),
            Self::ToggleMute => Some(Action::ToggleMute),
            Self::ToggleShuffle => Some(Action::ToggleShuffle),
            Self::CycleRepeat => Some(Action::CycleRepeat),
            Self::ToggleFavorite => Some(Action::ToggleFavorite),
            Self::ToggleHelp => Some(Action::ToggleHelp),
            Self::OpenActions => Some(Action::OpenActions),
            Self::RequestPlaylistTrackRemoval => Some(Action::RequestPlaylistTrackRemoval),
            Self::OpenCollectionSort => Some(Action::OpenCollectionSort),
            Self::StartCollectionFilter => Some(Action::StartCollectionFilter),
            Self::ClearCollectionFilter => Some(Action::ClearCollectionFilter),
            Self::Back => Some(Action::Back),
        }
    }
}

const BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        keys: "j / ↓",
        description: "move selection down",
        group: BindingGroup::Navigation,
        matches: &[KeyMatch::Character('j'), KeyMatch::Code(KeyCode::Down)],
        action: BindingAction::MoveDown,
    },
    KeyBinding {
        keys: "k / ↑",
        description: "move selection up",
        group: BindingGroup::Navigation,
        matches: &[KeyMatch::Character('k'), KeyMatch::Code(KeyCode::Up)],
        action: BindingAction::MoveUp,
    },
    KeyBinding {
        keys: "gg / Home",
        description: "jump to first item",
        group: BindingGroup::Navigation,
        matches: &[KeyMatch::Character('g'), KeyMatch::Code(KeyCode::Home)],
        action: BindingAction::JumpToStart,
    },
    KeyBinding {
        keys: "G / End",
        description: "jump to last item",
        group: BindingGroup::Navigation,
        matches: &[KeyMatch::Character('G'), KeyMatch::Code(KeyCode::End)],
        action: BindingAction::JumpToEnd,
    },
    KeyBinding {
        keys: ".",
        description: "jump to playing playlist track",
        group: BindingGroup::Navigation,
        matches: &[KeyMatch::Character('.')],
        action: BindingAction::JumpToPlayingTrack,
    },
    KeyBinding {
        keys: "Ctrl-u / PgUp",
        description: "page up",
        group: BindingGroup::Navigation,
        matches: &[KeyMatch::Control('u'), KeyMatch::Code(KeyCode::PageUp)],
        action: BindingAction::PageUp,
    },
    KeyBinding {
        keys: "Ctrl-d / PgDn",
        description: "page down",
        group: BindingGroup::Navigation,
        matches: &[KeyMatch::Control('d'), KeyMatch::Code(KeyCode::PageDown)],
        action: BindingAction::PageDown,
    },
    KeyBinding {
        keys: "h / ←",
        description: "focus left / back",
        group: BindingGroup::Navigation,
        matches: &[KeyMatch::Character('h'), KeyMatch::Code(KeyCode::Left)],
        action: BindingAction::FocusLeft,
    },
    KeyBinding {
        keys: "l / →",
        description: "focus right",
        group: BindingGroup::Navigation,
        matches: &[KeyMatch::Character('l'), KeyMatch::Code(KeyCode::Right)],
        action: BindingAction::FocusRight,
    },
    KeyBinding {
        keys: "Enter",
        description: "open selected item",
        group: BindingGroup::Navigation,
        matches: &[KeyMatch::Code(KeyCode::Enter)],
        action: BindingAction::OpenSelected,
    },
    KeyBinding {
        keys: "P",
        description: "play selected album or playlist",
        group: BindingGroup::Playback,
        matches: &[KeyMatch::Character('P')],
        action: BindingAction::PlaySelectedCollection,
    },
    KeyBinding {
        keys: "1–9",
        description: "jump to screen",
        group: BindingGroup::Navigation,
        matches: &[KeyMatch::ScreenNumber],
        action: BindingAction::GoToNumberedScreen,
    },
    KeyBinding {
        keys: "/",
        description: "search local library",
        group: BindingGroup::Navigation,
        matches: &[KeyMatch::Character('/')],
        action: BindingAction::StartSearch,
    },
    KeyBinding {
        keys: "c",
        description: "play",
        group: BindingGroup::Playback,
        matches: &[KeyMatch::Character('c')],
        action: BindingAction::Play,
    },
    KeyBinding {
        keys: "x",
        description: "pause",
        group: BindingGroup::Playback,
        matches: &[KeyMatch::Character('x')],
        action: BindingAction::Pause,
    },
    KeyBinding {
        keys: "Space",
        description: "toggle play/pause",
        group: BindingGroup::Playback,
        matches: &[KeyMatch::Character(' ')],
        action: BindingAction::PlayPause,
    },
    KeyBinding {
        keys: "n",
        description: "next track",
        group: BindingGroup::Playback,
        matches: &[KeyMatch::Character('n')],
        action: BindingAction::NextTrack,
    },
    KeyBinding {
        keys: "p",
        description: "previous track",
        group: BindingGroup::Playback,
        matches: &[KeyMatch::Character('p')],
        action: BindingAction::PreviousTrack,
    },
    KeyBinding {
        keys: "[",
        description: "seek back 5 seconds",
        group: BindingGroup::Playback,
        matches: &[KeyMatch::Character('[')],
        action: BindingAction::SeekBackward,
    },
    KeyBinding {
        keys: "]",
        description: "seek forward 5 seconds",
        group: BindingGroup::Playback,
        matches: &[KeyMatch::Character(']')],
        action: BindingAction::SeekForward,
    },
    KeyBinding {
        keys: "-",
        description: "volume down",
        group: BindingGroup::Playback,
        matches: &[KeyMatch::Character('-')],
        action: BindingAction::VolumeDown,
    },
    KeyBinding {
        keys: "+ / =",
        description: "volume up",
        group: BindingGroup::Playback,
        matches: &[KeyMatch::Character('+'), KeyMatch::Character('=')],
        action: BindingAction::VolumeUp,
    },
    KeyBinding {
        keys: "m",
        description: "toggle mute",
        group: BindingGroup::Playback,
        matches: &[KeyMatch::Character('m')],
        action: BindingAction::ToggleMute,
    },
    KeyBinding {
        keys: "s",
        description: "toggle shuffle",
        group: BindingGroup::Playback,
        matches: &[KeyMatch::Character('s')],
        action: BindingAction::ToggleShuffle,
    },
    KeyBinding {
        keys: "N",
        description: "open Now Playing",
        group: BindingGroup::Navigation,
        matches: &[KeyMatch::Character('N')],
        action: BindingAction::OpenNowPlaying,
    },
    KeyBinding {
        keys: "R",
        description: "refresh local library",
        group: BindingGroup::Navigation,
        matches: &[KeyMatch::Character('R')],
        action: BindingAction::RefreshLibrary,
    },
    KeyBinding {
        keys: "S",
        description: "sort current library view",
        group: BindingGroup::Navigation,
        matches: &[KeyMatch::Character('S')],
        action: BindingAction::OpenCollectionSort,
    },
    KeyBinding {
        keys: "F",
        description: "filter current library view",
        group: BindingGroup::Navigation,
        matches: &[KeyMatch::Character('F')],
        action: BindingAction::StartCollectionFilter,
    },
    KeyBinding {
        keys: "Ctrl-l",
        description: "clear current library filter",
        group: BindingGroup::Navigation,
        matches: &[KeyMatch::Control('l')],
        action: BindingAction::ClearCollectionFilter,
    },
    KeyBinding {
        keys: "r",
        description: "cycle repeat",
        group: BindingGroup::Playback,
        matches: &[KeyMatch::Character('r')],
        action: BindingAction::CycleRepeat,
    },
    KeyBinding {
        keys: "f",
        description: "favorite current track",
        group: BindingGroup::Playback,
        matches: &[KeyMatch::Character('f')],
        action: BindingAction::ToggleFavorite,
    },
    KeyBinding {
        keys: "?",
        description: "toggle help",
        group: BindingGroup::General,
        matches: &[KeyMatch::Character('?')],
        action: BindingAction::ToggleHelp,
    },
    KeyBinding {
        keys: "a",
        description: "actions for selected item",
        group: BindingGroup::General,
        matches: &[KeyMatch::Character('a')],
        action: BindingAction::OpenActions,
    },
    KeyBinding {
        keys: "d",
        description: "remove selected playlist track",
        group: BindingGroup::General,
        matches: &[KeyMatch::Character('d')],
        action: BindingAction::RequestPlaylistTrackRemoval,
    },
    KeyBinding {
        keys: "o",
        description: "open local player",
        group: BindingGroup::General,
        matches: &[KeyMatch::Character('o')],
        action: BindingAction::OpenPlayer,
    },
    KeyBinding {
        keys: "Esc",
        description: "back / close overlay",
        group: BindingGroup::General,
        matches: &[KeyMatch::Code(KeyCode::Esc)],
        action: BindingAction::Back,
    },
    KeyBinding {
        keys: "q / Ctrl-C",
        description: "quit",
        group: BindingGroup::General,
        matches: &[KeyMatch::Character('q'), KeyMatch::Control('c')],
        action: BindingAction::Quit,
    },
];

#[must_use]
pub const fn bindings() -> &'static [KeyBinding] {
    BINDINGS
}

#[must_use]
pub fn map_key(event: KeyEvent) -> Option<Action> {
    if !matches!(event.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }

    BINDINGS
        .iter()
        .find(|binding| {
            binding
                .matches
                .iter()
                .any(|matcher| matcher.matches(&event))
        })
        .and_then(|binding| binding.action.to_action(&event))
}

#[must_use]
pub fn map_search_key(event: KeyEvent) -> Option<Action> {
    if !matches!(event.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }
    if event.code == KeyCode::Char('c') && event.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Action::Quit);
    }
    match event.code {
        KeyCode::Char('?') if event.modifiers == KeyModifiers::SHIFT => Some(Action::ToggleHelp),
        KeyCode::Esc => Some(Action::Back),
        KeyCode::Enter => Some(Action::SubmitSearch),
        KeyCode::Backspace => Some(Action::SearchBackspace),
        KeyCode::Char(character)
            if !event
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            Some(Action::SearchInput(character))
        }
        _ => None,
    }
}

#[must_use]
pub fn map_collection_filter_key(event: KeyEvent) -> Option<Action> {
    if !matches!(event.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }
    if event.code == KeyCode::Char('c') && event.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Action::Quit);
    }
    match event.code {
        KeyCode::Esc => Some(Action::Back),
        KeyCode::Enter => Some(Action::SubmitCollectionFilter),
        KeyCode::Backspace => Some(Action::CollectionFilterBackspace),
        KeyCode::Char('l') if event.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::ClearCollectionFilter)
        }
        KeyCode::Char(character)
            if !event
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            Some(Action::CollectionFilterInput(character))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    use crate::app::{action::Action, state::Screen};

    use super::{bindings, map_collection_filter_key, map_key, map_search_key};

    #[test]
    fn maps_keys_to_semantic_actions() {
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Action::OpenSelected)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)),
            Some(Action::PlayPause)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT)),
            Some(Action::RefreshLibrary)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT)),
            Some(Action::OpenNowPlaying)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)),
            Some(Action::Play)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            Some(Action::Pause)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE)),
            Some(Action::ToggleShuffle)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('8'), KeyModifiers::NONE)),
            Some(Action::GoTo(Screen::Songs))
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Some(Action::Back)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE)),
            Some(Action::FocusLeft)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
            Some(Action::MoveDown)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE)),
            Some(Action::MoveUp)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)),
            Some(Action::JumpToStart)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT)),
            Some(Action::JumpToEnd)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE)),
            Some(Action::JumpToPlayingTrack)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            Some(Action::PageDown)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)),
            Some(Action::PageUp)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(Action::Quit)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE)),
            Some(Action::ToggleMute)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE)),
            Some(Action::OpenPlayer)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
            Some(Action::OpenActions)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)),
            Some(Action::RequestPlaylistTrackRemoval)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT)),
            Some(Action::OpenCollectionSort)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('F'), KeyModifiers::SHIFT)),
            Some(Action::StartCollectionFilter)
        );
    }

    #[test]
    fn control_c_quits_without_shadowing_plain_c() {
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)),
            Some(Action::Play)
        );
    }

    #[test]
    fn search_mode_routes_text_editing_to_semantic_actions() {
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)),
            Some(Action::StartSearch)
        );
        assert_eq!(
            map_search_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(Action::SearchInput('q'))
        );
        assert_eq!(
            map_search_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::SHIFT)),
            Some(Action::ToggleHelp)
        );
        assert_eq!(
            map_search_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
            Some(Action::SearchBackspace)
        );
        assert_eq!(
            map_search_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Action::SubmitSearch)
        );
    }

    #[test]
    fn collection_filter_mode_keeps_text_editing_local() {
        assert_eq!(
            map_collection_filter_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(Action::CollectionFilterInput('q'))
        );
        assert_eq!(
            map_collection_filter_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
            Some(Action::CollectionFilterBackspace)
        );
        assert_eq!(
            map_collection_filter_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)),
            Some(Action::ClearCollectionFilter)
        );
    }

    #[test]
    fn key_release_is_ignored() {
        let event = KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        assert_eq!(map_key(event), None);
    }

    #[test]
    fn every_active_binding_has_help_metadata() {
        assert!(!bindings().is_empty());
        assert!(
            bindings()
                .iter()
                .all(|binding| { !binding.keys.is_empty() && !binding.description.is_empty() })
        );
    }
}
