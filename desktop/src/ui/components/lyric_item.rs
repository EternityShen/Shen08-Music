use crate::utils::lyrics;
use gpui_kit::component::label::Label;
use gpui_kit::*;

pub struct LyricItem {
    lyric_item: lyrics::LyricItem,
}

impl LyricItem {
    pub fn new(lyric_item: lyrics::LyricItem) -> Self {
        Self { lyric_item }
    }
}

impl Render for LyricItem {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .h(px(30.))
            .w(Length::Definite(DefiniteLength::Fraction(0.6)))
            .justify_between()
            .items_center()
            .flex_row()
            .child(
                Label::new(self.lyric_item.time_str.clone())
                    .text_size(px(15.))
                    .text_color(Rgba {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 1.0,
                    }),
            )
            .child(
                Label::new(self.lyric_item.text.clone())
                    .text_size(px(15.))
                    .text_color(Rgba {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 1.0,
                    }),
            )
    }
}
