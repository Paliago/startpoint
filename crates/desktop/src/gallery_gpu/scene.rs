//! The same camera and world-space poster geometry drive WGSL rendering and
//! native GPUI hit targets. There is no separate CSS-like scale approximation.
use crate::gallery::{Layout, Placement};
use startpoint_sdk::GameId;
use std::path::PathBuf;

pub const MAX_COVERS: usize = 7;
pub const POSTER_HEIGHT: f32 = 5.;
pub const POSTER_WIDTH: f32 = POSTER_HEIGHT * 2. / 3.;

#[derive(Clone, Debug, PartialEq)]
pub struct Cover {
    pub id: GameId,
    pub path: Option<PathBuf>,
    pub offset: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub width: u32,
    pub height: u32,
    pub logical_width: f32,
    pub logical_height: f32,
    pub camera: Camera,
    pub covers: Vec<Cover>,
    pub prefetch: Vec<Cover>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub eye: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub tangent: f32,
    pub shift: f32,
}

#[derive(Clone, Copy)]
pub struct Poster {
    pub center: [f32; 3],
    pub yaw: f32,
    pub opacity: f32,
    pub shade: f32,
}

impl Scene {
    pub fn new(width: f32, height: f32, covers: Vec<Cover>) -> Self {
        let width = width.max(1.);
        let height = height.max(1.);
        let layout = Layout::new(width, height);
        let eye = [0., 0.9, 9.5];
        let pitch: f32 = 0.115;
        let forward = [0., pitch.sin(), -pitch.cos()];
        let up = [0., pitch.cos(), pitch.sin()];
        let bottom = dot([0., -eye[1], -eye[2]], up) / dot([0., -eye[1], -eye[2]], forward);
        let top = dot([0., POSTER_HEIGHT - eye[1], -eye[2]], up)
            / dot([0., POSTER_HEIGHT - eye[1], -eye[2]], forward);
        let tangent = (top - bottom) / (2. * layout.cover_height / height);
        let shift = 1. - 2. * layout.baseline / height - bottom / tangent;
        // The native UI stays at full DPI. Bound the initial copy bridge to
        // ~1.6 MP, and never allocate a full 4K target for each animation frame.
        let scale = (1600. / width).min(1000. / height).min(1.25);
        Self {
            width: (width * scale).round().max(1.) as u32,
            height: (height * scale).round().max(1.) as u32,
            logical_width: width,
            logical_height: height,
            camera: Camera {
                eye,
                forward,
                up,
                tangent,
                shift,
            },
            covers: covers.into_iter().take(MAX_COVERS).collect(),
            prefetch: Vec::new(),
        }
    }

    pub fn poster(&self, cover: &Cover) -> Poster {
        let distance = cover.offset.abs();
        let placement = Layout::new(self.logical_width, self.logical_height).place(cover.offset);
        let depth = distance * 4.;
        let center_depth = dot(
            [
                0.,
                POSTER_HEIGHT / 2. - self.camera.eye[1],
                -depth - self.camera.eye[2],
            ],
            self.camera.forward,
        );
        let screen_shift = placement.x + placement.width / 2. - self.logical_width / 2.;
        Poster {
            center: [
                screen_shift * 2. * self.camera.tangent * center_depth / self.logical_height,
                0.,
                -depth,
            ],
            yaw: -cover.offset.signum() * distance.min(1.) * 0.035,
            opacity: placement.opacity,
            shade: placement.shade,
        }
    }

    pub fn project(&self, point: [f32; 3]) -> [f32; 2] {
        let delta = [
            point[0] - self.camera.eye[0],
            point[1] - self.camera.eye[1],
            point[2] - self.camera.eye[2],
        ];
        let depth = dot(delta, self.camera.forward).max(0.001);
        let x = delta[0] / (depth * self.camera.tangent * self.logical_width / self.logical_height);
        let y = dot(delta, self.camera.up) / (depth * self.camera.tangent) + self.camera.shift;
        [
            (x + 1.) * self.logical_width / 2.,
            (1. - y) * self.logical_height / 2.,
        ]
    }

    pub fn placements(&self) -> Vec<(GameId, Placement)> {
        self.covers
            .iter()
            .map(|cover| {
                let poster = self.poster(cover);
                let tangent = [poster.yaw.cos(), 0., -poster.yaw.sin()];
                let mut min = [f32::MAX; 2];
                let mut max = [f32::MIN; 2];
                for x in [-POSTER_WIDTH / 2., POSTER_WIDTH / 2.] {
                    for y in [0., POSTER_HEIGHT] {
                        let p = self.project([
                            poster.center[0] + tangent[0] * x,
                            y,
                            poster.center[2] + tangent[2] * x,
                        ]);
                        for axis in 0..2 {
                            min[axis] = min[axis].min(p[axis]);
                            max[axis] = max[axis].max(p[axis]);
                        }
                    }
                }
                (
                    cover.id.clone(),
                    Placement {
                        x: min[0],
                        y: min[1],
                        width: max[0] - min[0],
                        height: max[1] - min[1],
                        opacity: poster.opacity,
                        shade: poster.shade,
                    },
                )
            })
            .collect()
    }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn camera_and_native_hit_geometry_share_the_same_projection_at_every_viewport() {
        for (w, h) in [(960., 620.), (1280., 820.), (2560., 1440.), (960., 1200.)] {
            let scene = Scene::new(
                w,
                h,
                vec![Cover {
                    id: GameId("center".into()),
                    path: None,
                    offset: 0.,
                }],
            );
            let layout = Layout::new(w, h);
            let bottom = scene.project([0., 0., 0.]);
            let top = scene.project([0., POSTER_HEIGHT, 0.]);
            assert!((bottom[1] - layout.baseline).abs() < 0.01);
            assert!((top[1] - (layout.baseline - layout.cover_height)).abs() < 0.01);
            assert!(scene.width <= 1600 && scene.height <= 1000);
            for (_, placement) in scene.placements() {
                assert!(placement.width > 0. && placement.height > 0. && placement.x.is_finite());
            }
        }
    }
}
