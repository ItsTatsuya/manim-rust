//! Colors, with the standard Manim palette.

/// Linear-in-sRGB color. Opacity is stored separately on mobjects (like Manim),
/// so `a` is normally 1.0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Color { r, g, b, a: 1.0 }
    }

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Color { r, g, b, a }
    }

    /// `Color::from_hex(0x58C4DD)`
    pub const fn from_hex(hex: u32) -> Self {
        Color {
            r: ((hex >> 16) & 0xff) as f32 / 255.0,
            g: ((hex >> 8) & 0xff) as f32 / 255.0,
            b: (hex & 0xff) as f32 / 255.0,
            a: 1.0,
        }
    }

    /// Parse `"#RRGGBB"`, `"RRGGBB"` or `"#RGB"`.
    pub fn hex(s: &str) -> Option<Self> {
        let s = s.trim_start_matches('#');
        let v = u32::from_str_radix(s, 16).ok()?;
        match s.len() {
            6 => Some(Self::from_hex(v)),
            3 => {
                let (r, g, b) = ((v >> 8) & 0xf, (v >> 4) & 0xf, v & 0xf);
                Some(Self::from_hex((r * 17) << 16 | (g * 17) << 8 | (b * 17)))
            }
            _ => None,
        }
    }

    pub fn lerp(self, other: Color, t: f64) -> Color {
        // a·(1−t) + b·t is exact at both ends (a + (b−a)·t is not at t = 1).
        let (t, u) = (t as f32, 1.0 - t as f32);
        Color {
            r: self.r * u + other.r * t,
            g: self.g * u + other.g * t,
            b: self.b * u + other.b * t,
            a: self.a * u + other.a * t,
        }
    }

    pub fn with_alpha(self, a: f32) -> Color {
        Color { a, ..self }
    }

    /// RGBA with an extra opacity factor applied, clamped to [0, 1].
    pub fn to_array(self, opacity: f64) -> [f32; 4] {
        let c = |v: f32| v.clamp(0.0, 1.0);
        [c(self.r), c(self.g), c(self.b), c(self.a * opacity as f32)]
    }
}

/// Interpolate across a list of colors (like `color_gradient`).
pub fn color_gradient(colors: &[Color], n: usize) -> Vec<Color> {
    if colors.is_empty() || n == 0 {
        return Vec::new();
    }
    if colors.len() == 1 || n == 1 {
        return vec![colors[0]; n];
    }
    (0..n)
        .map(|i| {
            let x = i as f64 / (n - 1) as f64 * (colors.len() - 1) as f64;
            let k = (x.floor() as usize).min(colors.len() - 2);
            colors[k].lerp(colors[k + 1], x - k as f64)
        })
        .collect()
}

macro_rules! palette {
    ($($name:ident = $hex:literal),* $(,)?) => {
        $(pub const $name: Color = Color::from_hex($hex);)*
    };
}

palette! {
    WHITE = 0xFFFFFF, BLACK = 0x000000,
    GRAY_A = 0xDDDDDD, GRAY_B = 0xBBBBBB, GRAY_C = 0x888888, GRAY_D = 0x444444, GRAY_E = 0x222222,
    BLUE_A = 0xC7E9F1, BLUE_B = 0x9CDCEB, BLUE_C = 0x58C4DD, BLUE_D = 0x29ABCA, BLUE_E = 0x236B8E,
    TEAL_A = 0xACEAD7, TEAL_B = 0x76DDC0, TEAL_C = 0x5CD0B3, TEAL_D = 0x55C1A7, TEAL_E = 0x49A88F,
    GREEN_A = 0xC9E2AE, GREEN_B = 0xA6CF8C, GREEN_C = 0x83C167, GREEN_D = 0x77B05D, GREEN_E = 0x699C52,
    YELLOW_A = 0xFFF1B6, YELLOW_B = 0xFFEA94, YELLOW_C = 0xF7D96F, YELLOW_D = 0xF4D345, YELLOW_E = 0xE8C11C,
    GOLD_A = 0xF7C797, GOLD_B = 0xF9B775, GOLD_C = 0xF0AC5F, GOLD_D = 0xE1A158, GOLD_E = 0xC78D46,
    RED_A = 0xF7A1A3, RED_B = 0xFF8080, RED_C = 0xFC6255, RED_D = 0xE65A4C, RED_E = 0xCF5044,
    MAROON_A = 0xECABC1, MAROON_B = 0xEC92AB, MAROON_C = 0xC55F73, MAROON_D = 0xA24D61, MAROON_E = 0x94424F,
    PURPLE_A = 0xCAA3E8, PURPLE_B = 0xB189C6, PURPLE_C = 0x9A72AC, PURPLE_D = 0x715582, PURPLE_E = 0x644172,
    PINK = 0xD147BD, LIGHT_PINK = 0xDC75CD, ORANGE = 0xFF862F,
    LIGHT_BROWN = 0xCD853F, DARK_BROWN = 0x8B4513, GRAY_BROWN = 0x736357,
    PURE_RED = 0xFF0000, PURE_GREEN = 0x00FF00, PURE_BLUE = 0x0000FF,
    PURE_CYAN = 0x00FFFF, PURE_MAGENTA = 0xFF00FF, PURE_YELLOW = 0xFFFF00,
}

pub const GRAY: Color = GRAY_C;
pub const GREY: Color = GRAY_C;
pub const LIGHT_GRAY: Color = GRAY_B;
pub const DARK_GRAY: Color = GRAY_D;
pub const BLUE: Color = BLUE_C;
pub const DARK_BLUE: Color = BLUE_E;
pub const TEAL: Color = TEAL_C;
pub const GREEN: Color = GREEN_C;
pub const YELLOW: Color = YELLOW_C;
pub const GOLD: Color = GOLD_C;
pub const RED: Color = RED_C;
pub const MAROON: Color = MAROON_C;
pub const PURPLE: Color = PURPLE_C;
