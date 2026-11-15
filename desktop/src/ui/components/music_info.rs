use async_channel::Receiver;
use gpui_kit::component::label::Label;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kgmusic_api::Song;

pub struct MusicInfo {
    song_data: Option<Song>,
    music_info_rx: Receiver<Song>,
}

impl MusicInfo {
    pub fn new(music_info_rx: Receiver<Song>) -> Self {
        Self {
            song_data: None,
            music_info_rx,
        }
    }

    pub fn back_update(&mut self, cx: &mut Context<Self>) {
        let rx = self.music_info_rx.clone();
        cx.spawn(async move |this, cx| {
            while let Ok(data) = rx.recv().await {
                if this
                    .update(cx, |this, cx| {
                        this.song_data = Some(data);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }
}

impl Render for MusicInfo {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .justify_center()
            .items_center()
            .flex_col()
            .when_some(self.song_data.as_ref(), |div, data| {
                div.flex()
                    .justify_center()
                    .items_center()
                    .gap(px(20.))
                    .when_some(data.album_image_url.as_ref(), |div, url| {
                        div.child(
                            img(url.clone())
                                .size(px(400.))
                                .object_fit(ObjectFit::Cover)
                                .rounded(px(30.))
                                .with_loading(|| {
                                    gpui_kit::div().child("图片加载中...").into_any_element()
                                })
                                .with_fallback(|| {
                                    gpui_kit::div().child("图片加载失败").into_any_element()
                                }),
                        )
                    })
                    .child(
                        Label::new(data.name.clone())
                            .text_size(px(40.))
                            .text_color(Rgba {
                                r: 1.0,
                                g: 0.6,
                                b: 0.6,
                                a: 1.0,
                            }),
                    )
                    .child(
                        Label::new(data.artist.clone())
                            .text_size(px(35.))
                            .text_color(Rgba {
                                r: 1.0,
                                g: 0.6,
                                b: 0.6,
                                a: 1.0,
                            }),
                    )
                    .child(
                        Label::new(data.album.clone())
                            .text_size(px(35.))
                            .text_color(Rgba {
                                r: 1.0,
                                g: 0.6,
                                b: 0.6,
                                a: 1.0,
                            }),
                    )
            })
    }
}
