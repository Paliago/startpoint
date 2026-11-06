use num_traits::ToPrimitive;
use rune::compose::{Compositor, DeviceKey, Frame};
use rune::render::{EffectPhase, KIND_EFFECT};
use rune::{El, effect, el};
use rune_fx::{
    DitherParams, DitherPattern, ElectricParams, FireParams, FlutedGlassParams, Fx, FxFrame,
    FxOptions, FxRenderer, FxSlot, GlowParams, GlowStyle, LiquidMetalParams, MeshGradientParams,
    PlasticParams, Shape,
};

use crate::component_effects::ComponentEffect;
use crate::state::State;

const ARTWORK_DITHER: u32 = 1;

pub fn artwork_dither(amount: f32, pixel: f32) -> El {
    effect(ARTWORK_DITHER)
        .key("artwork-dither")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .effect_backdrop(true)
        .effect_data([amount.clamp(0.0, 1.0), pixel.clamp(1.0, 12.0), 0.0, 0.0])
        .inert()
}

pub fn artwork_surface(image: El, amount: f32, pixel: f32) -> El {
    if amount <= 0.0 {
        return image;
    }
    el().child(image.absolute(0.0, 0.0).w_full().h_full())
        .child(artwork_dither(amount, pixel))
}

struct Resources {
    device: DeviceKey,
    format: wgpu::TextureFormat,
    renderer: FxRenderer,
}

#[derive(Default)]
pub struct EffectsCompositor {
    resources: Option<Resources>,
    animating: bool,
    preview_time: f32,
    preview_last_time: Option<f64>,
    preview_running: bool,
}

impl Compositor<State> for EffectsCompositor {
    fn animating(&self) -> bool {
        self.animating
    }

