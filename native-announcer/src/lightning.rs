use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct LightningSettings {
    pub roughness: f32,
    pub brightness: f32,
    pub core_width: f32,
    pub glow_spread: f32,
    pub glow_strength: f32,
    pub stroke_radius: f32,
    pub entrance_fork_spacing: u32,
    pub bolt_fork_spacing: u32,
    pub outline_fork_spacing: u32,
    pub halo_color: u32,
    pub core_color: u32,
    pub pulse_profile: PulseProfile,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PulseProfile { Original, Storm, Electric, Plasma }

impl PulseProfile {
    pub fn entrance(self, milliseconds: f32) -> f32 {
        match self {
            Self::Original => {
                if (120.0..142.0).contains(&milliseconds) || (155.0..174.0).contains(&milliseconds) { 1.0 }
                else if milliseconds < 180.0 { 0.5 }
                else { 0.72 + 0.28 * (milliseconds * 0.15).sin().abs() }
            }
            Self::Storm => match milliseconds as u32 % 95 {
                0..=24 => 1.35,
                25..=43 => 0.18,
                _ => 0.85,
            },
            Self::Electric => match milliseconds as u32 % 67 {
                0..=17 => 1.25,
                18..=34 => 0.12,
                _ => 0.7,
            },
            Self::Plasma => 0.8 + 0.2 * (milliseconds * 0.035).sin(),
        }
    }

    pub fn holding(self, milliseconds: f32, ambient: f32) -> f32 {
        if self == Self::Original { ambient } else { ambient * self.entrance(milliseconds) }
    }

    pub fn exit(self, elapsed: std::time::Duration) -> f32 {
        if self == Self::Original {
            match elapsed.as_millis() % 150 { 0..=55 => 1.45, 56..=90 => 0.0, _ => 1.1 }
        } else { self.entrance(elapsed.as_secs_f32() * 1000.0) * 1.35 }
    }
}

pub struct LightningPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub settings: LightningSettings,
}

pub const PRESETS: [LightningPreset; 4] = [
    LightningPreset { id: "original", label: "Original", settings: LightningSettings {
        roughness: 1.0, brightness: 1.0, core_width: 0.72, glow_spread: 6.0, glow_strength: 0.2, stroke_radius: 5.0,
        entrance_fork_spacing: 11, bolt_fork_spacing: 19, outline_fork_spacing: 29,
        halo_color: 0x8097ef, core_color: 0xf7fbff, pulse_profile: PulseProfile::Original,
    } },
    LightningPreset { id: "storm", label: "Storm", settings: LightningSettings {
        roughness: 1.7, brightness: 1.0, core_width: 0.85, glow_spread: 8.0, glow_strength: 0.25, stroke_radius: 5.0,
        entrance_fork_spacing: 5, bolt_fork_spacing: 7, outline_fork_spacing: 11,
        halo_color: 0x607fea, core_color: 0xffffff, pulse_profile: PulseProfile::Storm,
    } },
    LightningPreset { id: "electric", label: "Electric", settings: LightningSettings {
        roughness: 0.65, brightness: 1.0, core_width: 0.48, glow_spread: 3.5, glow_strength: 0.16, stroke_radius: 5.0,
        entrance_fork_spacing: 17, bolt_fork_spacing: 29, outline_fork_spacing: 43,
        halo_color: 0x36c8ed, core_color: 0xeaffff, pulse_profile: PulseProfile::Electric,
    } },
    LightningPreset { id: "plasma", label: "Plasma", settings: LightningSettings {
        roughness: 1.2, brightness: 1.0, core_width: 1.0, glow_spread: 16.0, glow_strength: 0.38, stroke_radius: 9.0,
        entrance_fork_spacing: 11, bolt_fork_spacing: 19, outline_fork_spacing: 29,
        halo_color: 0xb075f5, core_color: 0xfff0ff, pulse_profile: PulseProfile::Plasma,
    } },
];

impl Default for LightningSettings {
    fn default() -> Self { PRESETS[0].settings }
}

