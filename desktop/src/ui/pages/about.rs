use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::label::Label;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kgmusic_api::{QrStatus, UserProfile};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct AboutPage {
    music_api: Arc<Mutex<kgmusic_api::MusicSource>>,
    qr_url: Option<Vec<u8>>,
    user_data: Option<UserProfile>,
    is_logged_in: bool,
}

impl AboutPage {
    pub fn new(music_api: Arc<Mutex<kgmusic_api::MusicSource>>, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            music_api,
            qr_url: None,
            user_data: None,
            is_logged_in: false,
        };

        this.check_log(cx);
        this
    }

    pub fn get_url(&mut self) {
        if let Ok(api) = self.music_api.lock() {
            self.qr_url = Some(api.create_qrcode().unwrap().png);
        } else {
            panic!()
        }
    }

    pub fn check_log(&mut self, cx: &mut Context<Self>) {
        let music_api = self.music_api.clone();
        cx.spawn(async move |this, cx| {
            loop {
                let (is_logged_in, qr_status_result, user_data) = {
                    if let Ok(api) = music_api.lock() {
                        let logged_in = api.is_logged_in();
                        if logged_in {
                            let user_data = api.user_profile();
                            (true, None, Some(user_data))
                        } else {
                            (false, Some(api.check_qrcode()), None)
                        }
                    } else {
                        (false, None, None)
                    }
                };

                if is_logged_in {
                    println!("登录成功");
                    let _ = this.update(cx, |this, cx| {
                        this.is_logged_in = true;
                        this.user_data = user_data;
                        cx.notify();
                    });
                    return;
                }

                if let Some(result) = qr_status_result {
                    match result {
                        Ok(qrc) => match qrc.status {
                            QrStatus::Expired => println!("QR 过期"),
                            QrStatus::Scanned => println!("等待确认"),
                            QrStatus::Success => println!("登录成功"),
                            QrStatus::Waiting => println!("等待中"),
                            QrStatus::Unknown(i) => println!("未知:{}", i),
                        },
                        Err(err) => println!("Error:{:?}", err),
                    }
                }

                cx.background_executor().timer(Duration::from_secs(5)).await;
            }
        })
        .detach();
    }
}

impl Render for AboutPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let image = Arc::new(Image::from_bytes(
            ImageFormat::Png,
            self.qr_url.clone().unwrap_or_default(),
        ));
        let source: ImageSource = image.into();
        div()
            .flex()
            .size_full()
            .justify_center()
            .items_center()
            .flex_col()
            .when_else(
                self.is_logged_in,
                |div| {
                    div.when_some(self.user_data.as_ref(), |div, data| {
                        div.flex()
                            .justify_center()
                            .items_center()
                            .gap(px(30.))
                            .flex_col()
                            .when_some(data.avatar_url.as_ref(), |div, url| {
                                div.child(
                                    img(url.clone())
                                        .size(px(400.))
                                        .rounded(px(50.))
                                        .object_fit(ObjectFit::Cover)
                                        .with_loading(|| {
                                            gpui_kit::div()
                                                .child("图片加载中...")
                                                .into_any_element()
                                        })
                                        .with_fallback(|| {
                                            gpui_kit::div().child("图片加载失败").into_any_element()
                                        }),
                                )
                            })
                            .child(
                                Label::new(data.nickname.clone())
                                    .text_size(px(40.))
                                    .text_color(Rgba {
                                        r: 1.0,
                                        g: 1.0,
                                        b: 1.0,
                                        a: 1.0,
                                    }),
                            )
                            .child(
                                Label::new(data.userid.clone())
                                    .text_size(px(20.))
                                    .text_color(Rgba {
                                        r: 1.0,
                                        g: 1.0,
                                        b: 1.0,
                                        a: 1.0,
                                    }),
                            )
                    })
                },
                |div| {
                    div.flex()
                        .flex_col()
                        .when_some(self.qr_url.as_ref(), |this, _data| {
                            this.child(
                                img(source)
                                    .w(px(200.))
                                    .h(px(200.))
                                    .object_fit(ObjectFit::Contain)
                                    .with_loading(|| {
                                        gpui_kit::div().child("图片加载中...").into_any_element()
                                    })
                                    .with_fallback(|| {
                                        gpui_kit::div().child("图片加载失败").into_any_element()
                                    }),
                            )
                        })
                        .child(
                            Button::new("log")
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
                                .h(px(50.))
                                .w(px(100.))
                                .label("登陆")
                                .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                                    this.get_url();
                                    this.check_log(cx);
                                    cx.notify();
                                })),
                        )
                },
            )
    }
}
