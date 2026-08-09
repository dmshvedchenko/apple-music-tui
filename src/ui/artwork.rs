use std::io::{Cursor, IsTerminal, Write};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ratatui::layout::Rect;

use crate::{
    app::state::{AppState, ArtworkCacheEntry, RenderableArtworkCacheEntry, Route},
    domain::{Artwork, ArtworkKey, ArtworkMediaType},
    terminal::AppTerminal,
};

const MAX_INLINE_ARTWORK_BYTES: usize = 512 * 1024;
const MAX_SOURCE_ARTWORK_BYTES: usize = 2 * 1024 * 1024;
const MAX_RENDERABLE_ARTWORK_DIMENSION: u32 = 512;
/// A typical terminal cell is approximately twice as tall as it is wide.
const TERMINAL_CELL_WIDTH_TO_HEIGHT: f32 = 0.5;
const KITTY_IMAGE_ID: u32 = 715_045;
const KITTY_CHUNK_BYTES: usize = 4_096;
const PROBE_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ArtworkPlacement {
    x: u16,
    y: u16,
    columns: u16,
    rows: u16,
}

/// The final, border-excluded Ratatui rectangle assigned to inline artwork.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ArtworkLayout {
    pub(crate) key: ArtworkKey,
    pub(crate) area: Rect,
}

