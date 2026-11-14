use crate::ui::components::music_item_card::MusicItemCard;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::*;
use std::sync::{Arc, Mutex};

pub struct SearchPage {
    input_state: Entity<InputState>,
    music_api: Arc<Mutex<kgmusic_api::client::Api>>,
}

impl SearchPage {
    pub fn new(
        cx: &mut Context<Self>,
        window: &mut Window,
        music_api: Arc<Mutex<kgmusic_api::client::Api>>,
    ) -> Self {
        let input_state = cx.new(|cx| InputState::new(window, cx).placeholder("歌名"));
        Self {
            input_state,
            music_api,
        }
    }
}

impl Render for SearchPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let items = if !self.input_state.read(cx).value().is_empty() {
            if let Ok(api) = self.music_api.lock() {
                api.search(self.input_state.read(cx).value().as_str(), 25)
                    .unwrap_or_default()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        div()
            .flex()
            .relative()
            .size_full()
            .justify_start()
            .items_start()
            .flex_col()
            .child(
                div()
                    .flex()
                    .w_full()
                    .h(px(40.))
                    .bg(Rgba {
                        r: 0.1,
                        g: 0.1,
                        b: 0.1,
                        a: 1.0,
                    })
                    .justify_center()
                    .items_center()
                    .child(
                        Input::new(&self.input_state)
                            .w(px(200.))
                            .bg(Rgba {
                                r: 0.2,
                                g: 0.2,
                                b: 0.2,
                                a: 1.0,
                            })
                            .text_color(Rgba {
                                r: 1.0,
                                g: 1.0,
                                b: 1.0,
                                a: 1.0,
                            })
                            .rounded(px(10.))
                            .border(px(2.0)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .size_full()
                    .justify_start()
                    .items_start()
                    .overflow_y_scrollbar()
                    .gap(px(10.))
                    .flex_col()
                    .children(
                        items
                            .into_iter()
                            .map(|item| cx.new(|_cx| MusicItemCard::new(item))),
                    ),
            )
    }
}
