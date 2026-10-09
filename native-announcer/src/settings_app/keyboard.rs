use gpui_kit::prelude::*;
use gpui_kit::base::StyledExt;
use gpui_kit::{div, px, AnyElement, App, IntoElement, ParentElement, RenderOnce, ScrollHandle, SharedString, Window};
use std::{cell::Cell, rc::Rc};

gpui_kit::actions!(herald_settings, [Apply, NextPage, PreviousPage, NextField, PreviousField, NextItem, PreviousItem, FirstItem, LastItem, FirstChoice, LastChoice, Activate, Increase, Decrease, Minimum, Maximum]);

pub fn init(cx: &mut App) {
    use gpui_kit::KeyBinding;
    cx.bind_keys([
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-s", Apply, Some("HeraldSettings")),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-s", Apply, Some("HeraldSettings")),
        KeyBinding::new("ctrl-tab", NextPage, Some("HeraldSettings")),
        KeyBinding::new("ctrl-shift-tab", PreviousPage, Some("HeraldSettings")),
        KeyBinding::new("tab", NextField, Some("HeraldSettings")),
        KeyBinding::new("shift-tab", PreviousField, Some("HeraldSettings")),
        KeyBinding::new("tab", NextField, Some("HeraldSettings > Input")),
        KeyBinding::new("shift-tab", PreviousField, Some("HeraldSettings > Input")),
        KeyBinding::new("space", gpui_kit::base::actions::Confirm { secondary: false }, Some("HeraldSettings > Select && !Input")),
        KeyBinding::new("home", FirstChoice, Some("HeraldSettings > List && !Input")),
        KeyBinding::new("end", LastChoice, Some("HeraldSettings > List && !Input")),
        KeyBinding::new("right", NextItem, Some("HeraldNavHorizontal")),
        KeyBinding::new("left", PreviousItem, Some("HeraldNavHorizontal")),
        KeyBinding::new("down", NextItem, Some("HeraldNavVertical")),
        KeyBinding::new("up", PreviousItem, Some("HeraldNavVertical")),
        KeyBinding::new("home", FirstItem, Some("HeraldNavigation")),
        KeyBinding::new("end", LastItem, Some("HeraldNavigation")),
        KeyBinding::new("down", NextItem, Some("HeraldCharacters")),
        KeyBinding::new("up", PreviousItem, Some("HeraldCharacters")),
        KeyBinding::new("home", FirstItem, Some("HeraldCharacters")),
        KeyBinding::new("end", LastItem, Some("HeraldCharacters")),
        KeyBinding::new("enter", Activate, Some("HeraldNavigation")),
        KeyBinding::new("space", Activate, Some("HeraldNavigation")),
        KeyBinding::new("right", Increase, Some("HeraldSlider")),
        KeyBinding::new("up", Increase, Some("HeraldSlider")),
        KeyBinding::new("left", Decrease, Some("HeraldSlider")),
        KeyBinding::new("down", Decrease, Some("HeraldSlider")),
        KeyBinding::new("home", Minimum, Some("HeraldSlider")),
        KeyBinding::new("end", Maximum, Some("HeraldSlider")),
    ]);
}

// Kit 0.7.1 labels its textarea frame, but focuses an inner editor with no
// accessibility role. Attach metadata to that editor's own element and handle.
pub fn textarea(
    state: &gpui_kit::Entity<gpui_kit::component::input::TextareaState>,
    id: &'static str,
    label: &'static str,
    height: f32,
    readonly: bool,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    use gpui_kit::{Focusable, Render};
    use gpui_kit::component::ActiveTheme;
    let focused = state.read(cx).focus_handle(cx).is_focused(window);
    let value = window.is_a11y_active().then(|| state.read(cx).value().to_string());
    let editor = state.update(cx, |state, cx| {
        state.set_readonly(readonly, cx);
        state.set_editor_paddings(gpui_kit::Edges::all(px(8.)));
        NamedEditor { editor: state.render(window, cx).into_element(), id, label, value, readonly, role: gpui_kit::Role::MultilineTextInput }.into_any_element()
    });
    div().id(id).flex().w_full().h(px(height)).min_w_0().overflow_hidden()
        .border_1().rounded(cx.theme().radius).bg(cx.theme().input)
        .border_color(if focused { cx.theme().ring } else { cx.theme().border })
        .child(editor)
}

struct NamedEditor<E: gpui_kit::Element> {
    editor: E,
    id: &'static str,
    label: &'static str,
    value: Option<String>,
    readonly: bool,
    role: gpui_kit::Role,
}

impl<E: gpui_kit::Element> IntoElement for NamedEditor<E> {
    type Element = Self;
    fn into_element(self) -> Self { self }
}

