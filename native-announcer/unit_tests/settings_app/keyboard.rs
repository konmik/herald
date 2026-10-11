use super::{reveal_delta, NamedEditor};
use gpui_kit::{Element, InteractiveElement, IntoElement};

#[test]
fn focused_control_is_revealed_in_either_direction() {
    assert_eq!(reveal_delta(10., 30., 20., 100.), 10.);
    assert_eq!(reveal_delta(90., 120., 20., 100.), -20.);
    assert_eq!(reveal_delta(30., 80., 20., 100.), 0.);
    assert_eq!(reveal_delta(20., 150., 20., 100.), 0.);
}

#[test]
fn textarea_metadata_belongs_to_the_editor_element() {
    let editor = NamedEditor {
        editor: gpui_kit::div().id("input-state").into_element(),
        id: "summary-prompt",
        label: "Summary prompt",
        value: Some("First line\nSecond line".into()),
        readonly: false,
        role: gpui_kit::Role::MultilineTextInput,
    };
    assert_eq!(editor.id(), Some("input-state".into()));
    assert_eq!(editor.a11y_role(), Some(gpui_kit::Role::MultilineTextInput));
    let mut node = gpui_kit::accesskit::Node::new(editor.a11y_role().unwrap());
    editor.write_a11y_info(&mut node);
    assert_eq!(node.author_id(), Some("summary-prompt"));
    assert_eq!(node.label(), Some("Summary prompt"));
    assert_eq!(node.value(), Some("First line\nSecond line"));
}

#[cfg(target_os = "macos")]
#[test]
fn quit_and_close_window_have_macos_shortcuts_and_menu_items() {
    use super::{bindings, menus, CloseWindow, Quit};
    use gpui_kit::{Action, KeyBinding, MenuItem};
    let bindings = bindings();
    let bound = |keys: &str, action: &dyn Action| bindings.iter().any(|binding| {
        binding.keystrokes() == KeyBinding::new(keys, Quit, None).keystrokes() && binding.action().partial_eq(action)
    });
    assert!(bound("cmd-q", &Quit), "Cmd-Q must quit");
    assert!(bound("cmd-w", &CloseWindow), "Cmd-W must close the window");
    let items: Vec<(String, Box<dyn Action>)> = menus().into_iter().flat_map(|menu| menu.items).filter_map(|item| match item {
        MenuItem::Action { name, action, .. } => Some((name.to_string(), action)),
        _ => None,
    }).collect();
    assert!(items.iter().any(|(name, action)| name == "Quit Herald" && action.partial_eq(&Quit)), "the app menu must offer Quit");
    assert!(items.iter().any(|(name, action)| name == "Close Window" && action.partial_eq(&CloseWindow)), "the menu bar must offer Close Window");
}
