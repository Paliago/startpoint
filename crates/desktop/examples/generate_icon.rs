use std::{
    fs,
    path::{Path, PathBuf},
};

const SIZES: [u32; 9] = [16, 20, 24, 32, 40, 48, 64, 128, 256];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/icons");
    let svg = fs::read(directory.join("startpoint.svg"))?;
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default())?;
    let render = |size: u32| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut image = resvg::tiny_skia::Pixmap::new(size, size).ok_or("invalid icon size")?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(
                size as f32 / tree.size().width(),
                size as f32 / tree.size().height(),
            ),
            &mut image.as_mut(),
        );
        Ok(image.encode_png()?)
    };
    write_if_changed(&directory.join("startpoint.png"), &render(512)?)?;
    let frames = SIZES
        .into_iter()
        .map(|size| render(size).map(|png| (size, png)))
        .collect::<Result<Vec<_>, _>>()?;
    // Windows Vista+ supports PNG frames in ICO files, retaining full alpha.
    let mut ico = Vec::new();
    ico.extend_from_slice(&0u16.to_le_bytes());
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&(frames.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * frames.len() as u32;
    for (size, png) in &frames {
        let dimension = if *size == 256 { 0 } else { *size as u8 };
        ico.extend_from_slice(&[dimension, dimension, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in frames {
        ico.extend_from_slice(&png);
    }
    write_if_changed(&directory.join("startpoint.ico"), &ico)?;
    println!("Generated startpoint.png (512px) and startpoint.ico (16–256px) from startpoint.svg");
    Ok(())
}

fn write_if_changed(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if fs::read(path).is_ok_and(|previous| previous == bytes) {
        return Ok(());
    }
    fs::write(path, bytes)
}
