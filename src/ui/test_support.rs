//! Collects what widgets report about themselves, for layout assertions in tests.
use iced::advanced::widget::{Id, Operation, operation::Scrollable};

#[derive(Default)]
pub(super) struct Bounds {
    pub texts: Vec<(String, iced::Rectangle)>,
    pub scrolls: Vec<(Id, iced::Rectangle)>,
    pub containers: Vec<(Id, iced::Rectangle)>,
}
impl Operation for Bounds {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
    fn container(&mut self, id: Option<&Id>, bounds: iced::Rectangle) {
        if let Some(id) = id {
            self.containers.push((id.clone(), bounds));
        }
    }
    fn text(&mut self, _: Option<&Id>, bounds: iced::Rectangle, value: &str) {
        self.texts.push((value.into(), bounds));
    }
    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: iced::Rectangle,
        _: iced::Rectangle,
        _: iced::Vector,
        _: &mut dyn Scrollable,
    ) {
        if let Some(id) = id {
            self.scrolls.push((id.clone(), bounds));
        }
    }
}
impl Bounds {
    pub fn container(&self, id: &'static str) -> iced::Rectangle {
        self.containers
            .iter()
            .find(|(value, _)| *value == Id::new(id))
            .unwrap()
            .1
    }
    pub fn text(&self, value: &str) -> iced::Rectangle {
        self.texts.iter().find(|(text, _)| text == value).unwrap().1
    }
    pub fn scroll(&self, id: &'static str) -> iced::Rectangle {
        self.scrolls
            .iter()
            .find(|(value, _)| *value == Id::new(id))
            .unwrap()
            .1
    }
}
