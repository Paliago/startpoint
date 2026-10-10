use startpoint_sdk::{AppearanceSettings, LibraryDensity, LibraryLayout};

pub(crate) const CONTENT_TOP: f32 = 64.;
pub(crate) const DETAIL_WIDTH: f32 = 380.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Geometry {
    pub columns: usize,
    pub card_width: f32,
    pub artwork_height: f32,
    pub row_height: f32,
    pub gap: f32,
    pub caption_height: f32,
    pub title_font_size: f32,
    pub metadata_font_size: f32,
    pub metadata_height: f32,
}

impl Geometry {
    /// Width is the library viewport, excluding any sidebar/detail panel.
    pub fn new(width: f32, settings: &AppearanceSettings, rem: f32) -> Self {
        let width = (width - 40.).max(1.);
        let size = match settings.library_density {
            LibraryDensity::Small => 0,
            LibraryDensity::Medium => 1,
            LibraryDensity::Large => 2,
        };
        let gap = [8., 12., 16.][size];
        let (minimum, maximum) = match settings.library_layout {
            LibraryLayout::Pills => ([280., 320., 360.][size], 3),
            LibraryLayout::Covers => ([116., 152., 192.][size], 8),
            LibraryLayout::Tiles => ([180., 220., 280.][size], 5),
            LibraryLayout::Compact => (width, 1),
        };
        let columns = (((width + gap) / (minimum + gap)).floor() as usize).clamp(1, maximum);
        let card_width = (width - (columns - 1) as f32 * gap) / columns as f32;
        let artwork_height = match settings.library_layout {
            LibraryLayout::Pills => [48., 64., 80.][size],
            LibraryLayout::Compact => [28., 36., 44.][size],
            LibraryLayout::Covers => (card_width - 8.).max(1.) * 1.5,
            LibraryLayout::Tiles => (card_width - 8.).max(1.) * 9. / 16.,
        };
        let title_font_size = rem * 0.875;
        let metadata_font_size = rem * 0.75;
        let metadata_height = (metadata_font_size * 1.2).ceil();
        let metadata_lines =
            usize::from(settings.library_last_played) + usize::from(settings.library_sources);
        let caption_height = title_font_size * 1.2 + metadata_lines as f32 * (metadata_height + 4.);
        let row_height = match settings.library_layout {
            LibraryLayout::Pills => artwork_height + 20.,
            LibraryLayout::Compact => {
                (artwork_height + 16.).max(title_font_size * 1.2 + metadata_height + 4. + 8.)
            }
            // Only 4px of card padding per edge and a 6px artwork/caption gap.
            // Spacing between cards belongs to the row gap, not an empty footer.
            LibraryLayout::Covers | LibraryLayout::Tiles => artwork_height + caption_height + 14.,
        } + gap;
        Self {
            columns,
            card_width,
            artwork_height,
            row_height,
            gap,
            caption_height,
            title_font_size,
            metadata_font_size,
            metadata_height,
        }
    }
}

pub(crate) fn filter_reveal(from: f32, visible: bool, progress: f32) -> f32 {
    let progress = progress.clamp(0., 1.);
    let eased = progress * progress * (3. - 2. * progress);
    from + (if visible { 1. } else { 0. } - from) * eased
}

