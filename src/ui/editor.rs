//! One native editing configuration for multiline values and source files.
use gpui_kit::{
    component::input::{EditorState, TabSize},
    *,
};

pub(crate) fn state(
    language: &str,
    value: impl Into<SharedString>,
    source: bool,
    window: &mut Window,
    cx: &mut Context<EditorState>,
) -> EditorState {
    EditorState::new(window, cx)
        .language(language.to_owned())
        .line_number(source)
        .folding(source)
        .auto_close(source)
        .smart_indent(source)
        .tab_size(TabSize {
            tab_size: 2,
            hard_tabs: false,
        })
        .default_value(value)
}

pub(crate) fn expand(
    state: Entity<EditorState>,
    title: SharedString,
    readonly: bool,
    window: &mut Window,
    cx: &mut App,
) {
    use crate::ui::{self, theme::layout};
    use gpui_kit::component::{
        WindowExt,
        button::{Button, ButtonVariants},
        h_flex,
        input::Editor,
    };
    window.open_dialog(cx, move |dialog, window, cx| {
        let state = state.clone();
        let label = title.clone();
        let copy = state.clone();
        ui::modal(
            dialog,
            title.clone(),
            layout::EDITOR_DIALOG_WIDTH,
            window,
            cx,
        )
        .content(ui::modal_panel(
            crate::ui::theme::layout::EDITOR_HEIGHT,
            move |_, _| {
                Editor::new(&state)
                    .w_full()
                    .h_full()
                    .readonly(readonly)
                    .aria_label(label.clone())
                    .into_any_element()
            },
        ))
        .footer(
            h_flex()
                .gap(crate::ui::theme::space::SM)
                .child(Button::new("copy-source").label("复制").ghost().on_click(
                    move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            copy.read(cx).value().to_string(),
                        ));
                    },
                ))
                .child(ui::modal_close("完成")),
        )
    });
}
