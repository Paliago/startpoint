//! A lazy, bounded wgpu scene worker under native GPUI controls. At most one
//! pending scene and one completed frame exist. Idle scenes do not render.
mod material;
mod renderer;
pub(crate) mod scene;
use gpui::{prelude::*, *};
use renderer::{FRAME_GUTTER, FrameOutput, Renderer};
use scene::Scene;
use std::sync::{Arc, Condvar, Mutex};

struct Request {
    scene: Scene,
    generation: u64,
}
struct Completed {
    generation: u64,
    result: Result<FrameOutput, String>,
}

#[derive(Default)]
struct Mailbox {
    pending: Option<Request>,
    stopped: bool,
    paused: bool,
    generation: u64,
}
impl Mailbox {
    fn set_paused(&mut self, paused: bool) {
        if self.paused != paused {
            self.paused = paused;
            self.generation = self.generation.wrapping_add(1);
            self.pending = None;
        }
    }
    fn current(&self, generation: u64) -> bool {
        !self.paused && !self.stopped && self.generation == generation
    }
    fn enqueue(&mut self, scene: Scene) {
        if !self.paused && !self.stopped {
            self.pending = Some(Request {
                scene,
                generation: self.generation,
            });
        }
    }
}
type Shared = Arc<(Mutex<Mailbox>, Condvar)>;

#[derive(Default)]
pub(crate) struct GalleryGpu {
    mailbox: Option<Shared>,
    requested: Option<Scene>,
    frame: Option<FrameOutput>,
    failed: bool,
    task: Option<Task<()>>,
    paused: bool,
}

impl GalleryGpu {
    pub fn frame(&self) -> Option<&FrameOutput> {
        self.frame.as_ref()
    }

    pub fn set_paused(&mut self, paused: bool) {
        if self.paused == paused {
            return;
        }
        self.paused = paused;
        self.requested = None;
        if let Some(mailbox) = &self.mailbox {
            let (state, wake) = &**mailbox;
            state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .set_paused(paused);
            wake.notify_all();
        }
    }

    #[cfg(test)]
    pub fn install_test_frame(&mut self, scene: Scene) {
        let pixels = image::RgbaImage::from_pixel(
            scene.width + FRAME_GUTTER * 2,
            scene.height + FRAME_GUTTER * 2,
            image::Rgba([9, 10, 12, 255]),
        );
        self.frame = Some(FrameOutput {
            scene,
            image: Arc::new(RenderImage::new([image::Frame::new(pixels)])),
        });
    }

