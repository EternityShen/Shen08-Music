use gpui_kit::*;

pub struct HomePage {}

impl HomePage {
    pub fn new() -> Self {
        Self {}
    }
}

impl Render for HomePage {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .justify_center()
            .items_center()
            .child("Home")
    }
}
