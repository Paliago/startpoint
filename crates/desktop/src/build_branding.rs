//! Shared by build.rs and runtime branding so PE resources and window icons agree.
pub const fn is_development(debug: bool, force_dev: bool, production: bool) -> bool {
    debug || force_dev || !production
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_optimized_builds_are_development_by_default() {
        assert!(is_development(false, false, false));
    }

    #[test]
    fn production_branding_requires_an_explicit_optimized_build() {
        assert!(!is_development(false, false, true));
        assert!(is_development(true, false, true));
    }

    #[test]
    fn explicit_dev_branding_wins_over_production() {
        for debug in [false, true] {
            for production in [false, true] {
                assert!(is_development(debug, true, production));
            }
        }
    }
}
