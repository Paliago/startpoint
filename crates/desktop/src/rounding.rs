//! Corner-radius roles shared by artwork, inset surfaces, controls and panels.
use gpui::{App, Pixels, px};
use gpui_component::ActiveTheme;

pub(crate) const CARD_INSET: f32 = 4.;
pub(crate) const CARD_CAPTION_SIDE_INSET: f32 = 2.;
/// Extra bottom breathing room beneath text, in addition to the artwork gutter.
pub(crate) const CARD_CAPTION_INSET: f32 = 8.;

#[derive(Clone, Copy)]
pub(crate) struct Rounding {
    pub artwork: Pixels,
    pub surface: Pixels,
    pub panel: Pixels,
    pub control: Pixels,
}

impl Rounding {
    pub fn of(cx: &App) -> Self {
        Self::new(
            cx.theme().radius,
            cx.theme().radius_lg,
            px(crate::components::control_appearance(cx).radius),
        )
    }

    fn new(surface: Pixels, panel: Pixels, control: Pixels) -> Self {
        Self {
            artwork: surface,
            surface,
            panel,
            control,
        }
    }

    /// The artwork is inset from its card, so the outer contour is larger.
    pub fn card(self) -> Pixels {
        outset(self.artwork, px(CARD_INSET))
    }

    pub fn choice_group(self) -> Pixels {
        outset(self.control, px(2.))
    }
}

fn outset(inner: Pixels, inset: Pixels) -> Pixels {
    if inner > px(0.) {
        inner + inset
    } else {
        px(0.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artwork_cards_and_choice_groups_have_matching_inset_contours() {
        let rounding = Rounding::new(px(8.), px(12.), px(6.));
        assert_eq!(rounding.artwork, px(8.));
        assert_eq!(rounding.surface, px(8.));
        assert_eq!(rounding.card(), px(12.));
        assert_eq!(rounding.choice_group(), px(8.));
        for radius in [4., 8., 12., 16., 48.] {
            let rounding = Rounding::new(px(radius), px(radius), px(radius));
            assert_eq!(rounding.card() - rounding.artwork, px(CARD_INSET));
            assert_eq!(rounding.choice_group() - rounding.control, px(2.));
        }
    }

    #[test]
    fn square_themes_stay_square_and_control_rounding_remains_independent() {
        let square = Rounding::new(px(0.), px(0.), px(0.));
        assert_eq!(square.card(), px(0.));
        assert_eq!(square.choice_group(), px(0.));
        let rounding = Rounding::new(px(0.), px(0.), px(12.));
        assert_eq!(rounding.artwork, px(0.));
        assert_eq!(rounding.surface, px(0.));
        assert_eq!(rounding.panel, px(0.));
        assert_eq!(rounding.control, px(12.));
        assert_eq!(rounding.card(), px(0.));
        assert_eq!(rounding.choice_group(), px(14.));
    }
}
