use crate::ui::components::music_ctrler::MusicCtrler;
use crate::ui::components::music_info::MusicInfo;
use gpui::{
    AppContext, Context, DefiniteLength, Entity, IntoElement, Length, ParentElement, Render,
    Styled, Window, div,
};

pub struct MusicPage {
    music_info: Entity<MusicInfo>,
    music_ctrler: Entity<MusicCtrler>,
}

impl MusicPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let music_info = cx.new(|_cx| MusicInfo::new());
        let music_ctrler = cx.new(|cx| MusicCtrler::new(cx));
        Self {
            music_info,
            music_ctrler,
        }
    }
}

impl Render for MusicPage {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .justify_center()
            .items_center()
            .flex_col()
            .child(
                div()
                    .flex()
                    .w_full()
                    .h(Length::Definite(DefiniteLength::Fraction(0.6)))
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .child(self.music_info.clone()),
            )
            .child(
                div()
                    .flex()
                    .w_full()
                    .h(Length::Definite(DefiniteLength::Fraction(0.4)))
                    .flex_col()
                    .justify_center()
                    .items_center()
                    .child(self.music_ctrler.clone()),
            )
    }
}
