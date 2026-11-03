//! Native title-bar geometry shared by every visual design.
//! The component TitleBar's default macOS inset is explicitly overridden by
//! LauncherTitleBar, so both shells must reserve our relocated traffic lights.
pub(crate) const HEADER_HEIGHT: f32 = 44.;
pub(crate) const TITLEBAR_PADDING: f32 = 8.;
const MACOS_TRAFFIC_LIGHT_CLEARANCE: f32 = 82.;

pub(crate) fn native_left_inset(is_macos: bool, fullscreen: bool) -> f32 {
    if is_macos && !fullscreen {
        MACOS_TRAFFIC_LIGHT_CLEARANCE
    } else {
        0.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_shells_reserve_macos_traffic_lights_and_reclaim_fullscreen_space() {
        let mac = native_left_inset(true, false);
        assert_eq!(mac, 82.);
        let first_app_control = TITLEBAR_PADDING + 2. + mac;
        assert!(first_app_control > MACOS_TRAFFIC_LIGHT_CLEARANCE);
        assert_eq!(native_left_inset(true, true), 0.);
        assert_eq!(native_left_inset(false, false), 0.);
        assert_eq!(native_left_inset(false, true), 0.);
    }
}