#[derive(Clone, Copy, Debug)]
pub enum Parameter { Roughness, Brightness, CoreWidth, GlowSpread, GlowStrength }

impl Parameter {
    pub fn get(self, settings: LightningSettings) -> f32 {
        match self {
            Self::Roughness => settings.roughness,
            Self::Brightness => settings.brightness,
            Self::CoreWidth => settings.core_width,
            Self::GlowSpread => settings.glow_spread,
            Self::GlowStrength => settings.glow_strength,
        }
    }

    pub fn set(self, settings: &mut LightningSettings, value: f32) {
        match self {
            Self::Roughness => settings.roughness = value,
            Self::Brightness => settings.brightness = value,
            Self::CoreWidth => settings.core_width = value,
            Self::GlowSpread => settings.glow_spread = value,
            Self::GlowStrength => settings.glow_strength = value,
        }
    }
}

#[derive(Clone, Copy)]
pub struct ParameterSpec {
    pub parameter: Parameter,
    pub id: &'static str,
    pub label: &'static str,
    pub min: f32,
    pub max: f32,
    pub step: f32,
}

pub const PARAMETERS: [ParameterSpec; 5] = [
    ParameterSpec { parameter: Parameter::Roughness, id: "lightning-roughness", label: "Roughness", min: 0.25, max: 2.5, step: 0.05 },
    ParameterSpec { parameter: Parameter::Brightness, id: "lightning-brightness", label: "Brightness", min: 0.1, max: 2.0, step: 0.05 },
    ParameterSpec { parameter: Parameter::CoreWidth, id: "lightning-core-width", label: "Core width", min: 0.2, max: 2.0, step: 0.01 },
    ParameterSpec { parameter: Parameter::GlowSpread, id: "lightning-glow-spread", label: "Glow spread", min: 1.0, max: 24.0, step: 0.5 },
    ParameterSpec { parameter: Parameter::GlowStrength, id: "lightning-glow-strength", label: "Glow strength", min: 0.05, max: 0.6, step: 0.01 },
];

impl ParameterSpec {
    pub fn clamp(self, value: f32) -> f32 { value.clamp(self.min, self.max) }
}

impl LightningSettings {
    pub fn preset(self) -> Option<&'static LightningPreset> { PRESETS.iter().find(|preset| preset.settings == self) }

