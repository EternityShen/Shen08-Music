use gpui_kit::component::label::Label;
use gpui_kit::*;
use kgmusic_api::model::SearchResult;
pub struct MusicItemCard {
    song_data: SearchResult,
}

impl MusicItemCard {
    pub fn new(search_result: SearchResult) -> Self {
        Self {
            song_data: search_result,
        }
    }
}

impl Render for MusicItemCard {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let image_url = if let Some(url) = &self.song_data.cover_url {
            url
        } else {
            &"".to_string()
        };

        div()
            .flex()
            .h(px(60.))
            .w_full()
            .flex_row()
            .justify_between()
            .items_center()
            .bg(Rgba {
                r: 0.1,
                g: 0.1,
                b: 0.1,
                a: 1.0,
            })
            .child(
                div()
                    .flex()
                    .h_full()
                    .flex_row()
                    .gap(px(10.))
                    .child(
                        img(image_url.clone())
                            .object_fit(ObjectFit::Cover)
                            .rounded(px(10.))
                            .w(px(50.))
                            .h(px(50.)),
                    )
                    .child(
                        div()
                            .flex()
                            .h_full()
                            .flex_col()
                            .gap(px(10.))
                            .child(
                                Label::new(self.song_data.name.clone())
                                    .text_color(Rgba {
                                        r: 1.0,
                                        g: 1.0,
                                        b: 1.0,
                                        a: 1.0,
                                    })
                                    .text_size(px(20.)),
                            )
                            .child(Label::new(self.song_data.artist.clone()).text_color(Rgba {
                                r: 1.0,
                                g: 1.0,
                                b: 1.0,
                                a: 1.0,
                            }))
                            .text_size(px(15.)),
                    ),
            )
    }
}
