use gpui_kit::component::StyledExt;
use gpui_kit::component::label::Label;
use gpui_kit::*;

pub struct MusicInfo {
    title: String,
    artist: String,
    album: String,
    album_image: String,
}

impl MusicInfo {
    pub fn new() -> Self {
        Self {
            title: "ShenEternity".to_string(),
            artist: "Shen".to_string(),
            album: "Shen08".to_string(),
            album_image:
            "https://th.bing.com/th/id/R.bcdeb3e192b2b35ac74d511243c4dd06?rik=9siLv%2fapapq1Ng&riu=http%3a%2f%2fi0.hdslb.com%2fbfs%2farchive%2fb2e8fbc0782f917548bbbcb6786a3c9c4a3e64c5.jpg&ehk=KTbwyk%2ft6Omj0ZRLAvzOncGrRDLCQUReNg59ww0w%2bAI%3d&risl=&pid=ImgRaw&r=0"
                .to_string(),
        }
    }
}

impl Render for MusicInfo {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .flex_col()
            .gap(px(15.))
            .child(
                img(self.album_image.as_str())
                    .w(px(300.))
                    .h(px(300.))
                    .rounded(px(30.))
                    .object_fit(ObjectFit::Cover)
                    .with_loading(|| div().child("图片加载中...").into_any_element())
                    .with_fallback(|| div().child("图片加载失败").into_any_element()),
            )
            .child(
                Label::new(self.title.clone())
                    .text_size(px(35.))
                    .text_color(Rgba {
                        r: 0.7,
                        g: 0.3,
                        b: 0.5,
                        a: 1.0,
                    })
                    .font_bold(),
            )
            .child(
                Label::new(self.artist.clone())
                    .text_size(px(30.))
                    .text_color(Rgba {
                        r: 0.7,
                        g: 0.3,
                        b: 0.5,
                        a: 1.0,
                    })
                    .font_bold(),
            )
            .child(
                Label::new(self.album.clone())
                    .text_size(px(30.))
                    .text_color(Rgba {
                        r: 0.7,
                        g: 0.3,
                        b: 0.5,
                        a: 1.0,
                    })
                    .font_bold(),
            )
    }
}
