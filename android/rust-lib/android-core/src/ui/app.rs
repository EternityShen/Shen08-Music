use crate::ui::components::navigator_bar::NavigatorBar;
use crate::ui::pages::home::HomePage;
use crate::ui::pages::music::MusicPage;
use gpui::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div, relative,
};
use gpui_kit::base::{NavMotion, NavOperation, NavStack, NavStackState, PresencePhase, Transition};
use std::time::Duration;

pub struct App {
    navigator: Entity<NavStackState>,
    navigator_bar: Entity<NavigatorBar>,
    music_page: Entity<MusicPage>,
    home_page: Entity<HomePage>,
}

impl App {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let nav = cx.new(|_| NavStackState::new());
        let music_page = cx.new(|cx| MusicPage::new(cx));
        let home_page = cx.new(|cx| HomePage::new());
        let nav_bar =
            cx.new(|_| NavigatorBar::new(nav.clone(), music_page.clone(), home_page.clone()));
        nav.update(cx, |nav_state, cx| {
            nav_state.push(home_page.clone(), NavMotion::Animated, cx)
        });
        Self {
            navigator: nav,
            navigator_bar: nav_bar,
            music_page,
            home_page,
        }
    }
}

impl Render for App {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .relative()
            .child(
                NavStack::new(&self.navigator)
                    .transition(Transition::new(Duration::from_millis(500)))
                    .size_full()
                    .item(|page, _w, _c| {
                        let offset = match (page.phase(), page.operation()) {
                            // Push: 新页面从右滑入
                            (PresencePhase::Entering, Some(NavOperation::Push)) => {
                                1.0 - page.progress()
                            }
                            // Pop: 旧页面向右滑出
                            (PresencePhase::Exiting, Some(NavOperation::Pop)) => page.progress(),

                            // Replace: 新页面从右滑入
                            (PresencePhase::Entering, Some(NavOperation::Replace)) => {
                                1.0 - page.progress()
                            }
                            // Replace: 旧页面向左滑出
                            (PresencePhase::Exiting, Some(NavOperation::Replace)) => {
                                -page.progress()
                            }

                            // 其他情况 (如稳定状态) 保持不动
                            _ => 0.0,
                        };
                        page.left(relative(offset)).into_any_element()
                    }),
            )
            .child(self.navigator_bar.clone())
    }
}
