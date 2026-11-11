use super::*;
use ab_glyph::Font as _;

#[test]
fn selects_installed_fonts_and_falls_back_for_missing_families() {
    let catalog = FontCatalog::new();
    for family in ["Consolas", "Segoe UI"] {
        assert_eq!(catalog.get(family).unwrap().font_data(), font_data(family).unwrap());
    }
    let first = catalog.get("Missing Herald Font").unwrap();
    let second = catalog.get("Another Missing Herald Font").unwrap();
    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn fonts_are_loaded_on_request_and_shared_between_styles() {
    let catalog = FontCatalog::new();
    assert!(catalog.faces.borrow().is_empty());
    let body = catalog.get("Segoe UI").unwrap();
    let title = catalog.get("Segoe UI").unwrap();
    assert!(Arc::ptr_eq(&body, &title));
    assert_eq!(catalog.faces.borrow().len(), 1);
    let previous = Arc::downgrade(&body);
    drop((body, title));
    assert!(previous.upgrade().is_none());
    let selected = catalog.get("Consolas").unwrap();
    assert_eq!(catalog.faces.borrow().len(), 1);
    assert_eq!(selected.font_data(), font_data("Consolas").unwrap());
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
