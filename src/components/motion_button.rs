//! Shared button surface with directional light, cursor tracking, and spring press motion.

use gpui_kit::base::Disableable;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Icon;
use gpui_kit::component::button::{Button as KitButton, ButtonVariant, ButtonVariants};
use gpui_kit::{
    Animation, AnimationExt as _, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window, div, linear_color_stop, linear_gradient, px,
};
use std::rc::Rc;
use std::time::Duration;

#[derive(Default)]
struct VisualState {
    hovered: bool,
    cursor_x: f32,
    cursor_y: f32,
    anchor_x: Option<f32>,
    anchor_y: Option<f32>,
    presses: u64,
}

type ButtonClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    inner: KitButton,
    variant: ButtonVariant,
    is_outline: bool,
    disabled: bool,
    on_click: Option<ButtonClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();
        Self {
            inner: KitButton::new(id.clone()),
            id,
            variant: ButtonVariant::Default,
            is_outline: false,
            disabled: false,
            on_click: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.inner = self.inner.label(label);
        self
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.inner = self.inner.icon(icon);
        self
    }

    pub fn outline(mut self) -> Self {
        self.is_outline = true;
        self.inner = self.inner.outline();
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.inner = self.inner.tooltip(tooltip);
        self
    }

    pub fn loading(mut self, loading: bool) -> Self {
        self.inner = self.inner.loading(loading);
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl ButtonVariants for Button {
    fn with_variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self.inner = self.inner.with_variant(variant);
        self
    }
}

impl Disableable for Button {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self.inner = self.inner.disabled(disabled);
        self
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let visual = window.use_keyed_state(format!("button-visual-{:?}", self.id), cx, |_, _| {
            VisualState::default()
        });
        let state = visual.read(cx);
        let hovered = state.hovered && !self.disabled;
        let presses = state.presses;

        let is_ghost = self.variant == ButtonVariant::Ghost;
        let is_primary = self.variant == ButtonVariant::Primary;
        let is_danger = self.variant == ButtonVariant::Danger;

        let base_bg = if is_ghost {
            if hovered {
                cx.theme().secondary.opacity(0.4)
            } else {
                cx.theme().transparent
            }
        } else if is_primary {
            cx.theme().primary
        } else if is_danger {
            cx.theme().danger
        } else if self.is_outline {
            if hovered {
                cx.theme().secondary.opacity(0.5)
            } else {
                cx.theme().secondary.opacity(0.2)
            }
        } else {
            cx.theme().secondary
        };

        let top_edge = if is_ghost {
            cx.theme().transparent
        } else if is_primary {
            gpui_kit::white().opacity(if hovered { 0.42 } else { 0.26 })
        } else if is_danger {
            gpui_kit::white().opacity(if hovered { 0.38 } else { 0.22 })
        } else {
            gpui_kit::white().opacity(if hovered { 0.26 } else { 0.16 })
        };

        let bottom_edge = if is_ghost {
            cx.theme().transparent
        } else if is_primary {
            gpui_kit::black().opacity(if hovered { 0.65 } else { 0.45 })
        } else if is_danger {
            gpui_kit::black().opacity(0.60)
        } else {
            gpui_kit::black().opacity(if hovered { 0.70 } else { 0.50 })
        };

        let side_edge = if is_ghost {
            cx.theme().transparent
        } else if is_primary {
            cx.theme().primary.opacity(0.85)
        } else if is_danger {
            cx.theme().danger.opacity(0.85)
        } else {
            cx.theme().border.opacity(if hovered { 0.55 } else { 0.35 })
        };

        let rel_x = (state.cursor_x - state.anchor_x.unwrap_or(state.cursor_x)).clamp(-20.0, 360.0);
        let rel_y = (state.cursor_y - state.anchor_y.unwrap_or(state.cursor_y)).clamp(-15.0, 70.0);

        let spotlight_color = if is_primary {
            gpui_kit::white().opacity(0.32)
        } else if is_danger {
            gpui_kit::white().opacity(0.28)
        } else {
            cx.theme().primary.opacity(0.22)
        };

        let surface_id: SharedString = format!("btn-surf-{:?}", self.id).into();
        let click_handler = self.on_click.clone();

        // Inner div with bottom and side borders, background, cursor spotlight, and button contents
        let mut inner_surface = div()
            .relative()
            .w_full()
            .h_full()
            .rounded_md()
            .overflow_hidden()
            .border_b_1()
            .border_color(bottom_edge)
            .border_l_1()
            .border_r_1()
            .border_color(side_edge)
            .bg(base_bg);

        if hovered && !is_ghost {
            inner_surface = inner_surface.child(
                div()
                    .absolute()
                    .left(px(rel_x - 45.0))
                    .top(px(rel_y - 45.0))
                    .w(px(90.0))
                    .h(px(90.0))
                    .rounded_full()
                    .bg(linear_gradient(
                        135.,
                        linear_color_stop(spotlight_color, 0.),
                        linear_color_stop(cx.theme().transparent, 0.75),
                    )),
            );
        }

        inner_surface = inner_surface.child(
            self.inner
                .bg(cx.theme().transparent)
                .border_color(cx.theme().transparent),
        );

        // Outer div with thin light border on top corner edge
        let surface = div()
            .id(ElementId::Name(surface_id))
            .relative()
            .rounded_md()
            .border_t_1()
            .border_color(top_edge)
            .child(inner_surface)
            .on_hover({
                let visual = visual.clone();
                move |hovered, _, cx| {
                    visual.update(cx, |state, cx| {
                        state.hovered = *hovered;
                        if !*hovered {
                            state.anchor_x = None;
                            state.anchor_y = None;
                        }
                        cx.notify();
                    });
                }
            })
            .on_mouse_move({
                let visual = visual.clone();
                move |event, _, cx| {
                    visual.update(cx, |state, cx| {
                        let x = f32::from(event.position.x);
                        let y = f32::from(event.position.y);
                        if state.anchor_x.is_none() {
                            state.anchor_x = Some(x - 30.0);
                            state.anchor_y = Some(y - 18.0);
                        }
                        state.cursor_x = x;
                        state.cursor_y = y;
                        cx.notify();
                    });
                }
            })
            .on_click({
                let visual = visual.clone();
                move |event, window, cx| {
                    visual.update(cx, |state, cx| {
                        state.presses = state.presses.wrapping_add(1);
                        cx.notify();
                    });
                    if let Some(handler) = &click_handler {
                        handler(event, window, cx);
                    }
                }
            });

        if presses > 0 {
            surface
                .with_animation(
                    format!("button-spring-{:?}-{presses}", self.id),
                    Animation::new(Duration::from_millis(360)),
                    |this, progress| {
                        let y_offset = if progress < 0.15 {
                            (progress / 0.15) * 2.8
                        } else {
                            let t = (progress - 0.15) / 0.85;
                            let decay = (-5.5 * t).exp();
                            2.8 * decay * (t * std::f32::consts::PI * 3.2).cos()
                        };
                        this.top(px(y_offset))
                    },
                )
                .into_any_element()
        } else {
            surface.into_any_element()
        }
    }
}
