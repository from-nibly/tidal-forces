use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
pub enum Theme {
    #[default]
    Dark,
    Light,
    System,
    Custom,
}

pub fn read_theme<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Theme, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}
pub fn read_palette<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Palette, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value::<Palette>(value)
        .ok()
        .filter(|p| p.validate().is_ok())
        .unwrap_or_default())
}
pub type Rgb = [u8; 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    pub bg: Rgb,
    pub panel: Rgb,
    pub card: Rgb,
    pub border: Rgb,
    pub muted: Rgb,
    pub accent: Rgb,
    pub playing: Rgb,
    pub selected: Rgb,
    pub text: Rgb,
    pub hover: Rgb,
    pub error: Rgb,
}
impl Default for Palette {
    fn default() -> Self {
        Self::DARK
    }
}
impl Palette {
    pub const DARK: Self = Self {
        bg: [12, 13, 15],
        panel: [18, 19, 22],
        card: [28, 30, 34],
        border: [43, 47, 53],
        muted: [151, 155, 165],
        accent: [81, 225, 219],
        playing: [21, 37, 39],
        selected: [20, 46, 48],
        text: [241, 242, 245],
        hover: [38, 44, 49],
        error: [255, 196, 192],
    };
    pub const LIGHT: Self = Self {
        bg: [248, 250, 251],
        panel: [255, 255, 255],
        card: [235, 240, 242],
        border: [163, 175, 181],
        muted: [69, 79, 87],
        accent: [0, 92, 88],
        playing: [220, 239, 237],
        selected: [204, 231, 228],
        text: [24, 32, 38],
        hover: [220, 230, 233],
        error: [153, 27, 27],
    };
    pub fn resolve(theme: Theme, system_dark: bool, custom: Self) -> Self {
        match theme {
            Theme::Dark => Self::DARK,
            Theme::Light => Self::LIGHT,
            Theme::System => {
                if system_dark {
                    Self::DARK
                } else {
                    Self::LIGHT
                }
            }
            Theme::Custom => {
                if custom.validate().is_ok() {
                    custom
                } else {
                    Self::DARK
                }
            }
        }
    }
    pub fn dark(&self) -> bool {
        luminance(self.bg) < 0.5
    }
    pub fn on_accent(&self) -> Rgb {
        if contrast(self.accent, [0; 3]) >= contrast(self.accent, [255; 3]) {
            [0; 3]
        } else {
            [255; 3]
        }
    }
    pub fn validate(&self) -> Result<()> {
        for background in [
            self.bg,
            self.panel,
            self.card,
            self.playing,
            self.selected,
            self.hover,
        ] {
            for foreground in [self.text, self.muted, self.accent, self.error] {
                ensure!(
                    contrast(background, foreground) >= 4.5,
                    "Palette text, muted text, accent and error colors must have at least 4.5:1 contrast against every surface"
                );
            }
        }
        ensure!(
            contrast(self.accent, self.on_accent()) >= 4.5,
            "Accent label contrast is too low"
        );
        Ok(())
    }
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= 4096, "Palette exceeds 4 KiB");
        let palette: Self = serde_json::from_slice(bytes).map_err(|_| {
            anyhow::anyhow!("Invalid palette JSON; use the documented RGB-array format")
        })?;
        palette.validate()?;
        Ok(palette)
    }
}
fn luminance(rgb: Rgb) -> f64 {
    let channels = rgb.map(|c| {
        let value = f64::from(c) / 255.;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    });
    channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722
}
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtins_have_readable_semantic_colors() {
        for palette in [Palette::DARK, Palette::LIGHT] {
            palette.validate().unwrap();
        }
    }
    #[test]
    fn system_mode_tracks_os_but_explicit_modes_do_not() {
        assert_eq!(
            Palette::resolve(Theme::System, true, Palette::LIGHT),
            Palette::DARK
        );
        assert_eq!(
            Palette::resolve(Theme::System, false, Palette::DARK),
            Palette::LIGHT
        );
        assert_eq!(
            Palette::resolve(Theme::Dark, false, Palette::LIGHT),
            Palette::DARK
        );
        assert_eq!(
            Palette::resolve(Theme::Light, true, Palette::DARK),
            Palette::LIGHT
        );
    }
    #[test]
    fn palette_import_is_bounded_strict_and_contrast_checked() {
        for palette in [Palette::DARK, Palette::LIGHT] {
            let bytes = serde_json::to_vec(&palette).unwrap();
            assert_eq!(Palette::parse(&bytes).unwrap(), palette);
        }
        let invalid = Palette {
            text: Palette::DARK.bg,
            ..Palette::DARK
        };
        assert!(invalid.validate().is_err());
        assert_eq!(
            Palette::resolve(Theme::Custom, true, invalid),
            Palette::DARK
        );
        assert!(Palette::parse(&vec![b' '; 4097]).is_err());
        assert!(Palette::parse(b"{}").is_err());
        let mut value = serde_json::to_value(Palette::DARK).unwrap();
        value["script"] = "not allowed".into();
        assert!(Palette::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    }
}
