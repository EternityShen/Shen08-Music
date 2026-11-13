//! Pure gpui-kit UI. No Android / platform code lives here — this crate is
//! the whole "what the user sees" surface and renders identically on any
//! backend the host hands it.

mod ui;

use gpui::{App, AppContext, WindowOptions};

use gpui_kit::component::{Root, Theme, ThemeMode};
use reqwest_client::ReqwestClient;
use std::sync::Arc;

// 构建一个用内置根证书、能在 Android 上跑 TLS 的客户端
fn make_http_client() -> Arc<dyn gpui::http_client::HttpClient> {
    let client = reqwest::Client::builder()
        .user_agent("shen08-music/0.1")
        .build()
        .expect("failed to build reqwest client");
    Arc::new(ReqwestClient::from(client)) // 包上 tokio runtime + HttpClient trait
}

/// Build the window contents. Called by the host once the native surface and
/// GPUI `App` exist.
pub fn launch(cx: &mut App) {
    log::info!("core::launch — initialising gpui-kit and opening the window");

    cx.set_http_client(make_http_client());

    // Boot every enabled gpui-kit layer (component => base + component).
    gpui_kit::init(cx);
    Theme::change(ThemeMode::Dark, None, cx);

    cx.open_window(WindowOptions::default(), |window, cx| {
        let counter = cx.new(|cx| ui::app::App::new(cx));
        // `Root` is mandatory as the first view — it owns dialogs, sheets,
        // notifications and the touch-selection overlay.
        cx.new(|cx| Root::new(counter, window, cx))
    })
    .expect("failed to open window");

    cx.activate(true);
}
