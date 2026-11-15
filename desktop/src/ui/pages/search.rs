use crate::music_player::engine::Event;
use crate::ui::components::music_item_card::MusicItemCard;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kgmusic_api::Song;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

pub struct SearchPage {
    input_state: Entity<InputState>,
    music_api: Arc<Mutex<kgmusic_api::MusicSource>>,
    music_engine_event_sender: Sender<Event>,
    music_info_sender: Sender<Song>,
    cards: Vec<Entity<MusicItemCard>>,
    is_searching: bool,
}

impl SearchPage {
    pub fn new(
        cx: &mut Context<Self>,
        window: &mut Window,
        music_api: Arc<Mutex<kgmusic_api::MusicSource>>,
        music_engine_event_sender: Sender<Event>,
        music_info_sender: Sender<Song>,
    ) -> Self {
        let input_state = cx.new(|cx| InputState::new(window, cx).placeholder("歌名"));

        cx.subscribe(&input_state, |this, _input, event, cx| {
            if let InputEvent::Change = event {
                let query = this.input_state.read(cx).value().to_string();
                this.perform_search(query, cx);
            }
        })
        .detach();

        Self {
            input_state,
            music_api,
            music_engine_event_sender,
            music_info_sender,
            cards: Vec::new(),
            is_searching: false,
        }
    }

    fn perform_search(&mut self, query: String, cx: &mut Context<Self>) {
        if query.trim().is_empty() {
            self.cards.clear();
            cx.notify();
            return;
        }

        self.is_searching = true;
        cx.notify();

        let music_api = self.music_api.clone();
        let event_sender = self.music_engine_event_sender.clone();
        let info_sender = self.music_info_sender.clone();
        cx.spawn(async move |this, cx| {
            let results = cx
                .background_executor()
                .spawn(async move {
                    if let Ok(api) = music_api.lock() {
                        api.search(&query, 25).unwrap_or_default()
                    } else {
                        Vec::new()
                    }
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.cards = results
                    .into_iter()
                    .map(|song| {
                        let event_sender = event_sender.clone();
                        let info_sender = info_sender.clone();
                        let api = this.music_api.clone();
                        cx.new(|_cx| MusicItemCard::new(song, event_sender, info_sender, api))
                    })
                    .collect();

                this.is_searching = false;
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for SearchPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .when(self.is_searching, |this| this.child("搜索中..."))
                    .children(self.cards.clone()),
            )
    }
}