pub(crate) fn resets_scroll(previous: &AppearanceSettings, next: &AppearanceSettings) -> bool {
    previous.library_layout != next.library_layout
        || previous.library_density != next.library_density
        || previous.library_sources != next.library_sources
        || previous.library_last_played != next.library_last_played
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_fit_narrow_and_wide_viewports_without_empty_columns() {
        for layout in [
            LibraryLayout::Pills,
            LibraryLayout::Covers,
            LibraryLayout::Tiles,
            LibraryLayout::Compact,
        ] {
            for density in [
                LibraryDensity::Small,
                LibraryDensity::Medium,
                LibraryDensity::Large,
            ] {
                let settings = AppearanceSettings {
                    library_layout: layout,
                    library_density: density,
                    ..Default::default()
                };
                for width in [220., 348., 560., 840., 1600., 3200.] {
                    let geometry = Geometry::new(width, &settings, 14.);
                    assert!(geometry.columns > 0);
                    assert!(geometry.card_width > 0.);
                    assert!(geometry.row_height > geometry.artwork_height);
                    let occupied = geometry.card_width * geometry.columns as f32
                        + geometry.gap * (geometry.columns - 1) as f32;
                    assert!((occupied - (width - 40.)).abs() < 0.01);
                    if layout == LibraryLayout::Compact {
                        assert_eq!(geometry.columns, 1);
                    }
                    if layout == LibraryLayout::Covers {
                        assert_eq!(
                            geometry.artwork_height,
                            (geometry.card_width - 8.).max(1.) * 1.5
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn older_preferences_get_cover_default_and_saved_layouts_are_respected() {
        let settings: AppearanceSettings =
            serde_json::from_str(r#"{"sidebar_width":216,"dither_artwork":false}"#).unwrap();
        assert_eq!(settings.library_layout, LibraryLayout::Covers);
        let explicit: AppearanceSettings =
            serde_json::from_str(r#"{"library_layout":"pills"}"#).unwrap();
        assert_eq!(explicit.library_layout, LibraryLayout::Pills);
        assert!(
            settings.library_details
                && settings.library_last_played
                && settings.library_favourite_badges
        );
        assert!(!settings.library_sources);
        assert_eq!(settings.sidebar_width, 216.);
        let changed = AppearanceSettings {
            library_layout: LibraryLayout::Covers,
            library_density: LibraryDensity::Large,
            library_details: false,
            library_sources: true,
            ..settings
        };
        let restored: AppearanceSettings =
            serde_json::from_str(&serde_json::to_string(&changed).unwrap()).unwrap();
        assert_eq!(changed, restored);
    }

    #[test]
    fn grid_card_height_contains_only_artwork_caption_and_actual_padding() {
        for rem in [12., 14., 16., 20.] {
            for last_played in [false, true] {
                for sources in [false, true] {
                    let settings = AppearanceSettings {
                        library_last_played: last_played,
                        library_sources: sources,
                        ..Default::default()
                    };
                    let geometry = Geometry::new(840., &settings, rem);
                    let footer = geometry.row_height
                        - geometry.gap
                        - geometry.artwork_height
                        - geometry.caption_height;
                    assert!(
                        (footer - 14.).abs() < 0.01,
                        "only 8px total padding plus 6px image/caption gap"
                    );
                }
            }
        }
    }

    #[test]
    fn favourites_reveal_animates_in_both_directions_and_can_reverse_midway() {
        assert_eq!(filter_reveal(0., true, 0.), 0.);
        assert_eq!(filter_reveal(0., true, 1.), 1.);
        assert_eq!(filter_reveal(1., false, 0.), 1.);
        assert_eq!(filter_reveal(1., false, 1.), 0.);
        let halfway = filter_reveal(0., true, 0.5);
        assert_eq!(filter_reveal(halfway, false, 0.), halfway);
        assert_eq!(filter_reveal(halfway, false, 1.), 0.);
        for progress in [0., 0.25, 0.5, 0.75, 1.] {
            assert!(
                (filter_reveal(0., true, progress) + filter_reveal(1., false, progress) - 1.).abs()
                    < 0.001
            );
        }
        for visible in [false, true] {
            let from = if visible { 0. } else { 1. };
            let samples = (0..=100)
                .map(|n| filter_reveal(from, visible, n as f32 / 100.))
                .collect::<Vec<_>>();
            assert!(samples.iter().all(|sample| (0. ..=1.).contains(sample)));
            assert!(samples.windows(2).all(|pair| if visible {
                pair[0] <= pair[1]
            } else {
                pair[0] >= pair[1]
            }));
        }
    }

    #[test]
    fn layout_changes_reset_the_scroll_but_theme_and_sidebar_changes_do_not() {
        let previous = AppearanceSettings::default();
        for layout in [
            LibraryLayout::Pills,
            LibraryLayout::Compact,
            LibraryLayout::Tiles,
        ] {
            assert!(resets_scroll(
                &previous,
                &AppearanceSettings {
                    library_layout: layout,
                    ..previous.clone()
                }
            ));
        }
        assert!(resets_scroll(
            &previous,
            &AppearanceSettings {
                library_density: LibraryDensity::Small,
                ..previous.clone()
            }
        ));
        assert!(!resets_scroll(
            &previous,
            &AppearanceSettings {
                sidebar_visible: false,
                ..previous.clone()
            }
        ));
        assert!(!resets_scroll(
            &previous,
            &AppearanceSettings {
                mode: startpoint_sdk::AppearanceMode::Dark,
                ..previous.clone()
            }
        ));
    }

    #[test]
    fn compact_rows_always_fit_title_and_extra_metadata_in_every_density() {
        for density in [
            LibraryDensity::Small,
            LibraryDensity::Medium,
            LibraryDensity::Large,
        ] {
            let settings = AppearanceSettings {
                library_layout: LibraryLayout::Compact,
                library_density: density,
                library_last_played: false,
                library_sources: false,
                ..Default::default()
            };
            let geometry = Geometry::new(600., &settings, 20.);
            assert!(
                geometry.row_height - geometry.gap
                    >= geometry.title_font_size * 1.2 + geometry.metadata_height + 12.
            );
        }
    }
}
