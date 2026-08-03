use crate::backend::{BackendCommand, BackendEvent};

use super::state::Screen;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    Quit,
    OpenPlayer,
    MoveUp,
    MoveDown,
    FocusLeft,
    FocusRight,
    OpenSelected,
    PlaySelectedPlaylist,
    StartSearch,
    SearchInput(char),
    SearchBackspace,
    SubmitSearch,
    GoTo(Screen),
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
    Back,
    Resize { width: u16, height: u16 },
    Backend(Box<BackendEvent>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Backend(BackendCommand),
}