    pub fn validate(self) -> Result<(), String> {
        for spec in PARAMETERS {
            let value = spec.parameter.get(self);
            if !value.is_finite() || !(spec.min..=spec.max).contains(&value) {
                return Err(format!("Lightning {} must be between {} and {}.", spec.label.to_lowercase(), spec.min, spec.max));
            }
        }
        if !self.stroke_radius.is_finite() || !(1.0..=16.0).contains(&self.stroke_radius) {
            return Err("Lightning stroke radius must be between 1 and 16.".into());
        }
        if [self.entrance_fork_spacing, self.bolt_fork_spacing, self.outline_fork_spacing].iter().any(|value| !(1..=128).contains(value)) {
            return Err("Lightning fork spacings must be between 1 and 128.".into());
        }
        if self.halo_color > 0xffffff || self.core_color > 0xffffff { return Err("Lightning colors must be RGB values.".into()); }
        if self.halo_color == 0xff00ff || self.core_color == 0xff00ff { return Err("Lightning colors must not use the reserved transparency value.".into()); }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_exact_complete_values_not_stored_names() {
        for preset in PRESETS {
            preset.settings.validate().unwrap();
            assert_eq!(preset.settings.preset().unwrap().id, preset.id);
            assert_eq!(serde_json::from_slice::<LightningSettings>(&serde_json::to_vec(&preset.settings).unwrap()).unwrap(), preset.settings);
            let mut custom = preset.settings;
            custom.brightness += 0.05;
            assert!(custom.preset().is_none());
        }
        assert_eq!(serde_json::from_str::<LightningSettings>("{}").unwrap(), PRESETS[0].settings);
        let partial: LightningSettings = serde_json::from_str(r#"{"brightness":1.5}"#).unwrap();
        assert_eq!(partial, LightningSettings { brightness: 1.5, ..LightningSettings::default() });
    }

    #[test]
    fn descriptor_bounds_and_nonfinite_values_are_rejected_without_clamping() {
        for spec in PARAMETERS {
            for value in [spec.min, spec.max] {
                let mut settings = LightningSettings::default();
                spec.parameter.set(&mut settings, value);
                settings.validate().unwrap();
                assert_eq!(spec.parameter.get(settings), value);
            }
            for value in [spec.min - 0.01, spec.max + 0.01, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut settings = LightningSettings::default();
                spec.parameter.set(&mut settings, value);
                assert!(settings.validate().is_err(), "Accepted {value} for {}", spec.label);
            }
        }
        for value in [0.0, 17.0, f32::NAN] { assert!(LightningSettings { stroke_radius: value, ..LightningSettings::default() }.validate().is_err()); }
        for value in [0, 129] {
            assert!(LightningSettings { entrance_fork_spacing: value, ..LightningSettings::default() }.validate().is_err());
            assert!(LightningSettings { bolt_fork_spacing: value, ..LightningSettings::default() }.validate().is_err());
            assert!(LightningSettings { outline_fork_spacing: value, ..LightningSettings::default() }.validate().is_err());
        }
        assert!(LightningSettings { halo_color: 0x1000000, ..LightningSettings::default() }.validate().is_err());
        assert!(LightningSettings { core_color: 0x1000000, ..LightningSettings::default() }.validate().is_err());
        assert!(LightningSettings { halo_color: 0xff00ff, ..LightningSettings::default() }.validate().is_err());
        assert!(LightningSettings { core_color: 0xff00ff, ..LightningSettings::default() }.validate().is_err());
    }

    #[test]
    fn pointer_rounded_endpoints_are_clamped_to_descriptor_bounds() {
        for spec in PARAMETERS {
            let pointer_value = |percentage: f32| {
                let value = spec.min + (spec.max - spec.min) * percentage;
                (value / spec.step).round() * spec.step
            };
            for percentage in [0.0, 1.0] {
                let mut settings = LightningSettings::default();
                let value = spec.clamp(pointer_value(percentage));
                spec.parameter.set(&mut settings, value);
                assert_eq!(spec.parameter.get(settings), value, "{} endpoint changed after clamping", spec.label);
                assert!((spec.min..=spec.max).contains(&value), "{} endpoint was outside its descriptor", spec.label);
                settings.validate().unwrap();
            }
        }
    }

    #[test]
    fn pulse_profiles_preserve_phase_distinctions_and_boundaries() {
        for (time, expected) in [(119., 0.5), (120., 1.), (141., 1.), (142., 0.5), (154., 0.5), (155., 1.), (173., 1.), (174., 0.5)] {
            assert_eq!(PulseProfile::Original.entrance(time), expected);
        }
        for time in 0..1000 {
            let ms = time as f32;
            assert_eq!(PulseProfile::Original.holding(ms, 0.4), 0.4);
            let elapsed = std::time::Duration::from_millis(time);
            assert_eq!(PulseProfile::Original.exit(elapsed), match time % 150 { 0..=55 => 1.45, 56..=90 => 0., _ => 1.1 });
            for profile in [PulseProfile::Storm, PulseProfile::Electric, PulseProfile::Plasma] {
                let expected = match profile {
                    PulseProfile::Storm => match time % 95 { 0..=24 => 1.35, 25..=43 => 0.18, _ => 0.85 },
                    PulseProfile::Electric => match time % 67 { 0..=17 => 1.25, 18..=34 => 0.12, _ => 0.7 },
                    _ => 0.8 + 0.2 * (ms * 0.035).sin(),
                };
                assert_eq!(profile.entrance(ms), expected);
                assert_eq!(profile.holding(ms, 0.4), 0.4 * expected);
                assert_eq!(profile.exit(elapsed), profile.entrance(elapsed.as_secs_f32() * 1000.0) * 1.35);
            }
        }
    }
}
