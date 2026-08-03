use ratatui::style::Color;

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub background: Color,
    pub foreground: Color,
    pub muted: Color,
    pub accent: Color,
    pub selection: Color,
    pub playing: Color,
    pub border: Color,
    pub error: Color,
    pub warning: Color,
}

pub const DEFAULT: Theme = Theme {
    background: Color::Rgb(18, 18, 24),
    foreground: Color::Rgb(235, 235, 240),
    muted: Color::Rgb(135, 135, 150),
    accent: Color::Rgb(250, 75, 105),
    selection: Color::Rgb(62, 62, 78),
    playing: Color::Rgb(95, 215, 145),
    border: Color::Rgb(72, 72, 88),
    error: Color::Rgb(255, 95, 95),
    warning: Color::Rgb(245, 190, 80),
};
