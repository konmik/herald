use super::*;

#[test]
fn selects_installed_fonts_and_falls_back_for_missing_families() {
    let catalog = FontCatalog::new().unwrap();
    assert_eq!(catalog.get("Consolas").name(), Some("Consolas"));
    assert_eq!(catalog.get("Segoe UI").name(), Some("Segoe UI"));
    assert_eq!(catalog.get("Missing Herald Font").name(), catalog.fallback.name());
}

#[test]
fn enumeration_reads_only_the_font_signature() {
    for family in ["Consolas", "Segoe UI"] {
        let header = font_bytes(family, Some(4)).unwrap();
        assert_eq!(header.len(), 4);
        assert_eq!(header, font_data(family).unwrap()[..4]);
        assert!(supports_family(family));
    }
    assert!(!supports_family("Missing Herald Font"));
}
