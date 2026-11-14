use gpui_kit::*;

pub struct HomePage {}

impl HomePage {
    pub fn new() -> Self {
        Self {}
    }
}

impl Render for HomePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .justify_center()
            .items_center()
            .child("Home")
    }
}
