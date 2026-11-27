use std::borrow::Cow;

use egui::{load::Bytes, Color32, Image, ImageSource};

/// All bundled Hugeicons identifiers used across the launcher interface.
/// Every icon has both a stroke (outline) and solid (filled) SVG variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconKind {
    Play,
    Settings,
    Keyboard,
    Terminal,
    CheckCircle,
    AlertCircle,
    Folder,
    Power,
    InformationCircle,
    ChevronDown,
}

/// Visual state selector controlling whether an icon renders as filled or outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconStyle {
    Stroke,
    Solid,
}

impl IconStyle {
    pub fn from_active(active: bool) -> Self {
        if active {
            Self::Solid
        } else {
            Self::Stroke
        }
    }
}

/// Returns a bundled Hugeicons SVG image tinted to the requested color.
/// Source SVGs are authored in #FFFFFF so egui's multiplicative tinting maps
/// accurately to any dark-theme color token without resvg black-fill artifacts.
pub fn icon_image(kind: IconKind, style: IconStyle, size: f32, tint: Color32) -> Image<'static> {
    let (uri, bytes): (&'static str, &'static [u8]) = match (kind, style) {
        (IconKind::Play, IconStyle::Stroke) => (
            "bytes://icons/play-stroke.svg",
            include_bytes!("../assets/icons/play-stroke.svg"),
        ),
        (IconKind::Play, IconStyle::Solid) => (
            "bytes://icons/play-solid.svg",
            include_bytes!("../assets/icons/play-solid.svg"),
        ),
        (IconKind::Settings, IconStyle::Stroke) => (
            "bytes://icons/settings-stroke.svg",
            include_bytes!("../assets/icons/settings-stroke.svg"),
        ),
        (IconKind::Settings, IconStyle::Solid) => (
            "bytes://icons/settings-solid.svg",
            include_bytes!("../assets/icons/settings-solid.svg"),
        ),
        (IconKind::Keyboard, IconStyle::Stroke) => (
            "bytes://icons/keyboard-stroke.svg",
            include_bytes!("../assets/icons/keyboard-stroke.svg"),
        ),
        (IconKind::Keyboard, IconStyle::Solid) => (
            "bytes://icons/keyboard-solid.svg",
            include_bytes!("../assets/icons/keyboard-solid.svg"),
        ),
        (IconKind::Terminal, IconStyle::Stroke) => (
            "bytes://icons/terminal-stroke.svg",
            include_bytes!("../assets/icons/terminal-stroke.svg"),
        ),
        (IconKind::Terminal, IconStyle::Solid) => (
            "bytes://icons/terminal-solid.svg",
            include_bytes!("../assets/icons/terminal-solid.svg"),
        ),
        (IconKind::CheckCircle, IconStyle::Stroke) => (
            "bytes://icons/check-circle-stroke.svg",
            include_bytes!("../assets/icons/check-circle-stroke.svg"),
        ),
        (IconKind::CheckCircle, IconStyle::Solid) => (
            "bytes://icons/check-circle-solid.svg",
            include_bytes!("../assets/icons/check-circle-solid.svg"),
        ),
        (IconKind::AlertCircle, IconStyle::Stroke) => (
            "bytes://icons/alert-circle-stroke.svg",
            include_bytes!("../assets/icons/alert-circle-stroke.svg"),
        ),
        (IconKind::AlertCircle, IconStyle::Solid) => (
            "bytes://icons/alert-circle-solid.svg",
            include_bytes!("../assets/icons/alert-circle-solid.svg"),
        ),
        (IconKind::Folder, IconStyle::Stroke) => (
            "bytes://icons/folder-stroke.svg",
            include_bytes!("../assets/icons/folder-stroke.svg"),
        ),
        (IconKind::Folder, IconStyle::Solid) => (
            "bytes://icons/folder-solid.svg",
            include_bytes!("../assets/icons/folder-solid.svg"),
        ),
        (IconKind::Power, IconStyle::Stroke) => (
            "bytes://icons/power-stroke.svg",
            include_bytes!("../assets/icons/power-stroke.svg"),
        ),
        (IconKind::Power, IconStyle::Solid) => (
            "bytes://icons/power-solid.svg",
            include_bytes!("../assets/icons/power-solid.svg"),
        ),
        (IconKind::InformationCircle, IconStyle::Stroke) => (
            "bytes://icons/information-circle-stroke.svg",
            include_bytes!("../assets/icons/information-circle-stroke.svg"),
        ),
        (IconKind::InformationCircle, IconStyle::Solid) => (
            "bytes://icons/information-circle-solid.svg",
            include_bytes!("../assets/icons/information-circle-solid.svg"),
        ),
        (IconKind::ChevronDown, IconStyle::Stroke) => (
            "bytes://icons/chevron-down-stroke.svg",
            include_bytes!("../assets/icons/chevron-down-stroke.svg"),
        ),
        (IconKind::ChevronDown, IconStyle::Solid) => (
            "bytes://icons/chevron-down-solid.svg",
            include_bytes!("../assets/icons/chevron-down-solid.svg"),
        ),
    };

    Image::new(ImageSource::Bytes {
        uri: Cow::Borrowed(uri),
        bytes: Bytes::Static(bytes),
    })
    .fit_to_exact_size(egui::vec2(size, size))
    .tint(tint)
}
