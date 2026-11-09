//! Minimal RGB colour type shared by the style and layout stages.
//!
//! No alpha channel on purpose: the framebuffer compositor blends with an
//! explicit alpha argument, and keeping colours at 3 bytes each keeps the draw
//! list small (important for a 16 MiB heap).

/// A plain 8-bit-per-channel RGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
}

impl Color {
    /// Construct a colour from its components.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Black.
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    /// White.
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    /// Default body text colour (near-black, slightly blue to match the theme).
    pub const TEXT: Self = Self::rgb(24, 24, 28);

    /// Build a colour from a `0xRRGGBB` integer.
    pub const fn from_u32(value: u32) -> Self {
        Self {
            r: ((value >> 16) & 0xFF) as u8,
            g: ((value >> 8) & 0xFF) as u8,
            b: (value & 0xFF) as u8,
        }
    }

    /// Perceived brightness (0-255), used to pick readable foreground colours.
    pub fn brightness(self) -> u8 {
        // Rec. 601 luma, integer only (no floats in kernel code paths).
        ((self.r as u32 * 299 + self.g as u32 * 587 + self.b as u32 * 114) / 1000) as u8
    }

    /// Parse a CSS colour.
    ///
    /// Supported syntaxes:
    /// * `#rgb` / `#rrggbb`
    /// * `rgb(r, g, b)` / `rgb(r g b)` with 0-255 components
    /// * the CSS2.1 named colours plus a few common CSS3 names
    ///
    /// Returns `None` for unparsable input *and* for `transparent` — callers
    /// that need to distinguish those treat `None` as "leave unchanged".
    pub fn parse(input: &str) -> Option<Self> {
        let value = input.trim();
        if value.is_empty() {
            return None;
        }

        if let Some(hex) = value.strip_prefix('#') {
            return Self::parse_hex(hex);
        }

        let lower = value.to_ascii_lowercase();
        if lower.starts_with("rgb(") || lower.starts_with("rgba(") {
            let inner = lower
                .split_once('(')
                .and_then(|(_, rest)| rest.split_once(')'))
                .map(|(inner, _)| inner)?;
            let mut parts = inner.split([',', ' ', '/']).filter(|p| !p.is_empty());
            let r = parse_channel(parts.next()?)?;
            let g = parse_channel(parts.next()?)?;
            let b = parse_channel(parts.next()?)?;
            return Some(Self::rgb(r, g, b));
        }

        match lower.as_str() {
            "black" => Some(Self::rgb(0, 0, 0)),
            "white" => Some(Self::rgb(255, 255, 255)),
            "red" => Some(Self::rgb(255, 0, 0)),
            "green" => Some(Self::rgb(0, 128, 0)),
            "lime" => Some(Self::rgb(0, 255, 0)),
            "blue" => Some(Self::rgb(0, 0, 255)),
            "yellow" => Some(Self::rgb(255, 255, 0)),
            "cyan" | "aqua" => Some(Self::rgb(0, 255, 255)),
            "magenta" | "fuchsia" => Some(Self::rgb(255, 0, 255)),
            "silver" => Some(Self::rgb(192, 192, 192)),
            "gray" | "grey" => Some(Self::rgb(128, 128, 128)),
            "lightgray" | "lightgrey" => Some(Self::rgb(211, 211, 211)),
            "darkgray" | "darkgrey" => Some(Self::rgb(169, 169, 169)),
            "maroon" => Some(Self::rgb(128, 0, 0)),
            "olive" => Some(Self::rgb(128, 128, 0)),
            "navy" => Some(Self::rgb(0, 0, 128)),
            "teal" => Some(Self::rgb(0, 128, 128)),
            "purple" => Some(Self::rgb(128, 0, 128)),
            "orange" => Some(Self::rgb(255, 165, 0)),
            "brown" => Some(Self::rgb(165, 42, 42)),
            "pink" => Some(Self::rgb(255, 192, 203)),
            "gold" => Some(Self::rgb(255, 215, 0)),
            "whitesmoke" => Some(Self::rgb(245, 245, 245)),
            "gainsboro" => Some(Self::rgb(220, 220, 220)),
            "ivory" => Some(Self::rgb(255, 255, 240)),
            "beige" => Some(Self::rgb(245, 245, 220)),
            "transparent" => None,
            _ => None,
        }
    }

    fn parse_hex(hex: &str) -> Option<Self> {
        let bytes = hex.as_bytes();
        match bytes.len() {
            3 => {
                let r = hex_digit(bytes[0])? * 17;
                let g = hex_digit(bytes[1])? * 17;
                let b = hex_digit(bytes[2])? * 17;
                Some(Self::rgb(r, g, b))
            }
            6 => {
                let r = hex_digit(bytes[0])? * 16 + hex_digit(bytes[1])?;
                let g = hex_digit(bytes[2])? * 16 + hex_digit(bytes[3])?;
                let b = hex_digit(bytes[4])? * 16 + hex_digit(bytes[5])?;
                Some(Self::rgb(r, g, b))
            }
            _ => None,
        }
    }
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn parse_channel(input: &str) -> Option<u8> {
    let value = input.trim();
    let numeric = value.strip_suffix('%').unwrap_or(value);
    let parsed: u32 = numeric.parse().ok()?;
    let scaled = if value.ends_with('%') {
        (parsed.min(100) * 255) / 100
    } else {
        parsed.min(255)
    };
    Some(scaled as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_short_and_long_hex() {
        assert_eq!(Color::parse("#fff"), Some(Color::WHITE));
        assert_eq!(Color::parse("#00FF80"), Some(Color::rgb(0, 255, 128)));
        assert_eq!(Color::parse("#123"), Some(Color::rgb(17, 34, 51)));
    }

    #[test]
    fn parses_rgb_functions_and_names() {
        assert_eq!(Color::parse("rgb(1, 2, 3)"), Some(Color::rgb(1, 2, 3)));
        assert_eq!(
            Color::parse("rgb(100% 0% 50%)"),
            Some(Color::rgb(255, 0, 127))
        );
        assert_eq!(Color::parse(" Navy "), Some(Color::rgb(0, 0, 128)));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(Color::parse(""), None);
        assert_eq!(Color::parse("#12"), None);
        assert_eq!(Color::parse("#zzzzzz"), None);
        assert_eq!(Color::parse("rgb(1,2)"), None);
        assert_eq!(Color::parse("transparent"), None);
        assert_eq!(Color::parse("not-a-colour"), None);
    }

    #[test]
    fn brightness_orders_colours() {
        assert!(Color::WHITE.brightness() > Color::rgb(128, 128, 128).brightness());
        assert!(Color::BLACK.brightness() < Color::TEXT.brightness());
        assert_eq!(Color::from_u32(0x00_FF_00).g, 255);
    }
}
