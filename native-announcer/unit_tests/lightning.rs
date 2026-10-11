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
    let timing: LightningSettings = serde_json::from_str(r#"{"frequency":1.5,"speed":2.0}"#).unwrap();
    assert_eq!(timing, LightningSettings { frequency: 1.5, speed: 2.0, ..LightningSettings::default() });
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
