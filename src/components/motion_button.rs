//! Shared button surface with directional light, cursor tracking, and spring press motion.

use gpui_kit::base::{Disableable, StyledExt as _};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Icon;
use gpui_kit::component::button::{ButtonVariant, ButtonVariants};
use gpui_kit::{
    Animation, AnimationExt as _, App, BoxShadow, ClickEvent, ElementId, InteractiveElement as _,
    IntoElement, ParentElement as _, RenderOnce, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, linear_color_stop, linear_gradient, point, px,
};
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Default)]
struct VisualState {
    hovered: bool,
    cursor_x: f32,
    cursor_y: f32,
    anchor_x: Option<f32>,
    anchor_y: Option<f32>,
    presses: u64,
    last_click: Option<Instant>,
}

type ButtonClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<Icon>,
    tooltip: Option<SharedString>,
    loading: bool,
    variant: ButtonVariant,
    is_outline: bool,
    disabled: bool,
    debounce: Duration,
    on_click: Option<ButtonClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: None,
            icon: None,
            tooltip: None,
            loading: false,
            variant: ButtonVariant::Default,
            is_outline: false,
            disabled: false,
            debounce: Duration::from_millis(300),
            on_click: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn outline(mut self) -> Self {
        self.is_outline = true;
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn debounce(mut self, duration: Duration) -> Self {
        self.debounce = duration;
        self
    }

    pub fn debounce_ms(mut self, millis: u64) -> Self {
        self.debounce = Duration::from_millis(millis);
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
        self
    }
}

impl Disableable for Button {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
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

        let is_dark = cx.theme().background.l < 0.5;
        let is_ghost = self.variant == ButtonVariant::Ghost;
        let is_primary = self.variant == ButtonVariant::Primary;
        let is_danger = self.variant == ButtonVariant::Danger;

        let text_color: gpui_kit::Hsla = if is_primary || is_danger {
            gpui_kit::white()
        } else if is_ghost || self.is_outline {
            cx.theme().foreground
        } else if is_dark {
            gpui_kit::rgb(0xF8FAFC).into()
        } else {
            gpui_kit::rgb(0x0F172A).into()
        };

