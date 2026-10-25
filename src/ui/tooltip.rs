//! Animated tooltip overlay in the spirit of shadcn/ui's Tooltip and HoverCard.
//!
//! Positioning and hover handling follow `iced::widget::tooltip`. On open the
//! overlay fades in and slides a few pixels towards its anchor. Iced renderers
//! have no opacity layer, so the fade paints a veil in the page background
//! colour over the overlay and lifts it as the animation progresses.
use super::Message;
use super::theme::tokens;
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
    renderer::{Quad, Renderer as _},
    widget::{self, Tree},
};
use iced::widget::container;
pub(super) use iced::widget::tooltip::Position;
use iced::{
    Element, Event, Length, Padding, Point, Rectangle, Renderer, Size, Theme, Vector, window,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const DURATION: Duration = Duration::from_millis(150);
/// Distance the overlay travels towards its anchor while fading in.
const SLIDE: f32 = 6.0;
/// Space between the anchor and the overlay.
const GAP: f32 = 4.0;

/// Desktop "reduce motion" preference (WCAG 2.3.3): tooltips appear without fade or slide.
static REDUCED_MOTION: AtomicBool = AtomicBool::new(false);

pub(super) fn set_reduced_motion(reduced: bool) {
    REDUCED_MOTION.store(reduced, Ordering::Relaxed);
}

pub(super) fn tooltip<'a>(
    content: impl Into<Element<'a, Message>>,
    tip: impl Into<Element<'a, Message>>,
    position: Position,
) -> Tooltip<'a> {
    Tooltip {
        content: content.into(),
        tip: tip.into(),
        position,
        padding: Padding::from([6, 10]),
        delay: Duration::ZERO,
        style: Box::new(super::components::popover),
    }
}

pub(super) struct Tooltip<'a> {
    content: Element<'a, Message>,
    tip: Element<'a, Message>,
    position: Position,
    padding: Padding,
    delay: Duration,
    style: Box<dyn Fn(&Theme) -> container::Style + 'a>,
}

impl<'a> Tooltip<'a> {
    pub(super) fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }
    pub(super) fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
    pub(super) fn style(mut self, style: impl Fn(&Theme) -> container::Style + 'a) -> Self {
        self.style = Box::new(style);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Idle,
    Hovered { at: Instant },
    Open { cursor: Point, at: Instant },
}

struct State {
    phase: Phase,
    /// Time of the latest frame, so drawing is deterministic for a given frame.
    now: Instant,
}

impl State {
    fn progress(&self) -> f32 {
        let Phase::Open { at, .. } = self.phase else {
            return 0.0;
        };
        if REDUCED_MOTION.load(Ordering::Relaxed) {
            return 1.0;
        }
        let t = (self.now.saturating_duration_since(at).as_secs_f32() / DURATION.as_secs_f32())
            .min(1.0);
        1.0 - (1.0 - t).powi(3) // ease-out cubic
    }
}

impl Widget<Message, Theme, Renderer> for Tooltip<'_> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content), Tree::new(&self.tip)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[self.content.as_widget(), self.tip.as_widget()]);
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State {
            phase: Phase::Idle,
            now: Instant::now(),
        })
    }
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let now = match event {
            Event::Window(window::Event::RedrawRequested(at)) => Some(*at),
            Event::Mouse(_) => Some(Instant::now()),
            _ => None,
        };
        if let Some(now) = now {
            let state = tree.state.downcast_mut::<State>();
            state.now = now;
            let over = cursor.position_over(layout.bounds());
            match (state.phase, over) {
                (Phase::Idle, Some(cursor)) if self.delay.is_zero() => {
                    state.phase = Phase::Open { cursor, at: now };
                    shell.invalidate_layout();
                    shell.request_redraw();
                }
                (Phase::Idle, Some(_)) => {
                    state.phase = Phase::Hovered { at: now };
                    shell.request_redraw_at(now + self.delay);
                }
                (Phase::Hovered { .. }, None) => state.phase = Phase::Idle,
                (Phase::Hovered { at }, Some(_)) if now < at + self.delay => {
                    shell.request_redraw_at(at + self.delay);
                }
                (Phase::Hovered { .. }, Some(cursor)) => {
                    state.phase = Phase::Open { cursor, at: now };
                    shell.invalidate_layout();
                    shell.request_redraw();
                }
                (Phase::Open { cursor: last, at }, Some(cursor))
                    if self.position == Position::FollowCursor && last != cursor =>
                {
                    state.phase = Phase::Open { cursor, at };
                    shell.request_redraw();
                }
                (Phase::Open { .. }, None) => {
                    state.phase = Phase::Idle;
                    shell.invalidate_layout();
                    shell.request_redraw();
                }
                (Phase::Open { .. }, Some(_)) | (Phase::Idle, None) => {}
            }
            if state.progress() < 1.0 && matches!(state.phase, Phase::Open { .. }) {
                shell.request_redraw();
            }
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let state = tree.state.downcast_ref::<State>();
        let progress = state.progress();
        let phase = state.phase;
        let mut children = tree.children.iter_mut();
        let content = self.content.as_widget_mut().overlay(
            children.next().unwrap(),
            layout,
            renderer,
            viewport,
            translation,
        );
        let tip = match phase {
            Phase::Open { cursor, .. } => Some(overlay::Element::new(Box::new(Overlay {
                position: layout.position() + translation,
                tip: &mut self.tip,
                tree: children.next().unwrap(),
                cursor,
                anchor: layout.bounds(),
                positioning: self.position,
                padding: self.padding,
                style: &*self.style,
                progress,
            }))),
            _ => None,
        };
        (content.is_some() || tip.is_some()).then(|| {
            overlay::Group::with_children(content.into_iter().chain(tip).collect()).overlay()
        })
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                operation,
            );
        });
    }
}

