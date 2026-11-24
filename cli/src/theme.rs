use ratatui::style::Color;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeId {
    #[default]
    Midnight,
    Midnight2,
    Ember,
    Ocean,
    Daylight,
    Forest,
    Nothing,
}

impl ThemeId {
    pub const ALL: [ThemeId; 7] = [
        ThemeId::Midnight,
        ThemeId::Midnight2,
        ThemeId::Ember,
        ThemeId::Ocean,
        ThemeId::Daylight,
        ThemeId::Forest,
        ThemeId::Nothing,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ThemeId::Midnight => "Midnight",
            ThemeId::Midnight2 => "Midnight v2",
            ThemeId::Ember => "Ember",
            ThemeId::Ocean => "Ocean",
            ThemeId::Daylight => "Daylight",
            ThemeId::Forest => "Las",
            ThemeId::Nothing => "Nothing OS",
        }
    }

    pub fn desc(self) -> &'static str {
        match self {
            ThemeId::Midnight => "Domyślny SoundWave — mięta i fiolet",
            ThemeId::Midnight2 => "Granatowa czerń z fioletowym akcentem",
            ThemeId::Ember => "Ciepła, ciemna — bursztyn i koral",
            ThemeId::Ocean => "Chłodna, granatowa — błękit i indygo",
            ThemeId::Daylight => "Jasna — na dzień",
            ThemeId::Forest => "Ciemna zieleń i limonka",
            ThemeId::Nothing => "Minimalizm, czerń/biel i czerwony akcent",
        }
    }

    pub fn theme(self) -> Theme {
        match self {
            ThemeId::Midnight => MIDNIGHT,
            ThemeId::Midnight2 => MIDNIGHT2,
            ThemeId::Ember => EMBER,
            ThemeId::Ocean => OCEAN,
            ThemeId::Daylight => DAYLIGHT,
            ThemeId::Forest => FOREST,
            ThemeId::Nothing => NOTHING,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LayoutMode {
    #[default]
    Default,
    Studio,
}

impl LayoutMode {
    pub const ALL: [LayoutMode; 2] = [LayoutMode::Default, LayoutMode::Studio];

    pub fn name(self) -> &'static str {
        match self {
            LayoutMode::Default => "Default",
            LayoutMode::Studio => "Studio",
        }
    }

    pub fn desc(self) -> &'static str {
        match self {
            LayoutMode::Default => "Klasyczny: biblioteka + lista + odtwarzacz na dole",
            LayoutMode::Studio => "Stały panel po prawej: Audio Visualizer, kontrolki, Kolejka / Tekst (t)",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: Color,
    pub surface: Color,
    pub select_bg: Color,
    pub accent: Color,
    pub accent2: Color,
    pub accent3: Color,
    pub text: Color,
    pub text2: Color,
    pub text3: Color,
    pub border: Color,
    pub border_focus: Color,
    pub error: Color,
}

// 1. MIDNIGHT — mięta i fiolet (SoundWave domyślny)
pub const MIDNIGHT: Theme = Theme {
    bg: Color::Rgb(10, 10, 15),
    surface: Color::Rgb(17, 17, 24),
    select_bg: Color::Rgb(34, 34, 46),
    accent: Color::Rgb(110, 231, 183),
    accent2: Color::Rgb(167, 139, 250),
    accent3: Color::Rgb(244, 114, 182),
    text: Color::Rgb(240, 240, 248),
    text2: Color::Rgb(153, 153, 187),
    text3: Color::Rgb(85, 85, 112),
    border: Color::Rgb(42, 42, 58),
    border_focus: Color::Rgb(110, 231, 183),
    error: Color::Rgb(248, 113, 113),
};

// 2. MIDNIGHT V2 — granatowa czerń z fioletem
pub const MIDNIGHT2: Theme = Theme {
    bg: Color::Rgb(11, 10, 17),
    surface: Color::Rgb(18, 17, 25),
    select_bg: Color::Rgb(38, 36, 53),
    accent: Color::Rgb(139, 124, 246),
    accent2: Color::Rgb(169, 155, 255),
    accent3: Color::Rgb(248, 113, 113),
    text: Color::Rgb(237, 237, 243),
    text2: Color::Rgb(143, 143, 163),
    text3: Color::Rgb(86, 86, 106),
    border: Color::Rgb(44, 42, 60),
    border_focus: Color::Rgb(139, 124, 246),
    error: Color::Rgb(248, 113, 113),
};

// 3. EMBER — bursztyn i koral
pub const EMBER: Theme = Theme {
    bg: Color::Rgb(14, 10, 9),
    surface: Color::Rgb(22, 16, 14),
    select_bg: Color::Rgb(46, 34, 29),
    accent: Color::Rgb(251, 146, 60),
    accent2: Color::Rgb(244, 63, 94),
    accent3: Color::Rgb(250, 204, 21),
    text: Color::Rgb(251, 243, 238),
    text2: Color::Rgb(191, 167, 155),
    text3: Color::Rgb(111, 90, 80),
    border: Color::Rgb(58, 42, 36),
    border_focus: Color::Rgb(251, 146, 60),
    error: Color::Rgb(248, 113, 113),
};

// 4. OCEAN — błękit i indygo
pub const OCEAN: Theme = Theme {
    bg: Color::Rgb(7, 12, 22),
    surface: Color::Rgb(12, 19, 34),
    select_bg: Color::Rgb(28, 41, 66),
    accent: Color::Rgb(56, 189, 248),
    accent2: Color::Rgb(129, 140, 248),
    accent3: Color::Rgb(45, 212, 191),
    text: Color::Rgb(238, 244, 255),
    text2: Color::Rgb(147, 164, 195),
    text3: Color::Rgb(77, 93, 124),
    border: Color::Rgb(34, 50, 78),
    border_focus: Color::Rgb(56, 189, 248),
    error: Color::Rgb(248, 113, 113),
};

// 5. DAYLIGHT — jasna paleta
pub const DAYLIGHT: Theme = Theme {
    bg: Color::Rgb(244, 244, 247),
    surface: Color::Rgb(255, 255, 255),
    select_bg: Color::Rgb(220, 220, 229),
    accent: Color::Rgb(5, 150, 105),
    accent2: Color::Rgb(124, 58, 237),
    accent3: Color::Rgb(219, 39, 119),
    text: Color::Rgb(20, 20, 28),
    text2: Color::Rgb(84, 84, 104),
    text3: Color::Rgb(148, 148, 168),
    border: Color::Rgb(206, 206, 218),
    border_focus: Color::Rgb(5, 150, 105),
    error: Color::Rgb(220, 38, 38),
};

// 6. LAS — ciemna zieleń (szablon Własna z SoundWave)
pub const FOREST: Theme = Theme {
    bg: Color::Rgb(10, 17, 13),
    surface: Color::Rgb(16, 26, 20),
    select_bg: Color::Rgb(32, 44, 37),
    accent: Color::Rgb(74, 222, 128),
    accent2: Color::Rgb(163, 230, 53),
    accent3: Color::Rgb(52, 211, 153),
    text: Color::Rgb(238, 247, 240),
    text2: Color::Rgb(150, 163, 154),
    text3: Color::Rgb(92, 106, 97),
    border: Color::Rgb(38, 54, 45),
    border_focus: Color::Rgb(74, 222, 128),
    error: Color::Rgb(248, 113, 113),
};

// 7. NOTHING OS — zastępuje Liquid Glass w wersji CLI
pub const NOTHING: Theme = Theme {
    bg: Color::Rgb(10, 10, 12),
    surface: Color::Rgb(18, 18, 22),
    select_bg: Color::Rgb(36, 36, 44),
    accent: Color::Rgb(255, 59, 48),     // Nothing Red #ff3b30
    accent2: Color::Rgb(242, 242, 242),  // Crisp White
    accent3: Color::Rgb(255, 59, 48),    // Nothing Red ♥
    text: Color::Rgb(242, 242, 242),
    text2: Color::Rgb(142, 142, 150),
    text3: Color::Rgb(82, 82, 90),
    border: Color::Rgb(46, 46, 54),
    border_focus: Color::Rgb(255, 59, 48),
    error: Color::Rgb(255, 59, 48),
};