        let (top_bg, bottom_bg, border_color, shadows, spotlight_color) = if is_ghost {
            let bg = if hovered {
                cx.theme().secondary.opacity(0.25)
            } else {
                cx.theme().transparent
            };
            (
                bg,
                bg,
                cx.theme().transparent,
                vec![],
                cx.theme().transparent,
            )
        } else if is_primary {
            let (t, b) = if hovered {
                (gpui_kit::rgb(0x4087F8), gpui_kit::rgb(0x2154DD))
            } else {
                (gpui_kit::rgb(0x3B82F6), gpui_kit::rgb(0x1D4ED8))
            };
            let border = gpui_kit::rgb(0x1D4ED8).opacity(0.85);
            let s = vec![
                BoxShadow {
                    color: gpui_kit::rgb(0x2563EB)
                        .opacity(if hovered { 0.46 } else { 0.38 })
                        .into(),
                    offset: point(px(0.), px(4.)),
                    blur_radius: px(14.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: gpui_kit::black().opacity(0.20),
                    offset: point(px(0.), px(1.5)),
                    blur_radius: px(3.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: gpui_kit::white().opacity(if hovered { 0.52 } else { 0.45 }),
                    offset: point(px(0.), px(1.2)),
                    blur_radius: px(0.6),
                    spread_radius: px(0.),
                    inset: true,
                },
                BoxShadow {
                    color: gpui_kit::black().opacity(0.35),
                    offset: point(px(0.), px(-1.2)),
                    blur_radius: px(0.6),
                    spread_radius: px(0.),
                    inset: true,
                },
            ];
            (
                t.into(),
                b.into(),
                border.into(),
                s,
                gpui_kit::white().opacity(0.10),
            )
        } else if is_danger {
            let (t, b) = if hovered {
                (gpui_kit::rgb(0xF24E4E), gpui_kit::rgb(0xBD2020))
            } else {
                (gpui_kit::rgb(0xEF4444), gpui_kit::rgb(0xB91C1C))
            };
            let border = gpui_kit::rgb(0xB91C1C).opacity(0.85);
            let s = vec![
                BoxShadow {
                    color: gpui_kit::rgb(0xEF4444)
                        .opacity(if hovered { 0.45 } else { 0.38 })
                        .into(),
                    offset: point(px(0.), px(4.)),
                    blur_radius: px(14.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: gpui_kit::black().opacity(0.20),
                    offset: point(px(0.), px(1.5)),
                    blur_radius: px(3.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: gpui_kit::white().opacity(if hovered { 0.50 } else { 0.45 }),
                    offset: point(px(0.), px(1.2)),
                    blur_radius: px(0.6),
                    spread_radius: px(0.),
                    inset: true,
                },
                BoxShadow {
                    color: gpui_kit::black().opacity(0.35),
                    offset: point(px(0.), px(-1.2)),
                    blur_radius: px(0.6),
                    spread_radius: px(0.),
                    inset: true,
                },
            ];
            (
                t.into(),
                b.into(),
                border.into(),
                s,
                gpui_kit::white().opacity(0.10),
            )
        } else if self.is_outline {
            let bg = cx
                .theme()
                .secondary
                .opacity(if hovered { 0.22 } else { 0.12 });
            let border = cx.theme().border;
            let s = vec![BoxShadow {
                color: gpui_kit::black().opacity(if is_dark { 0.20 } else { 0.05 }),
                offset: point(px(0.), px(1.)),
                blur_radius: px(2.),
                spread_radius: px(0.),
                inset: false,
            }];
            (
                bg,
                bg,
                border,
                s,
                gpui_kit::white().opacity(if is_dark { 0.08 } else { 0.15 }),
            )
        } else if is_dark {
            let (t, b) = if hovered {
                (gpui_kit::rgb(0x2C324A), gpui_kit::rgb(0x1A1D2D))
            } else {
                (gpui_kit::rgb(0x282D42), gpui_kit::rgb(0x181A28))
            };
            let border = gpui_kit::white().opacity(if hovered { 0.16 } else { 0.12 });
            let s = vec![
                BoxShadow {
                    color: gpui_kit::black().opacity(if hovered { 0.48 } else { 0.42 }),
                    offset: point(px(0.), px(4.)),
                    blur_radius: px(10.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: gpui_kit::black().opacity(0.30),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(2.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: gpui_kit::white().opacity(if hovered { 0.22 } else { 0.18 }),
                    offset: point(px(0.), px(1.0)),
                    blur_radius: px(0.6),
                    spread_radius: px(0.),
                    inset: true,
                },
                BoxShadow {
                    color: gpui_kit::black().opacity(0.40),
                    offset: point(px(0.), px(-1.0)),
                    blur_radius: px(0.6),
                    spread_radius: px(0.),
                    inset: true,
                },
            ];
            (
                t.into(),
                b.into(),
                border,
                s,
                gpui_kit::white().opacity(0.08),
            )
        } else {
            let (t, b) = if hovered {
                (gpui_kit::rgb(0xFFFFFF), gpui_kit::rgb(0xEDF0F5))
            } else {
                (gpui_kit::rgb(0xFFFFFF), gpui_kit::rgb(0xF2F4F7))
            };
            let border = gpui_kit::black().opacity(0.12);
            let s = vec![
                BoxShadow {
                    color: gpui_kit::black().opacity(if hovered { 0.08 } else { 0.06 }),
                    offset: point(px(0.), px(3.)),
                    blur_radius: px(8.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: gpui_kit::black().opacity(0.04),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(2.),
                    spread_radius: px(0.),
                    inset: false,
                },
                BoxShadow {
                    color: gpui_kit::white().opacity(0.95),
                    offset: point(px(0.), px(1.0)),
                    blur_radius: px(0.5),
                    spread_radius: px(0.),
                    inset: true,
                },
                BoxShadow {
                    color: gpui_kit::black().opacity(0.05),
                    offset: point(px(0.), px(-1.0)),
                    blur_radius: px(0.5),
                    spread_radius: px(0.),
                    inset: true,
                },
            ];
            (
                t.into(),
                b.into(),
                border,
                s,
                gpui_kit::white().opacity(0.14),
            )
        };

        let rel_x = (state.cursor_x - state.anchor_x.unwrap_or(state.cursor_x)).clamp(-20.0, 360.0);
        let rel_y = (state.cursor_y - state.anchor_y.unwrap_or(state.cursor_y)).clamp(-15.0, 70.0);

        let surface_id: SharedString = format!("btn-surf-{:?}", self.id).into();
        let click_handler = self.on_click.clone();
        let debounce = self.debounce;

        let mut surface = div()
            .id(ElementId::Name(surface_id))
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .min_h(px(38.))
            .min_w(px(38.))
            .rounded(px(14.))
            .overflow_hidden()
            .border_1()
            .border_color(border_color)
            .shadow(shadows);

        if self.disabled {
            surface = surface.opacity(0.45).cursor_not_allowed();
        } else {
            surface = surface.cursor_pointer();
        }

        if is_ghost || self.is_outline {
            surface = surface.bg(top_bg);
        } else {
            surface = surface.bg(linear_gradient(
                180.,
                linear_color_stop(top_bg, 0.),
                linear_color_stop(bottom_bg, 1.),
            ));
        }

        // Very dim, soft lighting gleam tracking the cursor
        if hovered && !is_ghost {
            surface = surface.child(
                div()
                    .absolute()
                    .left(px(rel_x - 70.0))
                    .top(px(rel_y - 70.0))
                    .w(px(140.0))
                    .h(px(140.0))
                    .rounded_full()
                    .bg(linear_gradient(
                        135.,
                        linear_color_stop(spotlight_color, 0.),
                        linear_color_stop(cx.theme().transparent, 0.40),
                    )),
            );
        }

        // Clean typography and icon layout with no external background overrides
        let has_label = self.label.is_some();
        let mut content = div()
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .justify_center()
            .gap_2()
            .py_2()
            .text_color(text_color)
            .font_semibold()
            .text_size(px(14.));

        if has_label {
            content = content.px_4();
        } else {
            content = content.px_2p5();
        }

        if let Some(icon) = self.icon {
            content = content.child(icon);
        }

        if let Some(label) = self.label {
            content = content.child(label);
        }

        surface = surface.child(content);

        // Optional floating tooltip on hover
        if hovered {
            if let Some(tooltip_text) = &self.tooltip {
                surface = surface.child(
                    div()
                        .absolute()
                        .bottom(px(42.))
                        .px_2p5()
                        .py_1()
                        .rounded_md()
                        .bg(if is_dark {
                            gpui_kit::rgb(0x1E293B)
                        } else {
                            gpui_kit::rgb(0x0F172A)
                        })
                        .text_color(gpui_kit::white())
                        .text_size(px(11.))
                        .shadow(vec![BoxShadow {
                            color: gpui_kit::black().opacity(0.25),
                            offset: point(px(0.), px(2.)),
                            blur_radius: px(4.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                        .child(tooltip_text.clone()),
                );
            }
        }

        surface = surface
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
                let disabled = self.disabled;
                move |event, window, cx| {
                    if disabled {
                        return;
                    }

                    let now = Instant::now();
                    let should_fire = visual.update(cx, |state, cx| {
                        if !debounce.is_zero() {
                            if let Some(last) = state.last_click {
                                if now.duration_since(last) < debounce {
                                    return false;
                                }
                            }
                        }
                        state.last_click = Some(now);
                        state.presses = state.presses.wrapping_add(1);
                        cx.notify();
                        true
                    });

                    if !should_fire {
                        return;
                    }

                    cx.stop_propagation();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_button_debounce_default_and_custom() {
        let btn = Button::new("test-btn");
        assert_eq!(btn.debounce, Duration::from_millis(300));

        let btn_custom = btn.debounce(Duration::from_millis(150));
        assert_eq!(btn_custom.debounce, Duration::from_millis(150));

        let btn_ms = Button::new("test-ms").debounce_ms(500);
        assert_eq!(btn_ms.debounce, Duration::from_millis(500));
    }

    #[test]
    fn test_debounce_click_filter() {
        let mut state = VisualState::default();
        let debounce = Duration::from_millis(300);

        // First click succeeds
        let t0 = Instant::now();
        state.last_click = Some(t0);
        state.presses += 1;

        // Immediate duplicate click (10ms later) should be dropped
        let t1 = t0 + Duration::from_millis(10);
        let should_fire = if let Some(last) = state.last_click {
            t1.duration_since(last) >= debounce
        } else {
            true
        };
        assert!(!should_fire, "Immediate duplicate click must be debounced");

        // Click after 350ms should succeed
        let t2 = t0 + Duration::from_millis(350);
        let should_fire = if let Some(last) = state.last_click {
            t2.duration_since(last) >= debounce
        } else {
            true
        };
        assert!(should_fire, "Click after debounce window must be allowed");
    }
}