    pub fn request(&mut self, scene: Scene, window: &mut Window, cx: &mut Context<Self>) {
        // Headless input/layout tests deliberately exercise the 2D fallback;
        // the ignored native GPU smoke test validates WGSL and actual readback.
        if cfg!(test) || self.paused || self.failed || self.requested.as_ref() == Some(&scene) {
            return;
        }
        if self.mailbox.is_none() {
            self.start(window, cx);
        }
        self.requested = Some(scene.clone());
        if let Some(mailbox) = &self.mailbox {
            let (state, wake) = &**mailbox;
            state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .enqueue(scene);
            wake.notify_one();
        }
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mailbox: Shared = Arc::default();
        let worker_mailbox = mailbox.clone();
        let (send, receive) = async_channel::bounded(1);
        let started = std::thread::Builder::new()
            .name("gallery-wgpu".into())
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut renderer: Option<Renderer> = None;
                    loop {
                        let request = {
                            let (state, wake) = &*worker_mailbox;
                            let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
                            while !state.stopped {
                                if state.paused {
                                    if let Some(renderer) = &mut renderer {
                                        renderer.release_targets();
                                    }
                                }
                                if !state.paused && state.pending.is_some() {
                                    break;
                                } else {
                                    state = wake.wait(state).unwrap_or_else(|e| e.into_inner());
                                }
                            }
                            if state.stopped {
                                break;
                            }
                            state.pending.take().unwrap()
                        };
                        let allowed = || {
                            worker_mailbox
                                .0
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .current(request.generation)
                        };
                        if !allowed() {
                            continue;
                        }
                        if renderer.is_none() {
                            match Renderer::new() {
                                Ok(created) => renderer = Some(created),
                                Err(error) => {
                                    let _ = send.send_blocking(Completed {
                                        generation: request.generation,
                                        result: Err(error),
                                    });
                                    break;
                                }
                            }
                        }
                        let result = renderer
                            .as_mut()
                            .unwrap()
                            .render_if_current(request.scene.clone(), &allowed);
                        let failed = result.is_err();
                        let frame = match result {
                            Ok(Some(frame)) => Some(Ok(frame)),
                            Ok(None) => None,
                            Err(error) => Some(Err(error)),
                        };
                        if let Some(result) = frame
                            && send
                                .send_blocking(Completed {
                                    generation: request.generation,
                                    result,
                                })
                                .is_err()
                        {
                            break;
                        }
                        if failed {
                            break;
                        }
                        if let Some(renderer) = &mut renderer {
                            renderer.prefetch(&request.scene, &|| {
                                let state =
                                    worker_mailbox.0.lock().unwrap_or_else(|e| e.into_inner());
                                state.current(request.generation) && state.pending.is_none()
                            });
                        }
                    }
                }));
                if outcome.is_err() {
                    let _ = send.send_blocking(Completed {
                        generation: 0,
                        result: Err(
                            "Gallery GPU worker stopped; using the native 2D fallback".into()
                        ),
                    });
                }
            });
        if started.is_err() {
            self.failed = true;
            crate::diagnostics::record(
                "Gallery GPU worker could not start; using native 2D fallback",
            );
            return;
        }
        self.mailbox = Some(mailbox);
        self.task = Some(cx.spawn_in(window, async move |entity, cx| {
            while let Ok(completed) = receive.recv().await {
                if entity
                    .update_in(cx, |view, window, cx| {
                        // A frame that finished after suspension must not cause
                        // background repaints or undo a frozen scene on resume.
                        let current = view.mailbox.as_ref().is_some_and(|mailbox| {
                            mailbox
                                .0
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .current(completed.generation)
                        });
                        if completed.result.is_ok() && !current {
                            return;
                        }
                        match completed.result {
                            Ok(frame) => {
                                if let Some(previous) = view.frame.replace(frame) {
                                    // Retire the atlas resource only at a frame boundary.
                                    window.on_next_frame(move |window, _| {
                                        let _ = window.drop_image(previous.image);
                                        window.refresh();
                                    });
                                }
                            }
                            Err(error) => {
                                crate::diagnostics::record(&error);
                                view.failed = true;
                                if let Some(previous) = view.frame.take() {
                                    window.on_next_frame(move |window, _| {
                                        let _ = window.drop_image(previous.image);
                                        window.refresh();
                                    });
                                }
                            }
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
}

#[cfg(test)]
mod power_tests {
    use super::{Mailbox, Scene};
    fn scene() -> Scene {
        Scene::new(1280., 820., vec![])
    }
    #[test]
    fn suspension_discards_queued_and_inflight_generations_and_accepts_new_work_after_resume() {
        let mut mailbox = Mailbox::default();
        mailbox.enqueue(scene());
        let old = mailbox.pending.as_ref().unwrap().generation;
        mailbox.set_paused(true);
        assert!(mailbox.pending.is_none());
        assert!(!mailbox.current(old));
        mailbox.enqueue(scene());
        assert!(mailbox.pending.is_none());
        mailbox.set_paused(false);
        assert!(!mailbox.current(old));
        mailbox.enqueue(scene());
        assert!(mailbox.current(mailbox.pending.as_ref().unwrap().generation));
    }
    #[test]
    fn mailbox_coalesces_to_one_latest_pose() {
        let mut mailbox = Mailbox::default();
        for width in 1000..2000 {
            mailbox.enqueue(Scene::new(width as f32, 820., vec![]));
        }
        assert_eq!(mailbox.pending.as_ref().unwrap().scene.logical_width, 1999.);
    }
}

impl Drop for GalleryGpu {
    fn drop(&mut self) {
        if let Some(mailbox) = &self.mailbox {
            let (state, wake) = &**mailbox;
            let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
            state.stopped = true;
            state.pending = None;
            wake.notify_one();
        }
    }
}

pub(crate) fn frame_image(frame: &FrameOutput) -> impl IntoElement + use<> {
    let image = frame.image.clone();
    let width = frame.scene.width as f32;
    let height = frame.scene.height as f32;
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let x = bounds.size.width * (FRAME_GUTTER as f32 / width);
            let y = bounds.size.height * (FRAME_GUTTER as f32 / height);
            let fitted = Bounds::new(
                point(bounds.origin.x - x, bounds.origin.y - y),
                size(bounds.size.width + x * 2., bounds.size.height + y * 2.),
            );
            let _ = window.paint_image_fitted(
                bounds,
                fitted,
                Corners::all(px(0.)),
                image.clone(),
                0,
                false,
            );
        },
    )
    .size_full()
}
