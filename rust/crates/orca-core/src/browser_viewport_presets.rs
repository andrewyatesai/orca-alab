//! Browser DevTools viewport presets, ported from
//! `src/shared/browser-viewport-presets.ts`.
//!
//! The preset table mirrors Chrome's device-toolbar dimensions; `mobile = true`
//! enables touch emulation + small-viewport CSS, and `device_scale_factor = 2`
//! on mobile/tablet matches retina asset selection. Lookups resolve a persisted
//! preset id back to its row, and the row maps onto a CDP viewport override.

/// Stable id persisted on a `BrowserPage` so CDP emulation reapplies on reload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserViewportPresetId {
    MobileS,
    MobileM,
    MobileL,
    Tablet,
    Laptop,
    LaptopL,
    Desktop,
}

impl BrowserViewportPresetId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MobileS => "mobile-s",
            Self::MobileM => "mobile-m",
            Self::MobileL => "mobile-l",
            Self::Tablet => "tablet",
            Self::Laptop => "laptop",
            Self::LaptopL => "laptop-l",
            Self::Desktop => "desktop",
        }
    }

    pub fn from_id(value: &str) -> Option<Self> {
        match value {
            "mobile-s" => Some(Self::MobileS),
            "mobile-m" => Some(Self::MobileM),
            "mobile-l" => Some(Self::MobileL),
            "tablet" => Some(Self::Tablet),
            "laptop" => Some(Self::Laptop),
            "laptop-l" => Some(Self::LaptopL),
            "desktop" => Some(Self::Desktop),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BrowserViewportPreset {
    pub id: BrowserViewportPresetId,
    pub label: &'static str,
    pub width: u32,
    pub height: u32,
    pub device_scale_factor: u32,
    pub mobile: bool,
}

/// CDP viewport emulation override derived from a preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BrowserViewportOverride {
    pub width: u32,
    pub height: u32,
    pub device_scale_factor: u32,
    pub mobile: bool,
}

pub const BROWSER_VIEWPORT_PRESETS: [BrowserViewportPreset; 7] = [
    BrowserViewportPreset {
        id: BrowserViewportPresetId::MobileS,
        label: "Mobile S — 320 × 568",
        width: 320,
        height: 568,
        device_scale_factor: 2,
        mobile: true,
    },
    BrowserViewportPreset {
        id: BrowserViewportPresetId::MobileM,
        label: "Mobile M — 375 × 667",
        width: 375,
        height: 667,
        device_scale_factor: 2,
        mobile: true,
    },
    BrowserViewportPreset {
        id: BrowserViewportPresetId::MobileL,
        label: "Mobile L — 425 × 812",
        width: 425,
        height: 812,
        device_scale_factor: 2,
        mobile: true,
    },
    BrowserViewportPreset {
        id: BrowserViewportPresetId::Tablet,
        label: "Tablet — 768 × 1024",
        width: 768,
        height: 1024,
        device_scale_factor: 2,
        mobile: true,
    },
    BrowserViewportPreset {
        id: BrowserViewportPresetId::Laptop,
        label: "Laptop — 1024 × 768",
        width: 1024,
        height: 768,
        device_scale_factor: 1,
        mobile: false,
    },
    BrowserViewportPreset {
        id: BrowserViewportPresetId::LaptopL,
        label: "Laptop L — 1440 × 900",
        width: 1440,
        height: 900,
        device_scale_factor: 1,
        mobile: false,
    },
    BrowserViewportPreset {
        id: BrowserViewportPresetId::Desktop,
        label: "Desktop — 1920 × 1080",
        width: 1920,
        height: 1080,
        device_scale_factor: 1,
        mobile: false,
    },
];

/// Resolve a persisted preset id to its row, or `None` for a missing/unknown id
/// (matches the TS `?? null` over `Array.find`).
pub fn get_browser_viewport_preset(
    id: Option<BrowserViewportPresetId>,
) -> Option<BrowserViewportPreset> {
    let id = id?;
    BROWSER_VIEWPORT_PRESETS.iter().copied().find(|preset| preset.id == id)
}

/// Map a preset row onto a CDP viewport override.
// Trust contract: inert under stock cargo, proved under `--cfg trust_verify`.
// Postcondition — the override copies the preset's emulation fields exactly.
#[cfg_attr(trust_verify, trust::ensures(|out: &BrowserViewportOverride|
    out.width == preset.width
        && out.height == preset.height
        && out.device_scale_factor == preset.device_scale_factor
        && out.mobile == preset.mobile))]
pub fn browser_viewport_preset_to_override(
    preset: BrowserViewportPreset,
) -> BrowserViewportOverride {
    BrowserViewportOverride {
        width: preset.width,
        height: preset.height,
        device_scale_factor: preset.device_scale_factor,
        mobile: preset.mobile,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_matches_chrome_devtools_dimensions() {
        assert_eq!(BROWSER_VIEWPORT_PRESETS.len(), 7);
        let mobile_s = BROWSER_VIEWPORT_PRESETS[0];
        assert_eq!(mobile_s.id, BrowserViewportPresetId::MobileS);
        assert_eq!(mobile_s.label, "Mobile S — 320 × 568");
        assert_eq!(mobile_s.width, 320);
        assert_eq!(mobile_s.height, 568);
        assert_eq!(mobile_s.device_scale_factor, 2);
        assert!(mobile_s.mobile);

        let desktop = BROWSER_VIEWPORT_PRESETS[6];
        assert_eq!(desktop.id, BrowserViewportPresetId::Desktop);
        assert_eq!(desktop.width, 1920);
        assert_eq!(desktop.height, 1080);
        assert_eq!(desktop.device_scale_factor, 1);
        assert!(!desktop.mobile);
    }

    #[test]
    fn looks_up_a_preset_by_id() {
        let preset = get_browser_viewport_preset(Some(BrowserViewportPresetId::Tablet)).unwrap();
        assert_eq!(preset.id, BrowserViewportPresetId::Tablet);
        assert_eq!(preset.width, 768);
        assert_eq!(preset.height, 1024);
        assert!(preset.mobile);
    }

    #[test]
    fn returns_none_for_a_missing_id() {
        assert_eq!(get_browser_viewport_preset(None), None);
    }

    #[test]
    fn unknown_id_string_does_not_resolve() {
        assert_eq!(BrowserViewportPresetId::from_id("nope"), None);
        assert_eq!(get_browser_viewport_preset(BrowserViewportPresetId::from_id("nope")), None);
    }

    #[test]
    fn maps_a_preset_to_a_cdp_override() {
        let preset = get_browser_viewport_preset(Some(BrowserViewportPresetId::Laptop)).unwrap();
        assert_eq!(
            browser_viewport_preset_to_override(preset),
            BrowserViewportOverride {
                width: 1024,
                height: 768,
                device_scale_factor: 1,
                mobile: false,
            }
        );
    }

    #[test]
    fn id_round_trips_through_string() {
        for preset in BROWSER_VIEWPORT_PRESETS {
            assert_eq!(BrowserViewportPresetId::from_id(preset.id.as_str()), Some(preset.id));
        }
    }
}
