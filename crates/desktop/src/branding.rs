//! Visual identity only: development builds keep the existing library paths.
pub(crate) const IS_DEV: bool = cfg!(any(debug_assertions, feature = "dev-icon"));
pub(crate) const NAME: &str = if IS_DEV {
    "Startpoint Dev"
} else {
    "Startpoint"
};
pub(crate) const APP_ID: &str = if IS_DEV {
    "startpoint.dev"
} else {
    "startpoint"
};

pub(crate) fn window_title(example_library: bool) -> &'static str {
    match (IS_DEV, example_library) {
        (true, true) => "Startpoint Dev — Example library",
        (false, true) => "Startpoint — Example library",
        (_, false) => NAME,
    }
}
