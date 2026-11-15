use super::lyric_item::LyricItem;
use crate::utils::lyrics;
use async_channel::Receiver;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::*;

pub struct Lyrics {
    lyrics: Vec<lyrics::LyricItem>,
    lyric_items: Vec<Entity<LyricItem>>,
    current_time: f64,
    current_index: Option<usize>,
    lyrics_str_rx: Receiver<String>,
    time_rx: Receiver<f64>,
}

impl Lyrics {
    pub fn new(lyrics_str_rx: Receiver<String>, time_rx: Receiver<f64>) -> Self {
        Self {
            lyrics: Vec::new(),
            lyric_items: Vec::new(),
            current_index: None,
            current_time: 0.0,
            lyrics_str_rx,
            time_rx,
        }
    }

    pub fn back_update(&mut self, cx: &mut Context<Self>) {
        let time_rx = self.time_rx.clone();
        cx.spawn(async move |this, cx| {
            while let Ok(time) = time_rx.recv().await {
                let _ = this.update(cx, |this, cx| {
                    this.current_time = time;
                    cx.notify();
                });
            }
        })
        .detach();

        let lyrics_str_rx = self.lyrics_str_rx.clone();
        cx.spawn(async move |this, cx| {
            while let Ok(lyrics_str) = lyrics_str_rx.recv().await {
                let lyrics = lyrics::create_lrc_lyrics(1.0, lyrics_str);
                let items: Vec<_> = lyrics
                    .into_iter()
                    .map(|l| cx.new(|_cx| LyricItem::new(l.clone())))
                    .collect();
                let _ = this.update(cx, move |this, cx| {
                    this.lyric_items = items;
                    cx.notify();
                });
            }
        })
        .detach();
    }
}

impl Render for Lyrics {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .h(Length::Definite(DefiniteLength::Fraction(0.7)))
            .justify_center()
            .items_center()
            .gap(px(10.))
            .overflow_y_scrollbar()
            .flex_col()
            .children(self.lyric_items.clone())
    }
}
