use crate::music_player::engine::Event;
use async_channel::Sender;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::label::Label;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kgmusic_api::{Quality, Song};
use std::sync::{Arc, Mutex};

pub struct MusicItemCard {
    song_data: Song,
    music_engine_event_sender: Sender<Event>,
    music_info_sender: Sender<Song>,
    image_url: Option<String>,
    song_url: Option<String>,
    music_api: Arc<Mutex<kgmusic_api::MusicSource>>,
    is_loaded: bool,
}

impl MusicItemCard {
    pub fn new(
        song_data: Song,
        music_engine_event_sender: Sender<Event>,
        music_info_sender: Sender<Song>,
        music_api: Arc<Mutex<kgmusic_api::MusicSource>>,
    ) -> Self {
        Self {
            song_data,
            music_engine_event_sender,
            music_info_sender,
            image_url: None,
            song_url: None,
            music_api,
            is_loaded: false,
        }
    }

    pub fn get_url(&mut self, cx: &mut Context<Self>) {
        if self.is_loaded {
            return;
        }
        self.is_loaded = true;

        let image_url = self.song_data.album_image_url.clone();

        let music_api = self.music_api.clone();
        let song_data = self.song_data.clone();

        cx.spawn(async move |this, cx| {
            let song_url = if let Ok(api) = music_api.lock() {
                match api.song_url(&song_data, Quality::Standard) {
                    Ok(data) => Some(data.0),
                    Err(_) => None,
                }
            } else {
                None
            };

            let _ = this.update(cx, |this, cx| {
                this.song_url = song_url;
                this.image_url = image_url;
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for MusicItemCard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.get_url(cx);

        div()
            .flex()
            .h(px(60.))
            .w_full()
            .pr(px(20.))
            .pl(px(20.))
            .flex_row()
            .justify_between()
            .items_center()
            .bg(Rgba {
                r: 0.1,
                g: 0.1,
                b: 0.1,
                a: 1.0,
            })
            .child(
                div()
                    .flex()
                    .h_full()
                    .flex_row()
                    .gap(px(10.))
                    .when_some(self.image_url.as_ref(), |div, url| {
                        div.child(
                            img(url.clone())
                                .object_fit(ObjectFit::Cover)
                                .rounded(px(10.))
                                .w(px(50.))
                                .h(px(50.)),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .h_full()
                            .flex_col()
                            .gap(px(10.))
                            .child(
                                Label::new(self.song_data.name.clone())
                                    .text_color(Rgba {
                                        r: 1.0,
                                        g: 1.0,
                                        b: 1.0,
                                        a: 1.0,
                                    })
                                    .text_size(px(20.)),
                            )
                            .child(Label::new(self.song_data.artist.clone()).text_color(Rgba {
                                r: 1.0,
                                g: 1.0,
                                b: 1.0,
                                a: 1.0,
                            }))
                            .text_size(px(15.)),
                    ),
            )
            .when_some(self.song_url.as_ref(), |div, url| {
                let url_cloned = url.clone();
                let song_cloned = self.song_data.clone();
                let engine_event_sender = self.music_engine_event_sender.clone();
                let music_info_sender = self.music_info_sender.clone();

                div.child(
                    Button::new("play")
                        .custom(
                            ButtonCustomVariant::new(cx)
                                .color(
                                    Rgba {
                                        r: 0.8,
                                        g: 0.5,
                                        b: 0.6,
                                        a: 1.0,
                                    }
                                    .into(),
                                )
                                .hover(
                                    Rgba {
                                        r: 0.7,
                                        g: 0.5,
                                        b: 0.6,
                                        a: 1.0,
                                    }
                                    .into(),
                                )
                                .active(
                                    Rgba {
                                        r: 0.6,
                                        g: 0.5,
                                        b: 0.6,
                                        a: 1.0,
                                    }
                                    .into(),
                                ),
                        )
                        .rounded(px(10.))
                        .label(">")
                        .size(px(40.))
                        .on_click(move |_e, _w, _a| {
                            let _ = engine_event_sender
                                .send_blocking(Event::PlayUrl(url_cloned.clone()));
                            let _ = music_info_sender.send_blocking(song_cloned.clone());
                        }),
                )
            })
    }
}
