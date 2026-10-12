//! Theme-owned smoked surfaces. Only containing panels blur; their content stays sharp.
use gpui::{prelude::*, *};
use gpui_component::ActiveTheme;

pub(crate) const BLUR_RADIUS: f32 = 18.;
pub(crate) const SURFACE_ALPHA: f32 = 0.85;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Effects {
    pub window_translucency: bool,
}

impl Default for Effects {
    fn default() -> Self {
        Self {
            window_translucency: true,
        }
    }
}

impl Global for Effects {}

impl Effects {
    fn panel_blur(self, requested: f32) -> f32 {
        if self.window_translucency {
            requested.max(0.)
        } else {
            0.
        }
    }
}

pub(crate) fn effects(cx: &App) -> Effects {
    cx.try_global::<Effects>().copied().unwrap_or_default()
}

pub(crate) fn native_background(effects: Effects) -> WindowBackgroundAppearance {
    if !effects.window_translucency || !cfg!(any(target_os = "windows", target_os = "macos")) {
        WindowBackgroundAppearance::Opaque
    } else {
        WindowBackgroundAppearance::Blurred
    }
}

/// Preserve imported theme contrast over either an extremely bright or dark desktop.
fn shell_alpha(background: Hsla, foreground: Hsla, muted: Hsla) -> f32 {
    for step in 0..=20 {
        let alpha = 0.8 + 0.2 * step as f32 / 20.;
        if [black(), white()].into_iter().all(|desktop| {
            let composite = desktop.blend(background.alpha(alpha));
            crate::theme::contrast(composite, foreground) >= 4.5
                && crate::theme::contrast(composite, muted) >= 3.
        }) {
            return alpha;
        }
    }
    1.
}

pub(crate) fn base(cx: &App) -> Hsla {
    let theme = cx.theme();
    let alpha = if effects(cx).window_translucency
        && cfg!(any(target_os = "windows", target_os = "macos"))
    {
        shell_alpha(theme.background, theme.foreground, theme.muted_foreground)
    } else {
        1.
    };
    theme.background.alpha(alpha)
}

pub(crate) fn sidebar(cx: &App) -> Hsla {
    // The root already carries the window tint: don't stack two 80% opaque fills.
    cx.theme()
        .sidebar
        .alpha(if base(cx).a < 1. { 0.3 } else { 1. })
}

#[derive(Clone, Copy)]
pub(crate) struct Palette {
    pub surface: Hsla,
    pub button: Hsla,
    pub input: Hsla,
    pub rim: Hsla,
    pub highlight: Hsla,
    pub foreground: Hsla,
    pub muted: Hsla,
    pub shadow: Hsla,
}

impl Palette {
    pub(crate) fn from_colors(
        background: Hsla,
        secondary: Hsla,
        foreground: Hsla,
        border: Hsla,
    ) -> Self {
        let dark = background.l < 0.5;
        let mut base = secondary.alpha(1.).blend(background.alpha(0.45));
        // A small saturation lift on the tint, not a hard-coded slate palette.
        base.s = (base.s * 1.08).min(1.);
        let mut button = base;
        button.l = (base.l + if dark { 0.055 } else { 0.04 }).min(1.);
        let mut input = base;
        input.l = (base.l - if dark { 0.055 } else { 0.045 }).max(0.);
        Self {
            surface: base.alpha(SURFACE_ALPHA),
            button: button.alpha(1.),
            input: input.alpha(1.),
            rim: border
                .alpha(1.)
                .blend(white().alpha(0.4))
                .alpha(if dark { 0.35 } else { 0.6 }),
            highlight: white().alpha(if dark { 0.09 } else { 0.32 }),
            foreground: foreground.alpha(1.),
            muted: foreground.alpha(1.).blend(base.alpha(0.18)),
            shadow: black().alpha(if dark { 0.2 } else { 0.11 }),
        }
    }

    pub(crate) fn of(cx: &App) -> Self {
        let theme = cx.theme();
        let mut palette = Self::from_colors(
            theme.background,
            theme.secondary,
            theme.foreground,
            theme.border,
        );
        if !effects(cx).window_translucency {
            palette.surface.a = 1.;
        }
        palette
    }

    pub(crate) fn shadows(self) -> Vec<BoxShadow> {
        vec![
            BoxShadow {
                color: self.shadow,
                offset: point(px(0.), px(4.)),
                blur_radius: px(14.),
                spread_radius: px(0.),
                inset: false,
            },
            // Inset rims do not steal space from the existing 36px toolbar geometry.
            BoxShadow {
                color: self.rim,
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(0.5),
                inset: true,
            },
            BoxShadow {
                color: self.highlight,
                offset: point(px(0.), px(1.)),
                blur_radius: px(0.),
                spread_radius: px(0.),
                inset: true,
            },
        ]
    }
}

pub(crate) fn surface<T: Styled>(element: T, radius: f32, cx: &App) -> T {
    let palette = Palette::of(cx);
    element
        .rounded(px(radius))
        .border_0()
        .bg(palette.surface)
        .text_color(palette.foreground)
        .shadow(palette.shadows())
}

