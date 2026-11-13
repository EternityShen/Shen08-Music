use crate::ui::pages::home::HomePage;
use crate::ui::pages::music::MusicPage;
use gpui::{Context, Entity, IntoElement, ParentElement, Render, Rgba, Styled, Window, div, px};
use gpui_kit::base::{NavMotion, NavStackState};
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};

pub struct NavigatorBar {
    navigator: Entity<NavStackState>,
    music_page: Entity<MusicPage>,
    home_page: Entity<HomePage>,
}

impl NavigatorBar {
    pub fn new(
        navigator: Entity<NavStackState>,
        music_page: Entity<MusicPage>,
        home_page: Entity<HomePage>,
    ) -> Self {
        Self {
            navigator,
            music_page,
            home_page,
        }
    }
}

impl Render for NavigatorBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let navigator_home = self.navigator.clone();
        let navigator_music = self.navigator.clone();
        let music_page = self.music_page.clone();
        let home_page = self.home_page.clone();

        div()
            .flex()
            .absolute()
            .right_0()
            .bottom_0()
            .w_full()
            .h(px(60.))
            .justify_between()
            .items_center()
            .flex_row()
            .child(
                Button::new("Home")
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
                    .on_click(move |_e, _w, cx| {
                        navigator_home.update(cx, |nav, nav_cx| {
                            nav.replace(home_page.clone(), NavMotion::Animated, nav_cx);
                        });
                    })
                    .size(px(40.))
                    .label("H"),
            )
            .child(
                Button::new("Music")
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
                    .on_click(move |_e, _w, cx| {
                        navigator_music.update(cx, |nav, nav_cx| {
                            nav.replace(music_page.clone(), NavMotion::Animated, nav_cx);
                        });
                    })
                    .size(px(40.))
                    .label("M"),
            )
    }
}