impl ArtworkLayout {
    #[must_use]
    pub(crate) fn new(key: ArtworkKey, area: Rect) -> Self {
        Self { key, area }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DisplayedArtwork {
    key: ArtworkKey,
    protocol: TerminalArtworkProtocol,
    placement: ArtworkPlacement,
    tmux_passthrough: bool,
}

#[derive(Default)]
pub(crate) struct InlineArtworkRenderer {
    displayed: Option<DisplayedArtwork>,
}

impl InlineArtworkRenderer {
    pub(crate) fn clear(&mut self, terminal: &mut AppTerminal) -> std::io::Result<()> {
        if let Some(displayed) = self.displayed.take() {
            match displayed.protocol {
                TerminalArtworkProtocol::Kitty => {
                    let command = maybe_tmux_passthrough(
                        kitty_delete_command(KITTY_IMAGE_ID),
                        displayed.tmux_passthrough,
                    );
                    terminal.backend_mut().write_all(command.as_bytes())?;
                    terminal.backend_mut().flush()?;
                }
                TerminalArtworkProtocol::ITerm2 => terminal.clear()?,
                TerminalArtworkProtocol::Sixel | TerminalArtworkProtocol::Unicode => {}
            }
        }
        Ok(())
    }

    pub(crate) fn present(
        &mut self,
        terminal: &mut AppTerminal,
        state: &AppState,
        layout: Option<ArtworkLayout>,
    ) -> std::io::Result<()> {
        let selection = detected_renderer();
        let desired = desired_artwork_with_placement(state, selection.protocol, layout.as_ref());
        let desired_display = desired.map(|(key, _, placement)| DisplayedArtwork {
            key: key.clone(),
            protocol: selection.protocol,
            placement,
            tmux_passthrough: selection.tmux_passthrough,
        });
        if self.displayed == desired_display {
            return Ok(());
        }
        self.clear(terminal)?;
        if let Some((_, artwork, placement)) = desired {
            let Some(command) = inline_command(selection.protocol, artwork, placement) else {
                return Ok(());
            };
            let command = maybe_tmux_passthrough(command, selection.tmux_passthrough);
            terminal.backend_mut().write_all(command.as_bytes())?;
            terminal.backend_mut().flush()?;
            tracing::debug!(
                renderer = selection.protocol.label(),
                source = desired_artwork(state, selection.protocol)
                    .and_then(|(key, _)| state.artwork_cache.get(key))
                    .and_then(|entry| match entry {
                        ArtworkCacheEntry::Ready(source) =>
                            Some(media_type_label(source.media_type)),
                        ArtworkCacheEntry::Loading
                        | ArtworkCacheEntry::Transient(_)
                        | ArtworkCacheEntry::Unavailable(_) => None,
                    })
                    .unwrap_or("unknown"),
                transmitted = media_type_label(artwork.media_type),
                conversion = if selection.protocol == TerminalArtworkProtocol::Kitty {
                    "cached"
                } else {
                    "not required"
                },
                passthrough = selection.tmux_passthrough,
                columns = placement.columns,
                rows = placement.rows,
                "inline artwork render attempted"
            );
            self.displayed = desired_display;
        }
        Ok(())
    }
}

fn desired_artwork_with_placement<'a>(
    state: &'a AppState,
    protocol: TerminalArtworkProtocol,
    layout: Option<&ArtworkLayout>,
) -> Option<(&'a ArtworkKey, &'a Artwork, ArtworkPlacement)> {
    let (key, artwork) = desired_artwork(state, protocol)?;
    let layout = layout.filter(|layout| layout.key == *key)?;
    let placement = fit_artwork_placement(artwork, layout.area, TERMINAL_CELL_WIDTH_TO_HEIGHT)?;
    can_render_inline(protocol, artwork).then_some((key, artwork, placement))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalArtworkProtocol {
    Kitty,
    ITerm2,
    Sixel,
    Unicode,
}

pub const ARTWORK_RENDERER_ENV: &str = "APPLE_MUSIC_TUI_ARTWORK_RENDERER";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtworkRendererOverride {
    Auto,
    Kitty,
    Unicode,
}

impl ArtworkRendererOverride {
    fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("kitty") => Self::Kitty,
            Some("unicode") => Self::Unicode,
            Some("auto") | None => Self::Auto,
            Some(_) => Self::Auto,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtworkRendererSource {
    ExplicitOverride,
    KittyWindow,
    Ghostty,
    ITerm2,
    SixelTerm,
    OuterTerminalUnknown,
    Fallback,
}

impl ArtworkRendererSource {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ExplicitOverride => "explicit override",
            Self::KittyWindow => "KITTY_WINDOW_ID",
            Self::Ghostty => "TERM_PROGRAM=ghostty",
            Self::ITerm2 => "TERM_PROGRAM=iTerm.app",
            Self::SixelTerm => "TERM contains sixel",
            Self::OuterTerminalUnknown => "auto: outer terminal unknown inside tmux",
            Self::Fallback => "auto fallback",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtworkRendererSelection {
    pub protocol: TerminalArtworkProtocol,
    pub source: ArtworkRendererSource,
    pub tmux_passthrough: bool,
}

impl ArtworkRendererSelection {
    #[must_use]
    pub const fn outer_terminal(self) -> Option<&'static str> {
        match self.source {
            ArtworkRendererSource::KittyWindow => Some("Kitty-compatible terminal"),
            ArtworkRendererSource::Ghostty => Some("Ghostty"),
            ArtworkRendererSource::ITerm2 => Some("iTerm2"),
            ArtworkRendererSource::SixelTerm => Some("Sixel terminal"),
            ArtworkRendererSource::ExplicitOverride
            | ArtworkRendererSource::OuterTerminalUnknown
            | ArtworkRendererSource::Fallback => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtworkRendererDiagnostics {
    pub term: Option<String>,
    pub term_program: Option<String>,
    pub tmux: bool,
    pub tmux_value: Option<String>,
    pub selection: ArtworkRendererSelection,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtworkProbeReport {
    pub selection: ArtworkRendererSelection,
    pub tmux: bool,
    pub tmux_value: Option<String>,
    pub source_format: &'static str,
    pub transmitted_format: &'static str,
    pub payload_bytes: usize,
    pub base64_bytes: usize,
    pub chunk_count: usize,
    pub image_id: Option<u32>,
    pub placement_columns: Option<u16>,
    pub placement_rows: Option<u16>,
    pub total_bytes_written: usize,
    pub flush_succeeded: bool,
    pub stdout_is_tty: bool,
    pub stderr_is_tty: bool,
    first_bytes: String,
    last_bytes: String,
}

impl ArtworkProbeReport {
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("selected renderer: {}", self.selection.protocol.label()),
            format!("tmux: {}", yes_no(self.tmux)),
            format!("source format: {}", self.source_format),
            format!("transmitted format: {}", self.transmitted_format),
            format!("payload bytes: {}", self.payload_bytes),
            format!("base64 bytes: {}", self.base64_bytes),
            format!("chunk count: {}", self.chunk_count),
            format!(
                "image ID: {}",
                self.image_id
                    .map_or_else(|| "none (minimal)".to_owned(), |id| id.to_string())
            ),
            format!(
                "placement dimensions: {}",
                match (self.placement_columns, self.placement_rows) {
                    (Some(columns), Some(rows)) => format!("{columns}x{rows} cells"),
                    _ => "terminal default (minimal)".to_owned(),
                }
            ),
            format!("total graphics bytes written: {}", self.total_bytes_written),
            format!(
                "flush result: {}",
                if self.flush_succeeded {
                    "ok"
                } else {
                    "not attempted"
                }
            ),
            format!("isatty(stdout): {}", yes_no(self.stdout_is_tty)),
            format!("isatty(stderr): {}", yes_no(self.stderr_is_tty)),
        ];
        if artwork_debug_diagnostics_enabled() {
            lines.push(format!(
                "TMUX raw: {}",
                self.tmux_value.as_deref().unwrap_or("unset")
            ));
            lines.push(format!("graphics first bytes: {}", self.first_bytes));
            lines.push(format!("graphics last bytes: {}", self.last_bytes));
        }
        lines
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RendererEnvironment<'a> {
    pub kitty_window_id: Option<&'a str>,
    pub term_program: Option<&'a str>,
    pub term: Option<&'a str>,
    pub tmux: Option<&'a str>,
    pub override_renderer: Option<&'a str>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ProcessTerminalEnvironment {
    kitty_window_id: Option<String>,
    term_program: Option<String>,
    term: Option<String>,
    tmux: Option<String>,
    override_renderer: Option<String>,
}

impl ProcessTerminalEnvironment {
    fn capture() -> Self {
        Self {
            kitty_window_id: std::env::var("KITTY_WINDOW_ID").ok(),
            term_program: std::env::var("TERM_PROGRAM").ok(),
            term: std::env::var("TERM").ok(),
            tmux: std::env::var("TMUX").ok(),
            override_renderer: std::env::var(ARTWORK_RENDERER_ENV).ok(),
        }
    }

    fn renderer_environment(&self) -> RendererEnvironment<'_> {
        RendererEnvironment {
            kitty_window_id: self.kitty_window_id.as_deref(),
            term_program: self.term_program.as_deref(),
            term: self.term.as_deref(),
            tmux: self.tmux.as_deref(),
            override_renderer: self.override_renderer.as_deref(),
        }
    }

    fn tmux_active(&self) -> bool {
        self.tmux.as_deref().is_some_and(|value| !value.is_empty())
    }
}

impl TerminalArtworkProtocol {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Kitty => "Kitty",
            Self::ITerm2 => "iTerm2",
            Self::Sixel => "Sixel",
            Self::Unicode => "Unicode fallback",
        }
    }
}

#[must_use]
pub fn detected_protocol() -> TerminalArtworkProtocol {
    detected_renderer().protocol
}

#[must_use]
pub fn detected_renderer() -> ArtworkRendererSelection {
    let environment = ProcessTerminalEnvironment::capture();
    select_renderer(environment.renderer_environment())
}

#[must_use]
pub fn renderer_diagnostics() -> ArtworkRendererDiagnostics {
    renderer_diagnostics_for(ProcessTerminalEnvironment::capture())
}

fn renderer_diagnostics_for(environment: ProcessTerminalEnvironment) -> ArtworkRendererDiagnostics {
    let selection = select_renderer(environment.renderer_environment());
    let tmux = environment.tmux_active();
    ArtworkRendererDiagnostics {
        term: environment.term,
        term_program: environment.term_program,
        tmux,
        tmux_value: environment.tmux,
        selection,
    }
}

#[must_use]
pub fn select_renderer(environment: RendererEnvironment<'_>) -> ArtworkRendererSelection {
    let tmux = environment.tmux.is_some_and(|value| !value.is_empty());
    let (protocol, source) = match ArtworkRendererOverride::parse(environment.override_renderer) {
        ArtworkRendererOverride::Kitty => (
            TerminalArtworkProtocol::Kitty,
            ArtworkRendererSource::ExplicitOverride,
        ),
        ArtworkRendererOverride::Unicode => (
            TerminalArtworkProtocol::Unicode,
            ArtworkRendererSource::ExplicitOverride,
        ),
        ArtworkRendererOverride::Auto => {
            if environment
                .kitty_window_id
                .is_some_and(|value| !value.is_empty())
            {
                (
                    TerminalArtworkProtocol::Kitty,
                    ArtworkRendererSource::KittyWindow,
                )
            } else if environment
                .term_program
                .is_some_and(|value| value.eq_ignore_ascii_case("ghostty"))
            {
                (
                    TerminalArtworkProtocol::Kitty,
                    ArtworkRendererSource::Ghostty,
                )
            } else if environment
                .term_program
                .is_some_and(|value| value.eq_ignore_ascii_case("iTerm.app"))
            {
                (
                    TerminalArtworkProtocol::ITerm2,
                    ArtworkRendererSource::ITerm2,
                )
            } else if environment
                .term
                .is_some_and(|value| value.to_ascii_lowercase().contains("sixel"))
            {
                (
                    TerminalArtworkProtocol::Sixel,
                    ArtworkRendererSource::SixelTerm,
                )
            } else if tmux {
                (
                    TerminalArtworkProtocol::Unicode,
                    ArtworkRendererSource::OuterTerminalUnknown,
                )
            } else {
                (
                    TerminalArtworkProtocol::Unicode,
                    ArtworkRendererSource::Fallback,
                )
            }
        }
    };
    ArtworkRendererSelection {
        protocol,
        source,
        tmux_passthrough: tmux && protocol == TerminalArtworkProtocol::Kitty,
    }
}

pub fn write_kitty_probe(writer: &mut impl Write) -> std::io::Result<ArtworkProbeReport> {
    let diagnostics = renderer_diagnostics();
    let source = STANDARD
        .decode(PROBE_PNG_BASE64)
        .expect("the built-in Kitty probe PNG must be valid base64");
    let encoded = STANDARD.encode(&source);
    let mut report = ArtworkProbeReport {
        selection: diagnostics.selection,
        tmux: diagnostics.tmux,
        tmux_value: diagnostics.tmux_value,
        source_format: "PNG",
        transmitted_format: "not sent (Kitty renderer required)",
        payload_bytes: source.len(),
        base64_bytes: encoded.len(),
        chunk_count: encoded.len().div_ceil(KITTY_CHUNK_BYTES),
        image_id: None,
        placement_columns: None,
        placement_rows: None,
        total_bytes_written: 0,
        flush_succeeded: false,
        stdout_is_tty: std::io::stdout().is_terminal(),
        stderr_is_tty: std::io::stderr().is_terminal(),
        first_bytes: String::new(),
        last_bytes: String::new(),
    };
    if diagnostics.selection.protocol != TerminalArtworkProtocol::Kitty {
        return Ok(report);
    }

    let command = maybe_tmux_passthrough(
        minimal_kitty_png_command(&source),
        diagnostics.selection.tmux_passthrough,
    );
    writer.write_all(command.as_bytes())?;
    writer.flush()?;

    report.transmitted_format = "PNG (f=100, a=T; t=d default)";
    report.total_bytes_written = command.len();
    report.flush_succeeded = true;
    report.first_bytes = escaped_hex(command.as_bytes(), true);
    report.last_bytes = escaped_hex(command.as_bytes(), false);
    Ok(report)
}

const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn artwork_debug_diagnostics_enabled() -> bool {
    tracing::enabled!(target: "apple_music_tui::ui::artwork", tracing::Level::DEBUG)
}

fn escaped_hex(bytes: &[u8], first: bool) -> String {
    const DIAGNOSTIC_BYTES: usize = 16;
    let slice = if first {
        &bytes[..bytes.len().min(DIAGNOSTIC_BYTES)]
    } else {
        &bytes[bytes.len().saturating_sub(DIAGNOSTIC_BYTES)..]
    };
    slice
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[must_use]
pub const fn media_type_label(media_type: ArtworkMediaType) -> &'static str {
    match media_type {
        ArtworkMediaType::Jpeg => "JPEG",
        ArtworkMediaType::Png => "PNG",
        ArtworkMediaType::Gif => "GIF",
        ArtworkMediaType::Unknown => "unknown format",
    }
}

#[must_use]
pub fn can_render_inline(protocol: TerminalArtworkProtocol, artwork: &Artwork) -> bool {
    if artwork.bytes.is_empty() || artwork.bytes.len() > MAX_INLINE_ARTWORK_BYTES {
        return false;
    }
    match protocol {
        TerminalArtworkProtocol::Kitty => artwork.media_type == ArtworkMediaType::Png,
        TerminalArtworkProtocol::ITerm2 => artwork.media_type != ArtworkMediaType::Unknown,
        TerminalArtworkProtocol::Sixel | TerminalArtworkProtocol::Unicode => false,
    }
}

fn desired_artwork(
    state: &AppState,
    protocol: TerminalArtworkProtocol,
) -> Option<(&ArtworkKey, &Artwork)> {
    let key = match &state.navigation.active {
        Route::AlbumDetail { album_id } => ArtworkKey::Album(album_id.clone()),
        _ => state.artwork_key_for_track(&state.playback.current_track.as_ref()?.id),
    };
    let key = state
        .artwork_cache
        .keys()
        .find(|candidate| **candidate == key)?;
    match state.artwork_cache.get(key)? {
        ArtworkCacheEntry::Ready(source) if protocol == TerminalArtworkProtocol::Kitty => {
            let source_fingerprint = source.fingerprint();
            match state.renderable_artwork_cache.get(key)? {
                RenderableArtworkCacheEntry::Ready {
                    source_fingerprint: cached_fingerprint,
                    artwork,
                } if *cached_fingerprint == source_fingerprint => Some((key, artwork)),
                RenderableArtworkCacheEntry::Loading { .. }
                | RenderableArtworkCacheEntry::Unavailable { .. }
                | RenderableArtworkCacheEntry::Ready { .. } => None,
            }
        }
        ArtworkCacheEntry::Ready(artwork) => Some((key, artwork)),
        ArtworkCacheEntry::Loading
        | ArtworkCacheEntry::Transient(_)
        | ArtworkCacheEntry::Unavailable(_) => None,
    }
}

/// Produces the Kitty-compatible asset from a Music.app source image.
///
/// PNG is deliberately passed through as-is. JPEG is decoded and bounded to a
/// 512px square before PNG encoding; GIF remains unsupported by Kitty here.
pub fn prepare_kitty_renderable(source: &Artwork) -> Result<Artwork, String> {
    if source.bytes.is_empty() {
        return Err("Artwork source is empty".to_owned());
    }
    if source.bytes.len() > MAX_SOURCE_ARTWORK_BYTES {
        return Err(format!(
            "Artwork source exceeds the {} MiB Music.app limit",
            MAX_SOURCE_ARTWORK_BYTES / (1024 * 1024)
        ));
    }
    match source.media_type {
        ArtworkMediaType::Png => Ok(source.clone()),
        ArtworkMediaType::Jpeg => {
            let image =
                image::load_from_memory_with_format(&source.bytes, image::ImageFormat::Jpeg)
                    .map_err(|error| format!("JPEG artwork decode failed: {error}"))?;
            let bounded = image.thumbnail(
                MAX_RENDERABLE_ARTWORK_DIMENSION,
                MAX_RENDERABLE_ARTWORK_DIMENSION,
            );
            let mut encoded = Cursor::new(Vec::new());
            bounded
                .write_to(&mut encoded, image::ImageFormat::Png)
                .map_err(|error| format!("PNG artwork encode failed: {error}"))?;
            let bytes = encoded.into_inner();
            if bytes.len() > MAX_INLINE_ARTWORK_BYTES {
                return Err(format!(
                    "Converted artwork exceeds the {} KiB inline limit",
                    MAX_INLINE_ARTWORK_BYTES / 1024
                ));
            }
            Ok(Artwork {
                media_type: ArtworkMediaType::Png,
                bytes,
            })
        }
        ArtworkMediaType::Gif => {
            Err("GIF artwork is not supported by the Kitty renderer".to_owned())
        }
        ArtworkMediaType::Unknown => {
            Err("Artwork format is not supported by the Kitty renderer".to_owned())
        }
    }
}

fn artwork_placement(area: Rect) -> Option<ArtworkPlacement> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    Some(ArtworkPlacement {
        x: area.x,
        y: area.y,
        columns: area.width,
        rows: area.height,
    })
}

fn fit_artwork_placement(
    artwork: &Artwork,
    target: Rect,
    cell_width_to_height: f32,
) -> Option<ArtworkPlacement> {
    let fitted = png_dimensions(&artwork.bytes)
        .and_then(|(source_width, source_height)| {
            fit_artwork_rect(source_width, source_height, target, cell_width_to_height)
        })
        .unwrap_or(target);
    artwork_placement(fitted)
}

fn fit_artwork_rect(
    source_width: u32,
    source_height: u32,
    target: Rect,
    cell_width_to_height: f32,
) -> Option<Rect> {
    if source_width == 0
        || source_height == 0
        || target.width == 0
        || target.height == 0
        || cell_width_to_height <= 0.0
    {
        return None;
    }
    let source_aspect = source_width as f32 / source_height as f32;
    let target_aspect = target.width as f32 * cell_width_to_height / target.height as f32;
    let (width, height) = if source_aspect >= target_aspect {
        let width = target.width;
        let height = ((width as f32 * cell_width_to_height / source_aspect).round() as u16)
            .clamp(1, target.height);
        (width, height)
    } else {
        let height = target.height;
        let width = ((height as f32 * source_aspect / cell_width_to_height).round() as u16)
            .clamp(1, target.width);
        (width, height)
    };
    Some(Rect::new(
        target.x + (target.width - width) / 2,
        target.y + (target.height - height) / 2,
        width,
        height,
    ))
}

fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const PNG_HEADER: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || !bytes.starts_with(PNG_HEADER) || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    (width > 0 && height > 0).then_some((width, height))
}

#[must_use]
pub(crate) fn inline_artwork_is_ready(state: &AppState, key: &ArtworkKey) -> bool {
    inline_artwork_is_ready_for_protocol(state, key, detected_protocol())
}

#[must_use]
pub(crate) fn inline_artwork_is_ready_for_protocol(
    state: &AppState,
    key: &ArtworkKey,
    protocol: TerminalArtworkProtocol,
) -> bool {
    match state.artwork_cache.get(key) {
        Some(ArtworkCacheEntry::Ready(source)) if protocol == TerminalArtworkProtocol::Kitty => {
            matches!(
                state.renderable_artwork_cache.get(key),
                Some(RenderableArtworkCacheEntry::Ready {
                    source_fingerprint,
                    artwork,
                }) if *source_fingerprint == source.fingerprint()
                    && can_render_inline(protocol, artwork)
            )
        }
        Some(ArtworkCacheEntry::Ready(artwork)) => can_render_inline(protocol, artwork),
        Some(
            ArtworkCacheEntry::Loading
            | ArtworkCacheEntry::Transient(_)
            | ArtworkCacheEntry::Unavailable(_),
        )
        | None => false,
    }
}

fn inline_command(
    protocol: TerminalArtworkProtocol,
    artwork: &Artwork,
    placement: ArtworkPlacement,
) -> Option<String> {
    if !can_render_inline(protocol, artwork) {
        return None;
    }
    let cursor = format!("\u{1b}7\u{1b}[{};{}H", placement.y + 1, placement.x + 1);
    let restore = "\u{1b}8";
    match protocol {
        TerminalArtworkProtocol::ITerm2 => {
            let encoded = STANDARD.encode(&artwork.bytes);
            Some(format!(
                "{cursor}\u{1b}]1337;File=name=YXJ0d29yaw==;inline=1;width={};height={};preserveAspectRatio=1:{encoded}\u{7}{restore}",
                placement.columns, placement.rows
            ))
        }
        TerminalArtworkProtocol::Kitty => {
            let command = kitty_command(KITTY_IMAGE_ID, &artwork.bytes, placement);
            Some(format!("{cursor}{command}{restore}"))
        }
        TerminalArtworkProtocol::Sixel | TerminalArtworkProtocol::Unicode => None,
    }
}

fn minimal_kitty_png_command(bytes: &[u8]) -> String {
    format!("\u{1b}_Ga=T,f=100;{}\u{1b}\\", STANDARD.encode(bytes))
}

fn kitty_command(image_id: u32, bytes: &[u8], placement: ArtworkPlacement) -> String {
    let encoded = STANDARD.encode(bytes);
    let chunks = encoded
        .as_bytes()
        .chunks(KITTY_CHUNK_BYTES)
        .collect::<Vec<_>>();
    let mut output = String::new();
    for (index, chunk) in chunks.iter().enumerate() {
        let more = u8::from(index + 1 < chunks.len());
        if index == 0 {
            output.push_str(&format!(
                "\u{1b}_Gi={image_id},a=T,t=d,f=100,c={},r={},q=2,C=1,m={more};",
                placement.columns, placement.rows
            ));
        } else {
            output.push_str(&format!("\u{1b}_Gm={more};"));
        }
        output.push_str(std::str::from_utf8(chunk).expect("base64 output is UTF-8"));
        output.push_str("\u{1b}\\");
    }
    output
}

fn kitty_delete_command(image_id: u32) -> String {
    format!("\u{1b}_Ga=d,d=I,i={image_id},q=2;\u{1b}\\")
}

fn maybe_tmux_passthrough(command: String, enabled: bool) -> String {
    if !enabled {
        return command;
    }
    let escaped = command.replace('\u{1b}', "\u{1b}\u{1b}");
    format!("\u{1b}Ptmux;{escaped}\u{1b}\\")
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use base64::Engine as _;

    use crate::{
        app::state::{AppState, ArtworkCacheEntry, RenderableArtworkCacheEntry},
        domain::{Artwork, ArtworkKey, ArtworkMediaType, TrackId},
    };

    use super::{
        ArtworkPlacement, ArtworkRendererSource, ProcessTerminalEnvironment, RendererEnvironment,
        TerminalArtworkProtocol, artwork_placement, can_render_inline, fit_artwork_rect,
        inline_artwork_is_ready_for_protocol, inline_command, kitty_command, kitty_delete_command,
        maybe_tmux_passthrough, minimal_kitty_png_command, prepare_kitty_renderable,
        renderer_diagnostics_for, select_renderer,
    };

    fn jpeg_fixture() -> Artwork {
        let image = image::DynamicImage::new_rgb8(2, 2);
        let mut bytes = Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, image::ImageFormat::Jpeg)
            .expect("small JPEG fixture encodes");
        Artwork {
            media_type: ArtworkMediaType::Jpeg,
            bytes: bytes.into_inner(),
        }
    }

