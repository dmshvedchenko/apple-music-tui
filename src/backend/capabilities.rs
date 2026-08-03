use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Capability {
    Launch,
    Playback,
    Seek,
    Volume,
    Mute,
    Shuffle,
    Repeat,
    Favorite,
    LibraryRead,
    PlaylistRead,
    SelectionPlayback,
    QueueRead,
    QueueWrite,
    QueueReorder,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Capabilities(BTreeSet<Capability>);

impl Capabilities {
    #[must_use]
    pub fn new(capabilities: impl IntoIterator<Item = Capability>) -> Self {
        Self(capabilities.into_iter().collect())
    }

    #[must_use]
    pub fn mock() -> Self {
        Self::new([
            Capability::Playback,
            Capability::Seek,
            Capability::Volume,
            Capability::Mute,
            Capability::Shuffle,
            Capability::Repeat,
            Capability::Favorite,
            Capability::LibraryRead,
            Capability::PlaylistRead,
            Capability::SelectionPlayback,
            Capability::QueueRead,
            Capability::QueueWrite,
            Capability::QueueReorder,
        ])
    }

    #[must_use]
    pub fn macos() -> Self {
        Self::new([
            Capability::Launch,
            Capability::Playback,
            Capability::Seek,
            Capability::Volume,
            Capability::Shuffle,
            Capability::Repeat,
            Capability::LibraryRead,
            Capability::PlaylistRead,
            Capability::SelectionPlayback,
        ])
    }

    #[must_use]
    pub fn supports(&self, capability: Capability) -> bool {
        self.0.contains(&capability)
    }
}
