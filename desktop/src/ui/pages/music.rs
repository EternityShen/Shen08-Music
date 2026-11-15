use crate::ui::components::lyrics::Lyrics;
use crate::ui::components::music_info::MusicInfo;
use async_channel::{Receiver, Sender};
use gpui_kit::*;
use kgmusic_api::Song;

pub struct MusicPage {
    music_info: Entity<MusicInfo>,
    lyrics: Entity<Lyrics>,
}

impl MusicPage {
    pub fn new(
        cx: &mut Context<Self>,
        music_info_rx: Receiver<Song>,
        lyrics_str_rx: Receiver<String>,
        time_rx: Receiver<f64>,
    ) -> Self {
        let music_info = cx.new(|cx| {
            let mut m_i = MusicInfo::new(music_info_rx);
            m_i.back_update(cx);
            m_i
        });

        let lyrics = cx.new(|cx| {
            let mut lyrics = Lyrics::new(lyrics_str_rx, time_rx);
            lyrics.back_update(cx);
            lyrics
        });

        Self { music_info, lyrics }
    }
}

impl Render for MusicPage {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .flex_row()
            .child(
                div()
                    .flex()
                    .h_full()
                    .w(Length::Definite(DefiniteLength::Fraction(0.5)))
                    .justify_center()
                    .items_center()
                    .child(self.music_info.clone()),
            )
            .child(
                div()
                    .flex()
                    .h_full()
                    .w(Length::Definite(DefiniteLength::Fraction(0.5)))
                    .justify_center()
                    .items_center()
                    .child(self.lyrics.clone()),
            )
    }
}
