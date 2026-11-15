use crate::ui::components::music_info::MusicInfo;
use async_channel::Receiver;
use gpui_kit::*;
use kgmusic_api::Song;

pub struct MusicPage {
    music_info: Entity<MusicInfo>,
}

impl MusicPage {
    pub fn new(cx: &mut Context<Self>, music_info_rx: Receiver<Song>) -> Self {
        let music_info = cx.new(|cx| {
            let mut m_i = MusicInfo::new(music_info_rx);
            m_i.back_update(cx);
            m_i
        });

        Self { music_info }
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
                    .items_center(),
            )
    }
}
