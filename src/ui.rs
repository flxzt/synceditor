use crate::state::{SyncedBufferController, CHANGE_TOPIC, DIAL, SEND_WIRE_MESSAGE};
use crate::AppState;
use druid::widget::{Button, Flex, RawLabel, Scroll, Split, TextBox};
use druid::{Color, Widget, WidgetExt, WindowDesc};

pub(crate) const WINDOW_TITLE: &str = "Sync-Editor";
pub(crate) const WINDOW_SIZE_INIT: (f64, f64) = (800.0, 600.0);

pub(crate) fn new_main_window() -> WindowDesc<AppState> {
    WindowDesc::new(build_root_widget())
        .title(WINDOW_TITLE)
        .window_size(WINDOW_SIZE_INIT)
}

pub(crate) fn build_root_widget() -> impl Widget<AppState> {
    Flex::column()
        .with_flex_child(
            Flex::row()
                .with_flex_child(
                    TextBox::new()
                        .with_placeholder("Topic")
                        .lens(AppState::topic)
                        .expand_width(),
                    0.2,
                )
                .with_spacer(5.0)
                .with_flex_child(
                    Button::new("Change").on_click(|event_cx, state: &mut AppState, _| {
                        event_cx.submit_command(CHANGE_TOPIC.with(state.topic.clone()))
                    }),
                    0.0,
                )
                .with_spacer(20.0)
                .with_flex_child(
                    TextBox::new()
                        .with_placeholder("Peer Address")
                        .lens(AppState::connect_address)
                        .expand_width(),
                    0.8,
                )
                .with_spacer(5.0)
                .with_flex_child(
                    Button::new("Connect").on_click(|event_cx, state: &mut AppState, _| {
                        event_cx.submit_command(DIAL.with(state.connect_address.clone()));

                        event_cx.submit_command(SEND_WIRE_MESSAGE.with(
                            crate::connection::WireMessage::SyncDoc(state.buffer.doc_save()),
                        ));
                        event_cx.submit_command(SEND_WIRE_MESSAGE.with(
                            crate::connection::WireMessage::MergeDoc(state.buffer.doc_save()),
                        ));
                    }),
                    0.0,
                ),
            0.0,
        )
        .with_spacer(5.0)
        .with_flex_child(
            Split::columns(
                TextBox::multiline()
                    .controller(SyncedBufferController)
                    .lens(AppState::buffer)
                    .expand(),
                Scroll::new(
                    RawLabel::new()
                        .with_text_color(Color::BLACK)
                        .with_text_size(11.0)
                        .lens(AppState::log),
                )
                .expand()
                .background(Color::grey8(222)),
            )
            .draggable(true),
            1.0,
        )
        .padding(5.0)
}