    fn render(&mut self, frame: &mut Frame<'_, State>) {
        let mut indices = Vec::new();
        let mut layers = Vec::new();
        let mut placements = Vec::new();
        self.animating = false;
        let reduced = frame.rt.system_reduced_motion() || frame.rt.state.reduced_motion;
        let paused = (!frame.rt.state.window_focused
            && frame.rt.state.appearance.pause_in_background)
            || frame.rt.state.overlay != crate::state::Overlay::None;
        let preview_active = frame.rt.state.developer_tools
            && frame.rt.state.screen == crate::state::Screen::ComponentEffects;
        let running = preview_active
            && frame.rt.state.effects_preview.running()
            && (frame.rt.state.window_focused || frame.rt.state.headless)
            && frame.rt.state.open_select.is_none()
            && !reduced
            && !paused;
        if running && self.preview_running {
            let elapsed = self
                .preview_last_time
                .map_or(0.0, |previous| (frame.rt.t - previous).clamp(0.0, 0.1));
            self.preview_time +=
                elapsed.to_f32().unwrap_or(0.0) * frame.rt.state.effects_preview.rate();
        }
        self.preview_last_time = Some(frame.rt.t);
        self.preview_running = running;
        for (index, instance) in frame.rt.instances.iter().enumerate() {
            if instance.params[1] != KIND_EFFECT {
                continue;
            }
            let Some(mark) = frame.rt.marks.get(index) else {
                continue;
            };
            let scale = instance.params[3].max(0.001);
            let [x, y, width, height] = instance.rect.map(|value| value / scale);
            let radius = instance
                .radii
                .iter()
                .fold(0.0_f32, |radius, value| radius.max(value.abs()))
                / scale;
            let shape = Shape::rounded(x, y, width, height, radius);
            let layer = match *mark {
                ARTWORK_DITHER => {
                    let [amount, pixel, ..] = instance.uv;
                    if amount <= 0.0 {
                        continue;
                    }
                    Fx::fill(DitherParams {
                        pattern: DitherPattern::Bayer4,
                        pixel,
                        contrast: 1.0,
                        bias: 0.0,
                        ..DitherParams::backdrop()
                    })
                    .on(shape)
                    .over()
                    .opacity(amount)
                }
                mark if preview_active => {
                    let Some(style) = ComponentEffect::from_mark(mark) else {
                        continue;
                    };
                    let strength = instance.uv[0];
                    let Some(effect) = component_effect(style, strength) else {
                        continue;
                    };
                    effect
                        .on(shape)
                        .at(self.preview_time)
                        .reduced(reduced || frame.rt.state.effects_preview.reduced_motion)
                }
                _ => continue,
            };
            if let Ok(index) = u32::try_from(index) {
                self.animating |= layer.is_animating()
                    && !reduced
                    && !paused
                    && (*mark == ARTWORK_DITHER || running);
                indices.push(index);
                layers.push(layer);
                placements.push(
                    FxSlot::default()
                        .scale(scale)
                        .clip(instance.clip, instance.fade)
                        .backdrop_srgb(frame.srgb_blend()),
                );
            }
        }
        if layers.is_empty() {
            frame.draw_ui();
            return;
        }
        let device_key = frame.device_key();
        if self.resources.as_ref().is_none_or(|resources| {
            resources.device != device_key || resources.format != frame.format
        }) {
            let mut renderer =
                FxRenderer::new(frame.device, frame.format, FxOptions { dither: true });
            renderer.add_format(frame.device, wgpu::TextureFormat::Rgba16Float);
            self.resources = Some(Resources {
                device: device_key,
                format: frame.format,
                renderer,
            });
        }
        let Some(resources) = self.resources.as_mut() else {
            frame.draw_ui();
            return;
        };
        let fx = &mut resources.renderer;
        fx.set_srgb_blend(frame.srgb_blend());
        fx.prepare_with_slots(
            frame.device,
            frame.queue,
            &FxFrame {
                viewport: [frame.size.0, frame.size.1],
                scale: 1.0,
                layers: &layers,
                backdrop: None,
                sources: &[],
            },
            &placements,
        );
        let clear = frame.rt.clear_color();
        frame.ui.prepare(
            frame.device,
            frame.queue,
            &mut frame.rt.text.atlas,
            &frame.rt.instances,
            frame.size,
        );
        frame
            .ui
            .draw_with_backdrop(frame.encoder, frame.target, clear, &mut |phase, site| {
                let Some(layer) = indices.iter().position(|index| *index == site.index) else {
                    return;
                };
                match phase {
                    EffectPhase::Backdrop(encoder) => {
                        if let Some(backdrop) = site.backdrop {
                            fx.blur_backdrop(frame.device, frame.queue, encoder, backdrop, layer);
                        }
                    }
                    EffectPhase::Draw(pass) => {
                        fx.draw_layer_in(pass, site.format, site.origin, layer)
                    }
                }
            });
    }
}

fn component_effect(style: ComponentEffect, strength: f32) -> Option<Fx> {
    Some(match style {
        ComponentEffect::None => return None,
        ComponentEffect::Aura => Fx::glow(GlowParams {
            style: GlowStyle::Aura,
            intensity: strength,
            radius: 20.0,
            ..Default::default()
        }),
        ComponentEffect::Metal => Fx::fill(LiquidMetalParams {
            flow: strength,
            bevel: 10.0,
            ..LiquidMetalParams::chrome()
        }),
        ComponentEffect::Charged => Fx::electric(ElectricParams {
            intensity: strength,
            ..ElectricParams::charged()
        }),
        ComponentEffect::Fire => Fx::fire(FireParams {
            intensity: strength,
            ..FireParams::heat()
        }),
        ComponentEffect::Glass => Fx::fill(FlutedGlassParams {
            refraction: 9.0 * strength,
            ..FlutedGlassParams::reeded()
        }),
        ComponentEffect::Mesh => Fx::fill(MeshGradientParams {
            warp: strength * 0.4,
            ..MeshGradientParams::dusk()
        }),
        ComponentEffect::Plastic => Fx::fill(PlasticParams {
            dome: strength * 0.4,
            ..PlasticParams::glossy()
        }),
    })
}
