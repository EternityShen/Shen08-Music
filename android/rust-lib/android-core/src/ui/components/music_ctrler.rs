use gpui::{
    AppContext, Context, DefiniteLength, Entity, IntoElement, Length, ParentElement, Render, Rgba,
    Styled, Window, div, px,
};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};

pub struct MusicCtrler {
    music_slider_state: Entity<SliderState>,
    current_value: f32,
}

impl MusicCtrler {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let slider_state = cx.new(|_| SliderState::new().min(0.0).max(100.0).default_value(25.0));

        let _ = cx.subscribe(
            &slider_state,
            |this, _, event: &SliderEvent, cx| match event {
                SliderEvent::Change(value) => {
                    this.current_value = value.start();
                    cx.notify();
                }
                SliderEvent::Release(_) => {}
            },
        );

        Self {
            music_slider_state: slider_state,
            current_value: 25.0,
        }
    }
}

impl Render for MusicCtrler {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .w(Length::Definite(DefiniteLength::Fraction(0.6)))
            .h(px(80.))
            .justify_center()
            .gap(px(10.))
            .items_center()
            .child(
                Slider::new(&self.music_slider_state)
                    .horizontal()
                    .bg(Rgba {
                        r: 0.8,
                        g: 0.5,
                        b: 0.6,
                        a: 1.0,
                    })
                    .w(Length::Definite(DefiniteLength::Fraction(0.5))),
            )
            .child(
                div()
                    .flex()
                    .w(Length::Definite(DefiniteLength::Fraction(0.8)))
                    .h(px(50.))
                    .items_center()
                    .justify_between(),
            )
    }
}