impl<'a> From<Tooltip<'a>> for Element<'a, Message> {
    fn from(tooltip: Tooltip<'a>) -> Self {
        Element::new(tooltip)
    }
}

struct Overlay<'a, 'b> {
    position: Point,
    tip: &'b mut Element<'a, Message>,
    tree: &'b mut Tree,
    cursor: Point,
    anchor: Rectangle,
    positioning: Position,
    padding: Padding,
    style: &'b (dyn Fn(&Theme) -> container::Style + 'a),
    progress: f32,
}

impl overlay::Overlay<Message, Theme, Renderer> for Overlay<'_, '_> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let viewport = Rectangle::with_size(bounds);
        let tip = self.tip.as_widget_mut().layout(
            self.tree,
            renderer,
            &layout::Limits::new(Size::ZERO, viewport.size()).shrink(self.padding),
        );
        let inner = tip.bounds().size();
        let size = inner.expand(self.padding);
        let center_x = self.position.x + (self.anchor.width - size.width) / 2.0;
        let center_y = self.position.y + (self.anchor.height - size.height) / 2.0;
        let origin = match self.positioning {
            Position::Top => Point::new(center_x, self.position.y - size.height - GAP),
            Position::Bottom => Point::new(center_x, self.position.y + self.anchor.height + GAP),
            Position::Left => Point::new(self.position.x - size.width - GAP, center_y),
            Position::Right => Point::new(self.position.x + self.anchor.width + GAP, center_y),
            Position::FollowCursor => {
                let translation = self.position - self.anchor.position();
                Point::new(
                    self.cursor.x - self.padding.left,
                    self.cursor.y - size.height + self.padding.bottom,
                ) + translation
            }
        };
        // Keep the overlay inside the window.
        let x = origin.x.clamp(0.0, (viewport.width - size.width).max(0.0));
        let y = origin
            .y
            .clamp(0.0, (viewport.height - size.height).max(0.0));
        layout::Node::with_children(
            size,
            vec![tip.translate(Vector::new(self.padding.left, self.padding.top))],
        )
        .translate(Vector::new(x, y))
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        inherited: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let p = self.progress;
        let mut style = (self.style)(theme);
        style.shadow.color = style.shadow.color.scale_alpha(p);
        let bounds = layout.bounds();
        let travel = (1.0 - p) * SLIDE;
        let offset = match self.positioning {
            Position::Top | Position::FollowCursor => Vector::new(0.0, travel),
            Position::Bottom => Vector::new(0.0, -travel),
            Position::Left => Vector::new(travel, 0.0),
            Position::Right => Vector::new(-travel, 0.0),
        };
        // Its own layer: overlays share one, where every quad is painted before
        // any text, so an open select menu's labels would show through the tip.
        let shadow = style.shadow;
        let reach = SLIDE + shadow.blur_radius + shadow.offset.x.abs().max(shadow.offset.y.abs());
        renderer.with_layer(bounds.expand(reach), |renderer| {
            renderer.with_translation(offset, |renderer| {
                container::draw_background(renderer, &style, bounds);
                self.tip.as_widget().draw(
                    self.tree,
                    renderer,
                    theme,
                    &renderer::Style {
                        text_color: style.text_color.unwrap_or(inherited.text_color),
                    },
                    layout.children().next().unwrap(),
                    cursor,
                    &Rectangle::with_size(Size::INFINITE),
                );
                if p < 1.0 {
                    // A separate layer keeps the veil above text and images.
                    renderer.with_layer(bounds, |renderer| {
                        renderer.fill_quad(
                            Quad {
                                bounds,
                                border: iced::Border {
                                    radius: style.border.radius,
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                            tokens(theme).background.scale_alpha(1.0 - p),
                        );
                    });
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::clipboard;
    use iced::widget::{space, text};

    #[test]
    fn opens_with_fade_and_keeps_redrawing_until_settled() {
        crate::ui::components::load_fonts();
        let renderer = Renderer::new(iced::Font::default(), iced::Pixels(16.0));
        let mut element: Element<'_, Message> =
            tooltip(space().width(40).height(40), text("hint"), Position::Top).into();
        let mut tree = Tree::new(element.as_widget());
        let viewport = Rectangle::with_size(Size::new(200.0, 200.0));
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, viewport.size()),
        );
        let cursor = mouse::Cursor::Available(Point::new(20.0, 20.0));
        let start = Instant::now();
        let mut frame = |at: Instant| {
            let mut messages = vec![];
            let mut shell = Shell::new(&mut messages);
            element.as_widget_mut().update(
                &mut tree,
                &Event::Window(window::Event::RedrawRequested(at)),
                Layout::new(&node),
                cursor,
                &renderer,
                &mut clipboard::Null,
                &mut shell,
                &viewport,
            );
            let redraw = shell.redraw_request() == window::RedrawRequest::NextFrame;
            (tree.state.downcast_ref::<State>().progress(), redraw)
        };
        assert_eq!(frame(start), (0.0, true));
        let (halfway, redraw) = frame(start + DURATION / 2);
        assert!(halfway > 0.5 && halfway < 1.0 && redraw, "{halfway}");
        assert_eq!(frame(start + DURATION), (1.0, false));
        set_reduced_motion(true);
        assert_eq!(frame(start), (1.0, false));
        set_reduced_motion(false);
    }
}