    #[test]
    fn kitty_prepares_valid_jpeg_as_bounded_png() {
        let source = jpeg_fixture();
        let renderable = prepare_kitty_renderable(&source).expect("JPEG converts");
        assert_eq!(renderable.media_type, ArtworkMediaType::Png);
        assert!(renderable.bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_ne!(renderable.bytes, source.bytes);
    }

    #[test]
    fn kitty_png_source_passes_through_without_reencoding() {
        let source = Artwork {
            media_type: ArtworkMediaType::Png,
            bytes: b"\x89PNG\r\n\x1a\nsource".to_vec(),
        };
        assert_eq!(prepare_kitty_renderable(&source), Ok(source));
    }

    #[test]
    fn malformed_jpeg_falls_back_without_panicking() {
        let source = Artwork {
            media_type: ArtworkMediaType::Jpeg,
            bytes: vec![0xff, 0xd8, 0xff, 0xd9],
        };
        assert!(
            prepare_kitty_renderable(&source)
                .expect_err("invalid JPEG is rejected")
                .contains("decode failed")
        );
    }

    #[test]
    fn placement_uses_the_exact_border_excluded_ratatui_rectangle() {
        let inner = ratatui::layout::Rect::new(7, 11, 14, 5);
        assert_eq!(
            artwork_placement(inner),
            Some(ArtworkPlacement {
                x: 7,
                y: 11,
                columns: 14,
                rows: 5,
            })
        );
        assert_ne!(
            artwork_placement(inner),
            artwork_placement(ratatui::layout::Rect::new(7, 11, 10, 4))
        );
    }

    #[test]
    fn fit_artwork_rect_centers_square_wide_tall_and_odd_sizes() {
        let target = ratatui::layout::Rect::new(10, 20, 16, 6);
        let square = fit_artwork_rect(100, 100, target, 0.5).expect("square fits");
        assert_eq!(square, ratatui::layout::Rect::new(12, 20, 12, 6));

        let wide = fit_artwork_rect(200, 100, target, 0.5).expect("wide fits");
        assert_eq!(wide, ratatui::layout::Rect::new(10, 21, 16, 4));

        let tall = fit_artwork_rect(100, 200, target, 0.5).expect("tall fits");
        assert_eq!(tall, ratatui::layout::Rect::new(15, 20, 6, 6));

        let odd_target = ratatui::layout::Rect::new(3, 7, 13, 7);
        let odd = fit_artwork_rect(100, 100, odd_target, 0.5).expect("odd fit");
        assert!(odd.x >= odd_target.x && odd.y >= odd_target.y);
        assert!(odd.right() <= odd_target.right() && odd.bottom() <= odd_target.bottom());
        assert!(
            (i32::from(odd.x - odd_target.x) * 2 + i32::from(odd.width)
                - i32::from(odd_target.width))
            .abs()
                <= 1
        );
    }

    #[test]
    fn fit_artwork_rect_reacts_to_resize_and_rejects_empty_targets() {
        let source = (100, 100);
        let large = fit_artwork_rect(
            source.0,
            source.1,
            ratatui::layout::Rect::new(0, 0, 18, 7),
            0.5,
        )
        .expect("large target");
        let medium = fit_artwork_rect(
            source.0,
            source.1,
            ratatui::layout::Rect::new(0, 0, 14, 5),
            0.5,
        )
        .expect("medium target");
        assert_ne!(large, medium);
        assert!(
            fit_artwork_rect(
                source.0,
                source.1,
                ratatui::layout::Rect::new(0, 0, 0, 5),
                0.5
            )
            .is_none()
        );
    }

    #[test]
    fn kitty_inline_state_requires_a_matching_renderable_png() {
        let key = ArtworkKey::Track(TrackId::new("track-art"));
        let source = Artwork {
            media_type: ArtworkMediaType::Jpeg,
            bytes: vec![1, 2, 3],
        };
        let mut state = AppState::default();
        state
            .artwork_cache
            .insert(key.clone(), ArtworkCacheEntry::Ready(source.clone()));
        assert!(!inline_artwork_is_ready_for_protocol(
            &state,
            &key,
            TerminalArtworkProtocol::Kitty
        ));
        state.renderable_artwork_cache.insert(
            key.clone(),
            RenderableArtworkCacheEntry::Ready {
                source_fingerprint: source.fingerprint(),
                artwork: Artwork {
                    media_type: ArtworkMediaType::Png,
                    bytes: b"\x89PNG\r\n\x1a\nimage".to_vec(),
                },
            },
        );
        assert!(inline_artwork_is_ready_for_protocol(
            &state,
            &key,
            TerminalArtworkProtocol::Kitty
        ));
    }

    #[test]
    fn a_track_change_or_resize_changes_the_display_identity() {
        let first = super::DisplayedArtwork {
            key: ArtworkKey::Track(TrackId::new("first")),
            protocol: TerminalArtworkProtocol::Kitty,
            placement: ArtworkPlacement {
                x: 3,
                y: 20,
                columns: 16,
                rows: 5,
            },
            tmux_passthrough: false,
        };
        let changed_track = super::DisplayedArtwork {
            key: ArtworkKey::Track(TrackId::new("second")),
            ..first.clone()
        };
        let resized = super::DisplayedArtwork {
            placement: ArtworkPlacement {
                columns: 12,
                ..first.placement
            },
            ..first.clone()
        };
        assert_ne!(first, changed_track);
        assert_ne!(first, resized);
    }

    #[test]
    fn album_layout_uses_its_own_inner_rectangle() {
        let outer = ratatui::layout::Rect::new(82, 4, 18, 9);
        let inner = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .inner(outer);
        let placement = artwork_placement(inner).expect("non-empty album artwork area");
        assert_eq!((placement.x, placement.y), (83, 5));
        assert_eq!((placement.columns, placement.rows), (16, 7));
    }

    #[test]
    fn renderer_selection_detects_ghostty_and_unknown_terminals() {
        assert_eq!(
            select_renderer(RendererEnvironment {
                term_program: Some("ghostty"),
                term: Some("xterm-256color"),
                ..RendererEnvironment::default()
            }),
            super::ArtworkRendererSelection {
                protocol: TerminalArtworkProtocol::Kitty,
                source: ArtworkRendererSource::Ghostty,
                tmux_passthrough: false,
            }
        );
        assert_eq!(
            select_renderer(RendererEnvironment {
                term_program: Some("Apple_Terminal"),
                term: Some("xterm-256color"),
                ..RendererEnvironment::default()
            })
            .protocol,
            TerminalArtworkProtocol::Unicode
        );
    }

    #[test]
    fn tmux_does_not_force_unicode_when_a_kitty_renderer_is_known_or_requested() {
        assert_eq!(
            select_renderer(RendererEnvironment {
                term_program: Some("ghostty"),
                term: Some("tmux-256color"),
                tmux: Some("/tmp/tmux-501/default,1,0"),
                ..RendererEnvironment::default()
            }),
            super::ArtworkRendererSelection {
                protocol: TerminalArtworkProtocol::Kitty,
                source: ArtworkRendererSource::Ghostty,
                tmux_passthrough: true,
            }
        );
        assert_eq!(
            select_renderer(RendererEnvironment {
                term_program: Some("tmux"),
                term: Some("tmux-256color"),
                tmux: Some("/tmp/tmux-501/default,1,0"),
                override_renderer: Some("kitty"),
                ..RendererEnvironment::default()
            }),
            super::ArtworkRendererSelection {
                protocol: TerminalArtworkProtocol::Kitty,
                source: ArtworkRendererSource::ExplicitOverride,
                tmux_passthrough: true,
            }
        );
    }

    #[test]
    fn exact_tmux_environment_is_shared_by_renderer_and_diagnostics() {
        let diagnostics = renderer_diagnostics_for(ProcessTerminalEnvironment {
            term: Some("tmux-256color".to_owned()),
            term_program: Some("tmux".to_owned()),
            tmux: Some("/private/tmp/tmux-501/default,8459,0".to_owned()),
            override_renderer: Some("kitty".to_owned()),
            ..ProcessTerminalEnvironment::default()
        });

        assert!(diagnostics.tmux);
        assert_eq!(
            diagnostics.tmux_value.as_deref(),
            Some("/private/tmp/tmux-501/default,8459,0")
        );
        assert_eq!(
            diagnostics.selection.protocol,
            TerminalArtworkProtocol::Kitty
        );
        assert!(diagnostics.selection.tmux_passthrough);
    }

    #[test]
    fn direct_ghostty_with_unset_or_empty_tmux_reports_no_tmux() {
        for tmux in [None, Some(String::new())] {
            let diagnostics = renderer_diagnostics_for(ProcessTerminalEnvironment {
                term: Some("xterm-256color".to_owned()),
                term_program: Some("ghostty".to_owned()),
                tmux,
                ..ProcessTerminalEnvironment::default()
            });
            assert!(!diagnostics.tmux);
            assert_eq!(
                diagnostics.selection.protocol,
                TerminalArtworkProtocol::Kitty
            );
            assert!(!diagnostics.selection.tmux_passthrough);
        }
    }

    #[test]
    fn inline_commands_are_bounded_and_format_aware() {
        let jpeg = Artwork {
            media_type: ArtworkMediaType::Jpeg,
            bytes: vec![0xff, 0xd8, 0xff, 0xd9],
        };
        let png = Artwork {
            media_type: ArtworkMediaType::Png,
            bytes: b"\x89PNG\r\n\x1a\n".to_vec(),
        };
        let placement = ArtworkPlacement {
            x: 10,
            y: 3,
            columns: 9,
            rows: 5,
        };

        assert!(can_render_inline(TerminalArtworkProtocol::ITerm2, &jpeg));
        assert!(!can_render_inline(TerminalArtworkProtocol::Kitty, &jpeg));
        assert!(can_render_inline(TerminalArtworkProtocol::Kitty, &png));
        assert!(
            inline_command(TerminalArtworkProtocol::ITerm2, &jpeg, placement)
                .expect("iTerm2 command")
                .contains("1337;File=")
        );
        assert!(
            inline_command(TerminalArtworkProtocol::Kitty, &png, placement)
                .expect("Kitty command")
                .contains("a=T,t=d,f=100")
        );
        assert!(inline_command(TerminalArtworkProtocol::Sixel, &png, placement).is_none());
    }

    #[test]
    fn tmux_passthrough_escapes_kitty_control_sequences() {
        let raw = "\u{1b}_Ga=d\u{1b}\\".to_owned();
        assert_eq!(
            maybe_tmux_passthrough(raw, true),
            "\u{1b}Ptmux;\u{1b}\u{1b}_Ga=d\u{1b}\u{1b}\\\u{1b}\\"
        );
    }

    #[test]
    fn kitty_command_uses_specified_apc_framing_and_png_placement() {
        let placement = ArtworkPlacement {
            x: 0,
            y: 0,
            columns: 8,
            rows: 4,
        };
        assert_eq!(
            kitty_command(42, b"abc", placement),
            "\u{1b}_Gi=42,a=T,t=d,f=100,c=8,r=4,q=2,C=1,m=0;YWJj\u{1b}\\"
        );
        assert_eq!(
            kitty_delete_command(42),
            "\u{1b}_Ga=d,d=I,i=42,q=2;\u{1b}\\"
        );
    }

    #[test]
    fn minimal_probe_is_a_single_direct_png_apc_without_placement_or_identity() {
        let png = base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==")
            .expect("known-good PNG base64");
        assert_eq!(
            minimal_kitty_png_command(&png),
            "\u{1b}_Ga=T,f=100;iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==\u{1b}\\"
        );
    }

    #[test]
    fn kitty_command_chunks_base64_at_the_protocol_limit_and_terminates_final_chunk() {
        let placement = ArtworkPlacement {
            x: 0,
            y: 0,
            columns: 8,
            rows: 4,
        };
        let command = kitty_command(42, &vec![0_u8; 3_073], placement);
        assert_eq!(command.matches("\u{1b}_G").count(), 2);
        assert!(command.starts_with("\u{1b}_Gi=42,a=T,t=d,f=100,c=8,r=4,q=2,C=1,m=1;"));
        assert!(command.contains("\u{1b}\\\u{1b}_Gm=0;"));
        assert!(command.ends_with("\u{1b}\\"));
        assert!(!command.contains("\u{1b}_Gm=1;"));

        let wrapped = maybe_tmux_passthrough(command, true);
        assert!(wrapped.starts_with("\u{1b}Ptmux;\u{1b}\u{1b}_G"));
        assert!(wrapped.ends_with("\u{1b}\\"));
        assert_eq!(wrapped.matches("\u{1b}\u{1b}").count(), 4);
    }
}
