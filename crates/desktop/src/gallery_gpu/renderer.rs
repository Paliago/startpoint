//! wgpu owns scene shading and world-space visibility. The initial GPUI bridge
//! reads a bounded offscreen target on this worker; it is deliberately not
//! described as zero-copy. No GPU waits or image decoding run on the UI thread.
use super::material;
use super::scene::{MAX_COVERS, POSTER_HEIGHT, POSTER_WIDTH, Scene};
use bytemuck::{Pod, Zeroable};
use gpui::RenderImage;
use image::{Frame, RgbaImage};
use std::{path::PathBuf, sync::Arc, time::Duration};

const LAYERS: u32 = 16;
const ART_WIDTH: u32 = 512;
const ART_HEIGHT: u32 = 768;
const MIPS: u32 = 10;
pub const FRAME_GUTTER: u32 = 2;
// Match GPUI's native BGRA storage so the bridge only copies rows, not every
// individual pixel's channels. Artwork remains standard sRGB RGBA on upload.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
const ART_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[derive(Clone)]
pub struct FrameOutput {
    pub scene: Scene,
    pub image: Arc<RenderImage>,
}

#[derive(Clone, PartialEq, Eq)]
enum ArtKey {
    File(PathBuf),
    Placeholder(String),
}
struct Layer {
    key: ArtKey,
    tint: [f32; 3],
    touched: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PosterUniform {
    center: [f32; 4],
    shape: [f32; 4],
    tint: [f32; 4],
    basis: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    viewport: [f32; 4],
    eye: [f32; 4],
    forward: [f32; 4],
    up: [f32; 4],
    optics: [f32; 4],
    liquid: [f32; 4],
    posters: [PosterUniform; MAX_COVERS],
}

struct Targets {
    width: u32,
    height: u32,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    readback: wgpu::Buffer,
    row_bytes: u32,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    art: wgpu::Texture,
    layers: Vec<Option<Layer>>,
    serial: u64,
    targets: Option<Targets>,
}

impl Renderer {
    pub fn new() -> Result<Self, String> {
        pollster::block_on(async {
            let instance = wgpu::Instance::new(
                wgpu::InstanceDescriptor::new_without_display_handle_from_env(),
            );
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await
                .map_err(|e| format!("No gallery GPU adapter: {e}"))?;
            crate::diagnostics::record(&format!(
                "Gallery wgpu backend: {:?}",
                adapter.get_info().backend
            ));
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("Startpoint Gallery"),
                    ..Default::default()
                })
                .await
                .map_err(|e| format!("Gallery GPU device: {e}"))?;
            let errors = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Gallery world-space scene"),
                source: wgpu::ShaderSource::Wgsl(include_str!("gallery.wgsl").into()),
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Gallery perspective / reflective floor"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
            let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Gallery camera and planes"),
                size: std::mem::size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let art = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Bounded gallery artwork array"),
                size: wgpu::Extent3d {
                    width: ART_WIDTH,
                    height: ART_HEIGHT,
                    depth_or_array_layers: LAYERS,
                },
                mip_level_count: MIPS,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: ART_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = art.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            });
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Clamped trilinear artwork"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            });
            let material = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Cached polished concrete roughness / reflectivity / wear"),
                size: wgpu::Extent3d {
                    width: material::SIZE,
                    height: material::SIZE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: material::MIPS,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let mut grain = material::pixels();
            for mip in 0..material::MIPS {
                if mip > 0 {
                    grain = image::imageops::resize(
                        &grain,
                        material::SIZE >> mip,
                        material::SIZE >> mip,
                        image::imageops::FilterType::Triangle,
                    );
                }
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &material,
                        mip_level: mip,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    grain.as_raw(),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(grain.width() * 4),
                        rows_per_image: Some(grain.height()),
                    },
                    wgpu::Extent3d {
                        width: grain.width(),
                        height: grain.height(),
                        depth_or_array_layers: 1,
                    },
                );
            }
            let material_view = material.create_view(&Default::default());
            let material_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Repeating floor material"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Gallery scene bindings"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&material_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Sampler(&material_sampler),
                    },
                ],
            });
            if let Some(error) = errors.pop().await {
                return Err(format!("Gallery shader validation: {error}"));
            }
            Ok(Self {
                device,
                queue,
                pipeline,
                uniforms,
                bind_group,
                art,
                layers: (0..LAYERS).map(|_| None).collect(),
                serial: 0,
                targets: None,
            })
        })
    }

    fn layer(&mut self, key: ArtKey, pinned: &[ArtKey]) -> (u32, [f32; 3]) {
        if let Some((slot, layer)) = self
            .layers
            .iter_mut()
            .enumerate()
            .find_map(|(slot, layer)| {
                layer
                    .as_mut()
                    .filter(|layer| layer.key == key)
                    .map(|layer| (slot, layer))
            })
        {
            layer.touched = self.serial;
            return (slot as u32, layer.tint);
        }
        let slot = self
            .layers
            .iter()
            .position(Option::is_none)
            .unwrap_or_else(|| {
                self.layers
                    .iter()
                    .enumerate()
                    .filter(|(_, layer)| {
                        layer
                            .as_ref()
                            .is_some_and(|layer| !pinned.contains(&layer.key))
                    })
                    .min_by_key(|(_, layer)| layer.as_ref().unwrap().touched)
                    .map(|(slot, _)| slot)
                    .expect("texture layers hold the bounded visible and prefetched working set")
            });
        let artwork = match &key {
            ArtKey::File(path) => image::open(path).ok(),
            _ => None,
        };
        let mut pixels = artwork
            .map(|image| {
                image
                    .resize_to_fill(ART_WIDTH, ART_HEIGHT, image::imageops::FilterType::Lanczos3)
                    .to_rgba8()
            })
            .unwrap_or_else(|| placeholder(slot as u32));
        let mut tint = [0.; 3];
        let mut samples = 0.;
        for pixel in pixels.pixels().step_by(193) {
            for channel in 0..3 {
                tint[channel] += (f32::from(pixel[channel]) / 255.).powf(2.2);
            }
            samples += 1.;
        }
        for channel in &mut tint {
            *channel /= samples;
        }
        for mip in 0..MIPS {
            if mip > 0 {
                pixels = image::imageops::resize(
                    &pixels,
                    (ART_WIDTH >> mip).max(1),
                    (ART_HEIGHT >> mip).max(1),
                    image::imageops::FilterType::Triangle,
                );
            }
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.art,
                    mip_level: mip,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: slot as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                pixels.as_raw(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(pixels.width() * 4),
                    rows_per_image: Some(pixels.height()),
                },
                wgpu::Extent3d {
                    width: pixels.width(),
                    height: pixels.height(),
                    depth_or_array_layers: 1,
                },
            );
        }
        self.layers[slot] = Some(Layer {
            key,
            tint,
            touched: self.serial,
        });
        (slot as u32, tint)
    }

    #[cfg(test)]
    pub fn render(&mut self, scene: Scene) -> Result<FrameOutput, String> {
        self.render_if_current(scene, &|| true)?
            .ok_or_else(|| "Gallery render cancelled".into())
    }

    pub fn release_targets(&mut self) {
        self.targets = None;
    }

    pub fn prefetch(&mut self, scene: &Scene, allowed: &impl Fn() -> bool) {
        let key = |cover: &super::scene::Cover| {
            cover
                .path
                .clone()
                .map(ArtKey::File)
                .unwrap_or_else(|| ArtKey::Placeholder(cover.id.0.clone()))
        };
        let pinned = scene
            .covers
            .iter()
            .chain(&scene.prefetch)
            .map(key)
            .collect::<Vec<_>>();
        for cover in &scene.prefetch {
            if !allowed() {
                break;
            }
            self.layer(key(cover), &pinned);
        }
    }

    pub fn render_if_current(
        &mut self,
        scene: Scene,
        allowed: &impl Fn() -> bool,
    ) -> Result<Option<FrameOutput>, String> {
        if !allowed() {
            return Ok(None);
        }
        self.serial += 1;
        let keys = scene
            .covers
            .iter()
            .map(|cover| {
                cover
                    .path
                    .clone()
                    .map(ArtKey::File)
                    .unwrap_or_else(|| ArtKey::Placeholder(cover.id.0.clone()))
            })
            .collect::<Vec<_>>();
        let mut data = Uniforms::zeroed();
        data.viewport = [
            scene.width as f32,
            scene.height as f32,
            scene.logical_width / scene.logical_height,
            scene.covers.len() as f32,
        ];
        data.eye = [
            scene.camera.eye[0],
            scene.camera.eye[1],
            scene.camera.eye[2],
            0.,
        ];
        data.forward = [
            scene.camera.forward[0],
            scene.camera.forward[1],
            scene.camera.forward[2],
            0.,
        ];
        data.up = [
            scene.camera.up[0],
            scene.camera.up[1],
            scene.camera.up[2],
            0.,
        ];
        data.optics = [scene.camera.tangent, scene.camera.shift, 0.25, 0.];
        data.liquid = scene.liquid.unwrap_or([-1., 0., 0., 0.]);
        for (index, cover) in scene.covers.iter().enumerate() {
            if !allowed() {
                return Ok(None);
            }
            let poster = scene.poster(cover);
            let (layer, tint) = self.layer(keys[index].clone(), &keys);
            data.posters[index] = PosterUniform {
                center: [
                    poster.center[0],
                    poster.center[1],
                    poster.center[2],
                    layer as f32,
                ],
                shape: [POSTER_WIDTH / 2., POSTER_HEIGHT, poster.yaw, poster.opacity],
                tint: [tint[0], tint[1], tint[2], poster.shade],
                basis: [poster.yaw.cos(), poster.yaw.sin(), 0., 0.],
            };
        }
        if !allowed() {
            return Ok(None);
        }
        self.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&data));
        if self
            .targets
            .as_ref()
            .is_none_or(|target| target.width != scene.width || target.height != scene.height)
        {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Gallery offscreen color"),
                size: wgpu::Extent3d {
                    width: scene.width,
                    height: scene.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let row_bytes = (scene.width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
                * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Bounded GPUI frame bridge"),
                size: u64::from(row_bytes) * u64::from(scene.height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            self.targets = Some(Targets {
                width: scene.width,
                height: scene.height,
                texture,
                view,
                row_bytes,
                readback,
            });
        }
        let targets = self.targets.as_ref().unwrap();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Gallery frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("World-space posters and textured floor"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &targets.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &targets.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(targets.row_bytes),
                    rows_per_image: Some(scene.height),
                },
            },
            wgpu::Extent3d {
                width: scene.width,
                height: scene.height,
                depth_or_array_layers: 1,
            },
        );
        if !allowed() {
            return Ok(None);
        }
        let submission = self.queue.submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        targets
            .readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = send.send(result);
            });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(5)),
            })
            .map_err(|e| format!("Gallery GPU wait: {e}"))?;
        receive
            .recv_timeout(Duration::from_secs(1))
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        if !allowed() {
            targets.readback.unmap();
            return Ok(None);
        }
        // Zero allocation + row copies avoids initializing every pixel twice.
        // Only the tiny atlas guard needs an explicit background fill.
        let mut pixels = RgbaImage::new(
            scene.width + FRAME_GUTTER * 2,
            scene.height + FRAME_GUTTER * 2,
        );
        let background = image::Rgba([9, 10, 12, 255]);
        for y in 0..FRAME_GUTTER {
            for x in 0..pixels.width() {
                pixels.put_pixel(x, y, background);
                pixels.put_pixel(x, scene.height + FRAME_GUTTER + y, background);
            }
        }
        for y in FRAME_GUTTER..scene.height + FRAME_GUTTER {
            for x in 0..FRAME_GUTTER {
                pixels.put_pixel(x, y, background);
                pixels.put_pixel(scene.width + FRAME_GUTTER + x, y, background);
            }
        }
        {
            let mapped = targets.readback.slice(..).get_mapped_range();
            for y in 0..scene.height {
                let source = &mapped[(y * targets.row_bytes) as usize
                    ..(y * targets.row_bytes + scene.width * 4) as usize];
                let start = (((y + FRAME_GUTTER) * pixels.width() + FRAME_GUTTER) * 4) as usize;
                let destination: &mut [u8] = pixels.as_mut();
                let row = &mut destination[start..start + source.len()];
                row.copy_from_slice(source);
            }
        }
        targets.readback.unmap();
        Ok(Some(FrameOutput {
            scene,
            image: Arc::new(RenderImage::new([Frame::new(pixels)])),
        }))
    }
}

