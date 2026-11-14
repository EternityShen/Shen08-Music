use crate::ui::components::navigator_bar::NavigatorBar;
use crate::ui::pages::home::HomePage;
use crate::ui::pages::music::MusicPage;
use gpui_kit::base::{NavMotion, NavOperation, NavStack, NavStackState, PresencePhase, Transition};
use gpui_kit::*;
use gpui_pre_reqwest_client::ReqwestClient;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn make_http_client() -> Arc<dyn http_client::HttpClient> {
    let client = reqwest::Client::builder()
        .user_agent("shen08-music/0.1")
        .build()
        .expect("构建http客户端失败");
    Arc::new(ReqwestClient::from(client))
}

pub struct App {
    navigator: Entity<NavStackState>,
    navigator_bar: Entity<NavigatorBar>,
    music_api_client: Arc<Mutex<kgmusic_api::client::Api>>,
}

impl App {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let nav_s = cx.new(|_cx| NavStackState::new());
        let home_page = cx.new(|_cx| HomePage::new());
        let music_page = cx.new(|cx| MusicPage::new(cx));
        nav_s.update(cx, |this, state| {
            this.push(home_page.clone(), NavMotion::Animated, state);
        });

        let config_dir = if let Some(c_d) = dirs::config_dir() {
            c_d.join("shen08-music/")
        } else {
            eprintln!("没有发现系统config目录");
            panic!()
        };

        let music_api_client =
            kgmusic_api::client::Api::new(kgmusic_api::session::Paths::new(config_dir))
                .expect("无法构建Music客户端");

        let navigator_bar = cx.new(|_cx| NavigatorBar::new(nav_s.clone(), music_page, home_page));

        Self {
            navigator: nav_s,
            navigator_bar,
            music_api_client: Arc::new(Mutex::new(music_api_client)),
        }
    }

    pub fn run() {
        application().run(|cx| {
            cx.set_http_client(make_http_client());
            init(cx);
            open_window(WindowOptions::default(), cx, |_x, cx| {
                cx.new(|cx| App::new(cx))
            })
            .expect("无法打开窗口");
        })
    }
}

impl Render for App {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .flex()
            .size_full()
            .bg(Rgba {
                r: 0.01,
                g: 0.01,
                b: 0.02,
                a: 1.0,
            })
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
