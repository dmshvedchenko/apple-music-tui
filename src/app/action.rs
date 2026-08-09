use crate::{
    backend::{BackendCommand, BackendEvent},
    domain::{Artwork, ArtworkKey, TrackId},
};

use super::state::Screen;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    Quit,
    InputClosed,
    RequestPlaylistTrackRemoval,
    ConfirmPlaylistTrackRemoval,
    OpenNowPlaying,
    RefreshLibrary,
    OpenPlayer,
    MoveUp,
    MoveDown,
    JumpToStart,
    JumpToEnd,
    PageUp,
    PageDown,
    FocusLeft,
    FocusRight,
    OpenSelected,
    PlaySelectedCollection,
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
    OpenActions,
    OpenCollectionSort,
    ToggleCollectionSortDirection,
    StartCollectionFilter,
    CollectionFilterInput(char),
    CollectionFilterBackspace,
    SubmitCollectionFilter,
    ClearCollectionFilter,
    Back,
    Resize {
        width: u16,
        height: u16,
    },
    Backend(Box<BackendEvent>),
    ArtworkConversionCompleted {
        key: ArtworkKey,
        source_fingerprint: u64,
        result: Result<Artwork, String>,
    },
    RetryArtwork {
        key: ArtworkKey,
        track_id: TrackId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Backend(BackendCommand),
    ConvertArtwork {
        key: ArtworkKey,
        source_fingerprint: u64,
        source: Artwork,
    },
    RetryArtwork {
        key: ArtworkKey,
        track_id: TrackId,
    },
}