fn placeholder(index: u32) -> RgbaImage {
    let color = [
        [65., 98., 81.],
        [137., 72., 35.],
        [54., 76., 115.],
        [128., 103., 57.],
    ][index as usize % 4];
    RgbaImage::from_fn(ART_WIDTH, ART_HEIGHT, |x, y| {
        let light = 0.5 + 0.5 * ((x as f32 * 0.012 + y as f32 * 0.006).sin()).abs();
        image::Rgba([
            (color[0] * light) as u8,
            (color[1] * light) as u8,
            (color[2] * light) as u8,
            255,
        ])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Requires a native wgpu adapter; run explicitly for GPU shader/readback validation"]
    fn renders_real_gpu_scene_and_floor() {
        use super::super::scene::Cover;
        let mut renderer = Renderer::new().expect("wgpu adapter and valid WGSL");
        let scene = Scene::new(
            640.,
            410.,
            (-2..=2)
                .map(|i| Cover {
                    id: startpoint_sdk::GameId(format!("gpu:{i}")),
                    path: None,
                    offset: i as f32,
                })
                .collect(),
        );
        let frame = renderer.render(scene).expect("GPU frame and readback");
        let bytes = frame.image.as_bytes(0).unwrap();
        assert!(bytes.chunks_exact(4).any(|pixel| pixel[1] > 80));
        assert!(bytes.chunks_exact(4).all(|pixel| pixel[3] == 255));
        let steady_scene = Scene::new(1280., 820., frame.scene.covers.clone());
        for _ in 0..5 {
            renderer
                .render(steady_scene.clone())
                .expect("reused GPU targets and texture cache");
        }
        let mut timings = Vec::new();
        for _ in 0..30 {
            let started = std::time::Instant::now();
            renderer.render(steady_scene.clone()).unwrap();
            timings.push(started.elapsed().as_secs_f64() * 1000.);
        }
        timings.sort_by(f64::total_cmp);
        eprintln!(
            "Warm 1280x820 scene including readback/CPU copy (not GPUI upload): median {:.2} ms, p95 {:.2} ms (30 frames)",
            timings[15], timings[28]
        );
        renderer.release_targets();
        let before = renderer.serial;
        assert!(
            renderer
                .render_if_current(steady_scene.clone(), &|| false)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            renderer.serial, before,
            "suspended requests never prepare or submit a frame"
        );
        assert!(renderer.targets.is_none());
        let checkpoints = std::cell::Cell::new(0);
        assert!(
            renderer
                .render_if_current(steady_scene.clone(), &|| {
                    checkpoints.set(checkpoints.get() + 1);
                    checkpoints.get() < 3
                })
                .unwrap()
                .is_none()
        );
        assert!(
            renderer.targets.is_none(),
            "cancel between preparing covers and GPU submission"
        );
        renderer
            .render(steady_scene)
            .expect("resume recreates released targets");
        let liquid = renderer
            .render(Scene::reel(1280., 820., 0., [0.2, 0.75, 0.5]))
            .unwrap();
        let later = renderer
            .render(Scene::reel(1280., 820., 8., [0.2, 0.75, 0.5]))
            .unwrap();
        assert_ne!(liquid.image.as_bytes(0), later.image.as_bytes(0));
        assert!(
            later
                .image
                .as_bytes(0)
                .unwrap()
                .chunks_exact(4)
                .all(|pixel| pixel[3] == 255)
        );
        let mut liquid_timings = Vec::new();
        for i in 0..30 {
            let started = std::time::Instant::now();
            renderer
                .render(Scene::reel(
                    1280.,
                    820.,
                    8. + i as f32 / 30.,
                    [0.2, 0.75, 0.5],
                ))
                .unwrap();
            liquid_timings.push(started.elapsed().as_secs_f64() * 1000.);
        }
        liquid_timings.sort_by(f64::total_cmp);
        eprintln!(
            "Film Reel liquid including readback/CPU copy: median {:.2} ms, p95 {:.2} ms",
            liquid_timings[15], liquid_timings[28]
        );
        if let Ok(path) = std::env::var("STARTPOINT_LIQUID_CAPTURE") {
            let mut pixels = RgbaImage::from_raw(
                later.scene.width + FRAME_GUTTER * 2,
                later.scene.height + FRAME_GUTTER * 2,
                later.image.as_bytes(0).unwrap().to_vec(),
            )
            .unwrap();
            for pixel in pixels.pixels_mut() {
                pixel.0.swap(0, 2);
            }
            pixels.save(path).unwrap();
        }
        if let Ok(path) = std::env::var("STARTPOINT_GPU_CAPTURE") {
            let mut pixels = RgbaImage::from_raw(
                frame.scene.width + FRAME_GUTTER * 2,
                frame.scene.height + FRAME_GUTTER * 2,
                bytes.to_vec(),
            )
            .unwrap();
            for pixel in pixels.pixels_mut() {
                pixel.0.swap(0, 2);
            }
            pixels.save(path).unwrap();
        }
    }
}
