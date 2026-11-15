use crate::utils::lyrics;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let item_time_str = self.lyric_item.time_str.clone();
        div()
            .flex()
            .relative()
            .h(px(30.))
            .w(Length::Definite(DefiniteLength::Fraction(0.6)))
            .justify_center()
            .items_center()
            .gap(px(20.))
            .flex_row()
            .child(
                Button::new("Seek")
                    .size_full()
                    .custom(
                        ButtonCustomVariant::new(cx)
                            .color(
                                Rgba {
                                    r: 0.1,
                                    g: 0.1,
                                    b: 0.1,
                                    a: 0.0,
                                }
                                .into(),
                            )
                            .hover(
                                Rgba {
                                    r: 0.1,
                                    g: 0.1,
                                    b: 0.1,
                                    a: 2.0,
                                }
                                .into(),
                            )
                            .active(
                                Rgba {
                                    r: 0.1,
                                    g: 0.1,
                                    b: 0.1,
                                    a: 3.0,
                                }
                                .into(),
                            ),
                    )
                    .absolute()
                    .right_0()
                    .bottom_0()
                    .on_click(move |_e, _w, _a| println!("Seek:{}", item_time_str)),
            )
            .child(
                Label::new(self.lyric_item.time_str.clone())
                    .text_size(px(15.))
                    .text_color(Rgba {
                        r: 0.8,
                        g: 0.8,
                        b: 0.8,
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