impl<E: gpui_kit::Element> gpui_kit::Element for NamedEditor<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<gpui_kit::ElementId> { self.editor.id() }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> { self.editor.source_location() }
    fn request_layout(&mut self, id: Option<&gpui_kit::GlobalElementId>, inspector: Option<&gpui_kit::InspectorElementId>, window: &mut Window, cx: &mut App) -> (gpui_kit::LayoutId, Self::RequestLayoutState) {
        self.editor.request_layout(id, inspector, window, cx)
    }
    fn prepaint(&mut self, id: Option<&gpui_kit::GlobalElementId>, inspector: Option<&gpui_kit::InspectorElementId>, bounds: gpui_kit::Bounds<gpui_kit::Pixels>, layout: &mut Self::RequestLayoutState, window: &mut Window, cx: &mut App) -> Self::PrepaintState {
        self.editor.prepaint(id, inspector, bounds, layout, window, cx)
    }
    fn paint(&mut self, id: Option<&gpui_kit::GlobalElementId>, inspector: Option<&gpui_kit::InspectorElementId>, bounds: gpui_kit::Bounds<gpui_kit::Pixels>, layout: &mut Self::RequestLayoutState, prepaint: &mut Self::PrepaintState, window: &mut Window, cx: &mut App) {
        self.editor.paint(id, inspector, bounds, layout, prepaint, window, cx);
    }
    fn a11y_role(&self) -> Option<gpui_kit::Role> { Some(self.role) }
    fn write_a11y_info(&self, node: &mut gpui_kit::accesskit::Node) {
        self.editor.write_a11y_info(node);
        node.set_author_id(self.id);
        node.set_label(self.label);
        if let Some(value) = &self.value { node.set_value(value.clone()); }
        if self.readonly { node.set_read_only(); }
    }
}

pub fn input(state: &gpui_kit::Entity<gpui_kit::component::input::InputState>, id: &'static str, label: &'static str, window: &mut Window, cx: &mut App) -> impl IntoElement {
    use gpui_kit::{Focusable, Render};
    use gpui_kit::component::ActiveTheme;
    let focused = state.read(cx).focus_handle(cx).is_focused(window);
    let value = window.is_a11y_active().then(|| state.read(cx).value().to_string());
    let editor = state.update(cx, |state, cx| {
        state.set_disabled(false, cx);
        state.set_editor_paddings(gpui_kit::Edges::all(px(8.)));
        NamedEditor { editor: state.render(window, cx).into_element(), id, label, value, readonly: false, role: gpui_kit::Role::TextInput }.into_any_element()
    });
    div().id(id).flex().w_full().h_8().min_w_0().overflow_hidden().border_1().rounded(cx.theme().radius).bg(cx.theme().input)
        .border_color(if focused { cx.theme().ring } else { cx.theme().border }).child(editor)
}

pub trait RevealFocused: IntoElement {
    fn reveal(self, id: &'static str, scroll: &ScrollHandle) -> Reveal {
        Reveal { id: id.into(), child: self.into_any_element(), scroll: scroll.clone(), style: Default::default() }
    }
}
impl<T: IntoElement> RevealFocused for T {}

#[derive(IntoElement)]
pub struct Reveal {
    id: SharedString,
    child: AnyElement,
    scroll: ScrollHandle,
    style: gpui_kit::StyleRefinement,
}

struct RevealState {
    focus: gpui_kit::FocusHandle,
    previous: Rc<Cell<(bool, gpui_kit::Size<gpui_kit::Pixels>, gpui_kit::Size<gpui_kit::Pixels>)>>,
}

impl gpui_kit::Styled for Reveal {
    fn style(&mut self) -> &mut gpui_kit::StyleRefinement { &mut self.style }
}

impl RenderOnce for Reveal {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| RevealState {
            focus: cx.focus_handle(), previous: Rc::new(Cell::new((false, Default::default(), Default::default()))),
        });
        let focus = state.read(cx).focus.clone();
        let previous = state.read(cx).previous.clone();
        div().id(self.id).relative().min_w_0().refine_style(&self.style).track_focus(&focus).tab_stop(false)
            .child(self.child)
            .child(gpui_kit::canvas(move |bounds, window, cx| {
                let viewport = self.scroll.bounds();
                let focused = focus.contains_focused(window, cx);
                let last = previous.replace((focused, viewport.size, bounds.size));
                if !focused || (last.0 && last.1 == viewport.size && last.2 == bounds.size) { return; }
                if viewport.size.height <= px(0.) { return; }
                let delta = reveal_delta(f32::from(bounds.top()), f32::from(bounds.bottom()), f32::from(viewport.top()), f32::from(viewport.bottom()));
                if delta.abs() > 0.5 {
                    let scroll = self.scroll;
                    let measured_offset = scroll.offset();
                    // The parent's prepaint has already positioned every child.
                    // Commit between frames so geometry and scrollbar agree.
                    window.on_next_frame(move |window, cx| {
                        if !focus.contains_focused(window, cx) || scroll.offset() != measured_offset { return; }
                        let mut offset = measured_offset;
                        offset.y += px(delta);
                        scroll.set_offset(offset);
                        window.refresh();
                    });
                }
            }, |_, _, _, _| {}).absolute().inset_0())
    }
}

fn reveal_delta(top: f32, bottom: f32, viewport_top: f32, viewport_bottom: f32) -> f32 {
    if bottom - top > viewport_bottom - viewport_top { return viewport_top - top; }
    if top < viewport_top { viewport_top - top }
    else if bottom > viewport_bottom { viewport_bottom - bottom }
    else { 0. }
}

#[cfg(test)]
mod tests {
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
}