/// Keep blur and sharp content in a single ordered layer. Delegating layout and
/// prepaint preserves the child's focus, hit testing, clipping and native controls.
pub(crate) fn frosted(radius: f32, blur_radius: f32, child: impl IntoElement) -> Frosted {
    Frosted {
        radius,
        blur_radius,
        child: child.into_any_element(),
    }
}

pub(crate) struct Frosted {
    radius: f32,
    blur_radius: f32,
    child: AnyElement,
}

impl Element for Frosted {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        window.paint_layer(bounds, |window| {
            let radius = effects(cx).panel_blur(self.blur_radius);
            if radius > 0. {
                window.paint_backdrop_blur(bounds, Corners::all(px(self.radius)), px(radius));
            }
            self.child.paint(window, cx);
        });
    }
}

impl IntoElement for Frosted {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn translucency_always_enables_blur_and_never_uses_a_sharp_native_backdrop() {
        for window_translucency in [false, true] {
            let effects = Effects {
                window_translucency,
            };
            assert_eq!(
                effects.panel_blur(18.),
                if window_translucency { 18. } else { 0. }
            );
            assert_eq!(effects.panel_blur(0.), 0.);
            let expected =
                if !window_translucency || !cfg!(any(target_os = "windows", target_os = "macos")) {
                    WindowBackgroundAppearance::Opaque
                } else {
                    WindowBackgroundAppearance::Blurred
                };
            assert_eq!(native_background(effects), expected);
        }
    }

    #[::core::prelude::v1::test]
    fn native_window_tint_keeps_shell_text_readable_in_both_appearances() {
        for (background, foreground, muted) in [
            (0x1b1e22, 0xf1f0ec, 0xa4a7aa),
            (0xf6f5f1, 0x17202b, 0x62676b),
        ] {
            let background = Hsla::from(rgb(background));
            let foreground = Hsla::from(rgb(foreground));
            let muted = Hsla::from(rgb(muted));
            let alpha = shell_alpha(background, foreground, muted);
            assert!((0.8..1.).contains(&alpha));
            for desktop in [black(), white()] {
                let composite = desktop.blend(background.alpha(alpha));
                assert!(crate::theme::contrast(composite, foreground) >= 4.5);
                assert!(crate::theme::contrast(composite, muted) >= 3.);
            }
        }
        assert_eq!(
            shell_alpha(
                hsla(0., 0., 0.5, 1.),
                hsla(0., 0., 0.5, 1.),
                hsla(0., 0., 0.5, 1.)
            ),
            1.
        );
    }

    #[::core::prelude::v1::test]
    fn smoke_preserves_theme_hue_and_opaque_text_in_both_modes() {
        for (background, secondary, foreground) in [
            (rgb(0x1b1e22), rgb(0x303438), rgb(0xf1f0ec)),
            (rgb(0xf6f5f1), rgb(0xdedfda), rgb(0x17202b)),
            (rgb(0x23132e), rgb(0x40294d), rgb(0xf9edff)),
        ] {
            let palette = Palette::from_colors(
                background.into(),
                secondary.into(),
                foreground.into(),
                secondary.into(),
            );
            assert_eq!(palette.surface.a, SURFACE_ALPHA);
            assert_eq!(palette.foreground.a, 1.);
            assert!(palette.input.l < palette.surface.l);
            assert!(palette.button.l > palette.surface.l);
            assert!((palette.surface.h - Hsla::from(secondary).h).abs() < 0.1);
            let shadows = palette.shadows();
            assert!(!shadows[0].inset);
            assert!(shadows[1].inset && shadows[2].inset);
        }
        assert_eq!(BLUR_RADIUS, 18.);
    }

    fn luminance(color: Hsla) -> f32 {
        let rgb = Rgba::from(color);
        let linear = |v: f32| {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(rgb.r) + 0.7152 * linear(rgb.g) + 0.0722 * linear(rgb.b)
    }

    #[::core::prelude::v1::test]
    fn builtin_glass_text_has_contrast_over_extreme_artwork() {
        for (background, secondary, foreground, border) in [
            (0x1b1e22, 0x303438, 0xf1f0ec, 0x393c40),
            (0xf6f5f1, 0xdedfda, 0x17202b, 0xaeb3b3),
        ] {
            let palette = Palette::from_colors(
                rgb(background).into(),
                rgb(secondary).into(),
                rgb(foreground).into(),
                rgb(border).into(),
            );
            for artwork in [black(), white()] {
                let surface = artwork.blend(palette.surface);
                for (text, fill) in [
                    (palette.foreground, surface),
                    (palette.muted, surface),
                    (palette.foreground, surface.blend(palette.input)),
                    (palette.foreground, surface.blend(palette.button)),
                ] {
                    let a = luminance(text);
                    let b = luminance(fill);
                    let contrast = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                    assert!(contrast >= 4.5, "glass text contrast {contrast}");
                    assert_eq!(text.a, 1.);
                }
            }
        }
    }
}
