use anyhow::{Result, anyhow};
use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeName {
    Dracula,
    TokyoNight,
    Catppuccin,
    RosePine,
    VimClassic,
    Monokai,
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub bg: Color,
    pub panel: Color,
    pub panel_alt: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub accent_alt: Color,
    pub warn: Color,
    pub danger: Color,
    pub code: Color,
    pub link: Color,
    pub quote: Color,
    pub title: Color,
}

impl ThemeName {
    pub fn parse(input: &str) -> Result<Self> {
        match normalize(input).as_str() {
            "dracula" => Ok(Self::Dracula),
            "tokyonight" => Ok(Self::TokyoNight),
            "catppuccin" | "catpuccin" => Ok(Self::Catppuccin),
            "rosepine" => Ok(Self::RosePine),
            "vimclassic" => Ok(Self::VimClassic),
            "monokai" => Ok(Self::Monokai),
            other => Err(anyhow!(
                "unknown theme '{other}'; choose dracula, tokyo night, catppuccin, rose pine, vim classic, or monokai"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dracula => "dracula",
            Self::TokyoNight => "tokyo night",
            Self::Catppuccin => "catppuccin",
            Self::RosePine => "rose pine",
            Self::VimClassic => "vim classic",
            Self::Monokai => "monokai",
        }
    }
}

impl Theme {
    pub fn from_name(name: ThemeName) -> Self {
        match name {
            ThemeName::Dracula => Self {
                bg: rgb(24, 25, 33),
                panel: rgb(40, 42, 54),
                panel_alt: rgb(53, 57, 76),
                text: rgb(248, 248, 242),
                muted: rgb(189, 147, 249),
                accent: rgb(139, 233, 253),
                accent_alt: rgb(80, 250, 123),
                warn: rgb(241, 250, 140),
                danger: rgb(255, 85, 85),
                code: rgb(255, 184, 108),
                link: rgb(139, 233, 253),
                quote: rgb(98, 114, 164),
                title: rgb(255, 121, 198),
            },
            ThemeName::TokyoNight => Self {
                bg: rgb(17, 19, 29),
                panel: rgb(26, 27, 38),
                panel_alt: rgb(41, 46, 66),
                text: rgb(192, 202, 245),
                muted: rgb(122, 162, 247),
                accent: rgb(125, 207, 255),
                accent_alt: rgb(158, 206, 106),
                warn: rgb(224, 175, 104),
                danger: rgb(247, 118, 142),
                code: rgb(187, 154, 247),
                link: rgb(125, 207, 255),
                quote: rgb(86, 95, 137),
                title: rgb(187, 154, 247),
            },
            ThemeName::Catppuccin => Self {
                bg: rgb(17, 17, 27),
                panel: rgb(30, 30, 46),
                panel_alt: rgb(49, 50, 68),
                text: rgb(205, 214, 244),
                muted: rgb(108, 112, 134),
                accent: rgb(137, 180, 250),
                accent_alt: rgb(166, 227, 161),
                warn: rgb(249, 226, 175),
                danger: rgb(243, 139, 168),
                code: rgb(250, 179, 135),
                link: rgb(116, 199, 236),
                quote: rgb(88, 91, 112),
                title: rgb(203, 166, 247),
            },
            ThemeName::RosePine => Self {
                bg: rgb(25, 23, 36),
                panel: rgb(31, 29, 46),
                panel_alt: rgb(38, 35, 58),
                text: rgb(224, 222, 244),
                muted: rgb(144, 140, 170),
                accent: rgb(156, 207, 216),
                accent_alt: rgb(49, 116, 143),
                warn: rgb(246, 193, 119),
                danger: rgb(235, 111, 146),
                code: rgb(196, 167, 231),
                link: rgb(156, 207, 216),
                quote: rgb(82, 79, 103),
                title: rgb(234, 154, 151),
            },
            ThemeName::VimClassic => Self {
                bg: rgb(0, 0, 0),
                panel: rgb(12, 12, 12),
                panel_alt: rgb(30, 30, 30),
                text: rgb(255, 255, 255),
                muted: rgb(128, 128, 128),
                accent: rgb(0, 255, 255),
                accent_alt: rgb(0, 255, 0),
                warn: rgb(255, 255, 0),
                danger: rgb(255, 64, 64),
                code: rgb(255, 175, 0),
                link: rgb(0, 255, 255),
                quote: rgb(90, 90, 90),
                title: rgb(255, 255, 0),
            },
            ThemeName::Monokai => Self {
                bg: rgb(28, 28, 28),
                panel: rgb(39, 40, 34),
                panel_alt: rgb(52, 53, 47),
                text: rgb(248, 248, 242),
                muted: rgb(117, 113, 94),
                accent: rgb(102, 217, 239),
                accent_alt: rgb(166, 226, 46),
                warn: rgb(230, 219, 116),
                danger: rgb(249, 38, 114),
                code: rgb(253, 151, 31),
                link: rgb(102, 217, 239),
                quote: rgb(117, 113, 94),
                title: rgb(174, 129, 255),
            },
        }
    }
}

fn normalize(input: &str) -> String {
    input
        .chars()
        .filter(|ch| !matches!(ch, ' ' | '-' | '_'))
        .flat_map(char::to_lowercase)
        .collect()
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}
