use image::RgbaImage;

fn render_source(svg: &[u8], size: u32) -> RgbaImage {
    let tree =
        resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default()).expect("logo SVG");
    let mut image = resvg::tiny_skia::Pixmap::new(size, size).unwrap();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(
            size as f32 / tree.size().width(),
            size as f32 / tree.size().height(),
        ),
        &mut image.as_mut(),
    );
    image::load_from_memory(&image.encode_png().unwrap())
        .unwrap()
        .to_rgba8()
}

#[test]
fn launcher_icon_frames_match_the_svg_and_cover_windows_sizes() {
    check_exports(
        include_bytes!("../assets/icons/startpoint.svg"),
        include_bytes!("../assets/icons/startpoint.ico"),
        include_bytes!("../assets/icons/startpoint.png"),
    );
}

#[test]
fn dev_icon_frames_match_the_svg_and_cover_windows_sizes() {
    check_exports(
        include_bytes!("../assets/icons/startpoint-dev.svg"),
        include_bytes!("../assets/icons/startpoint-dev.ico"),
        include_bytes!("../assets/icons/startpoint-dev.png"),
    );
}

fn check_exports(svg: &[u8], ico: &[u8], png: &[u8]) {
    assert_eq!(&ico[..4], &[0, 0, 1, 0]);
    let sizes = [16u32, 20, 24, 32, 40, 48, 64, 128, 256];
    assert_eq!(
        u16::from_le_bytes(ico[4..6].try_into().unwrap()) as usize,
        sizes.len()
    );
    let mut next_offset = 6 + sizes.len() * 16;
    for (index, size) in sizes.into_iter().enumerate() {
        let entry = &ico[6 + index * 16..6 + (index + 1) * 16];
        let dimension = if size == 256 { 0 } else { size as u8 };
        assert_eq!(&entry[..4], &[dimension, dimension, 0, 0]);
        assert_eq!(u16::from_le_bytes(entry[4..6].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(entry[6..8].try_into().unwrap()), 32);
        let length = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
        let offset = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;
        assert_eq!(offset, next_offset);
        let frame = image::load_from_memory(&ico[offset..offset + length])
            .expect("ICO PNG frame")
            .to_rgba8();
        assert_eq!(frame.dimensions(), (size, size));
        assert_eq!(frame, render_source(svg, size));
        assert_eq!(frame.get_pixel(0, 0)[3], 0, "transparent outer corners");
        assert_eq!(
            frame.get_pixel(size / 2, size / 2).0,
            [246, 245, 241, 255],
            "ivory play mark"
        );
        next_offset += length;
    }
    assert_eq!(next_offset, ico.len());
    let png = image::load_from_memory(png)
        .expect("standalone logo PNG")
        .to_rgba8();
    assert_eq!(png, render_source(svg, 512));
}

#[test]
fn dev_icon_is_only_a_recolor_of_the_regular_design() {
    let regular = include_str!("../assets/icons/startpoint.svg");
    let dev = include_str!("../assets/icons/startpoint-dev.svg");
    assert_eq!(
        regular
            .replace("Startpoint", "Startpoint Dev")
            .replace("#17665e", "#7950c7"),
        dev
    );
    for size in [16, 32, 48, 256] {
        let regular = render_source(regular.as_bytes(), size);
        let dev = render_source(dev.as_bytes(), size);
        assert!(regular != dev);
        for (regular, dev) in regular.pixels().zip(dev.pixels()) {
            assert_eq!(regular[3], dev[3], "identical transparency and silhouette");
            if regular.0 == [246, 245, 241, 255] {
                assert_eq!(regular, dev);
            }
        }
    }
}

#[test]
fn dev_branding_is_opt_in_for_optimized_builds() {
    use crate::branding;
    assert_eq!(
        branding::IS_DEV,
        cfg!(any(debug_assertions, feature = "dev-icon"))
    );
    let (name, id) = if branding::IS_DEV {
        ("Startpoint Dev", "startpoint.dev")
    } else {
        ("Startpoint", "startpoint")
    };
    assert_eq!(branding::NAME, name);
    assert_eq!(branding::APP_ID, id);
    assert_eq!(branding::window_title(false), name);
    assert_eq!(
        branding::window_title(true),
        format!("{name} — Example library")
    );
}
