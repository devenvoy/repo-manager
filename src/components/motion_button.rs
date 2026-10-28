//! Shared button surface with directional light and press motion.

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
    pointer_x: f32,
    presses: u64,
}

type ButtonClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    inner: KitButton,
    variant: ButtonVariant,
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
        let pointer_x = state.pointer_x;
        let presses = state.presses;
        let base = if self.variant == ButtonVariant::Primary {
            cx.theme().primary
        } else if self.variant == ButtonVariant::Danger {
            cx.theme().danger
        } else {
            cx.theme().secondary
        };
        let light = if self.variant == ButtonVariant::Primary {
            cx.theme().button_primary_hover
        } else {
            cx.theme().border.opacity(0.85)
        };
        let top_edge = cx
            .theme()
            .foreground
            .opacity(if hovered { 0.32 } else { 0.18 });
        let bottom_edge = cx.theme().background.opacity(0.85);
        let angle = 60. + (pointer_x % 320.) / 320. * 120.;
        let surface_id: SharedString = format!("btn-surf-{:?}", self.id).into();
        let surface = div()
            .id(ElementId::Name(surface_id))
            .relative()
            .rounded_md()
            .border_t_1()
            .border_color(top_edge)
            .border_b_1()
            .border_color(bottom_edge)
            .border_l_1()
            .border_r_1()
            .border_color(cx.theme().border.opacity(if hovered { 0.5 } else { 0.25 }))
            .bg(if hovered {
                linear_gradient(
                    angle,
                    linear_color_stop(light, 0.),
                    linear_color_stop(base, 0.85),
                )
            } else {
                base.into()
            })
            .child(
                self.inner
                    .bg(cx.theme().transparent)
                    .border_color(cx.theme().transparent)
                    .on_click({
                        let visual = visual.clone();
                        move |event, window, cx| {
                            visual.update(cx, |state, cx| {
                                state.presses = state.presses.wrapping_add(1);
                                cx.notify();
                            });
                            if let Some(handler) = &self.on_click {
                                handler(event, window, cx);
                            }
                        }
                    }),
            );
        let surface = surface
            .on_hover({
                let visual = visual.clone();
                move |hovered, _, cx| {
                    visual.update(cx, |state, cx| {
                        state.hovered = *hovered;
                        cx.notify();
                    });
                }
            })
            .on_mouse_move({
                let visual = visual.clone();
                move |event, _, cx| {
                    visual.update(cx, |state, cx| {
                        state.pointer_x = f32::from(event.position.x);
                        cx.notify();
                    });
                }
            });
        if presses > 0 {
            surface
                .with_animation(
                    format!("button-spring-{:?}-{presses}", self.id),
                    Animation::new(Duration::from_millis(320)),
                    |this, progress| {
                        let y_offset = if progress < 0.15 {
                            (progress / 0.15) * 2.2
                        } else {
                            let t = (progress - 0.15) / 0.85;
                            let decay = (-6.0 * t).exp();
                            2.2 * decay * (t * std::f32::consts::PI * 3.5).cos()
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
