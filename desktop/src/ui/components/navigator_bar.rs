use crate::ui::pages::home::HomePage;
use crate::ui::pages::music::MusicPage;
use crate::ui::pages::search::SearchPage;
use gpui_kit::base::{NavMotion, NavStackState};
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::*;

pub struct NavigatorBar {
    navigator: Entity<NavStackState>,
    music_page: Entity<MusicPage>,
    home_page: Entity<HomePage>,
    search_page: Entity<SearchPage>,
}

impl NavigatorBar {
    pub fn new(
        navigator: Entity<NavStackState>,
        music_page: Entity<MusicPage>,
        home_page: Entity<HomePage>,
        search_page: Entity<SearchPage>,
    ) -> Self {
        Self {
            navigator,
            music_page,
            home_page,
            search_page,
        }
    }
}

impl Render for NavigatorBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let music_nav = self.navigator.clone();
        let home_nav = self.navigator.clone();
        let search_nav = self.navigator.clone();

        let music_page = self.music_page.clone();
        let home_page = self.home_page.clone();
        let search_page = self.search_page.clone();

        div()
            .flex()
            .h(px(80.))
            .w_full()
            .absolute()
            .bottom_1()
            .right_0()
            .justify_center()
            .items_center()
            .flex_row()
            .child(
                div()
                    .flex()
                    .h(px(80.))
                    .w(px(160.))
                    .flex_row()
                    .justify_between()
                    .items_center()
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
                                home_nav.update(cx, |nav, nav_cx| {
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
                                music_nav.update(cx, |nav, nav_cx| {
                                    nav.replace(music_page.clone(), NavMotion::Animated, nav_cx);
                                });
                            })
                            .size(px(40.))
                            .label("M"),
                    )
                    .child(
                        Button::new("Search")
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
                                search_nav.update(cx, |nav, nav_cx| {
                                    nav.replace(search_page.clone(), NavMotion::Animated, nav_cx);
                                });
                            })
                            .size(px(40.))
                            .label("S"),
                    ),
            )
    }
}
