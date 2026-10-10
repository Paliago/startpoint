use gpui::{App, Bounds, Context, Pixels, Task, Window, WindowBounds, point, px, size};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq)]
struct Placement {
    bounds: Bounds<Pixels>,
    maximized: bool,
}

impl Placement {
    fn read(path: &Path) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        let number = |key| value.get(key)?.as_f64().map(|n| n as f32);
        let placement = Self {
            bounds: Bounds::new(
                point(px(number("x")?), px(number("y")?)),
                size(px(number("width")?), px(number("height")?)),
            ),
            maximized: value.get("maximized")?.as_bool()?,
        };
        valid(placement.bounds).then_some(placement)
    }

    fn write(self, path: &Path) {
        let value = serde_json::json!({
            "x": f32::from(self.bounds.origin.x), "y": f32::from(self.bounds.origin.y),
            "width": f32::from(self.bounds.size.width), "height": f32::from(self.bounds.size.height),
            "maximized": self.maximized,
        });
        let result = (|| -> std::io::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            // A separate file keeps geometry writes away from application preferences.
            std::fs::write(path, serde_json::to_vec_pretty(&value)?)
        })();
        if result.is_err() {
            crate::diagnostics::record("window placement could not be saved");
        }
    }
}

fn valid(bounds: Bounds<Pixels>) -> bool {
    [
        bounds.origin.x,
        bounds.origin.y,
        bounds.size.width,
        bounds.size.height,
    ]
    .iter()
    .all(|n| f32::from(*n).is_finite())
        && bounds.size.width >= px(960.)
        && bounds.size.height >= px(620.)
        && bounds.size.width <= px(32768.)
        && bounds.size.height <= px(32768.)
}

fn fit(bounds: Bounds<Pixels>, displays: &[Bounds<Pixels>]) -> Option<Bounds<Pixels>> {
    let display = displays.iter().max_by(|a, b| {
        let overlap = |display: &Bounds<Pixels>| {
            f32::from(
                (bounds.right().min(display.right()) - bounds.left().max(display.left()))
                    .max(px(0.)),
            ) * f32::from(
                (bounds.bottom().min(display.bottom()) - bounds.top().max(display.top()))
                    .max(px(0.)),
            )
        };
        overlap(a).total_cmp(&overlap(b))
    })?;
    // Recover windows from removed monitors and keep the top/close controls reachable.
    let width = bounds.size.width.min(display.size.width).max(px(960.));
    let height = bounds.size.height.min(display.size.height).max(px(620.));
    Some(Bounds::new(
        point(
            bounds.origin.x.clamp(
                display.left(),
                (display.right() - width).max(display.left()),
            ),
            bounds.origin.y.clamp(
                display.top(),
                (display.bottom() - height).max(display.top()),
            ),
        ),
        size(width, height),
    ))
}

pub(crate) fn restore(directory: &Path, cx: &App) -> WindowBounds {
    let saved = Placement::read(&directory.join("window-state.json"));
    let displays = cx
        .displays()
        .iter()
        .map(|display| display.bounds())
        .collect::<Vec<_>>();
    if let Some(saved) = saved
        && let Some(bounds) = fit(saved.bounds, &displays)
    {
        if saved.maximized {
            WindowBounds::Maximized(bounds)
        } else {
            WindowBounds::Windowed(bounds)
        }
    } else {
        WindowBounds::centered(size(px(1280.), px(820.)), cx)
    }
}

pub(crate) struct Memory {
    path: PathBuf,
    latest: Arc<Mutex<Option<Placement>>>,
    save: Option<Task<()>>,
}

impl Memory {
    pub fn new(directory: &Path) -> Self {
        Self {
            path: directory.join("window-state.json"),
            latest: Arc::new(Mutex::new(None)),
            save: None,
        }
    }

    pub fn capture<T: 'static>(&mut self, window: &Window, cx: &Context<T>) {
        let bounds = window.window_bounds();
        let placement = Placement {
            bounds: bounds.get_bounds(),
            maximized: matches!(bounds, WindowBounds::Maximized(_)),
        };
        if !valid(placement.bounds) {
            return;
        }
        let mut latest = self.latest.lock().unwrap();
        if *latest == Some(placement) {
            return;
        }
        *latest = Some(placement);
        drop(latest);
        let path = self.path.clone();
        let latest = self.latest.clone();
        let executor = cx.background_executor().clone();
        self.save = Some(cx.background_executor().spawn(async move {
            executor.timer(Duration::from_millis(300)).await;
            // Hold the lock through the short write so shutdown cannot race a stale write.
            if let Some(placement) = *latest.lock().unwrap() {
                placement.write(&path);
            }
        }));
    }

    pub fn flush(&self) {
        if let Some(placement) = *self.latest.lock().unwrap() {
            placement.write(&self.path);
        }
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_negative_monitor_coordinates_and_recovers_offscreen_windows() {
        let left = Bounds::new(point(px(-1920.), px(0.)), size(px(1920.), px(1080.)));
        let main = Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.)));
        let saved = Bounds::new(point(px(-1700.), px(100.)), size(px(1200.), px(800.)));
        assert_eq!(fit(saved, &[left, main]), Some(saved));
        let recovered = fit(saved, &[main]).unwrap();
        assert_eq!(recovered.origin.x, px(0.));
        assert!(recovered.bottom() <= main.bottom());
        assert!(!valid(Bounds::new(point(px(f32::NAN), px(0.)), saved.size)));
        assert!(!valid(Bounds::new(
            point(px(0.), px(0.)),
            size(px(0.), px(0.))
        )));
    }

    #[test]
    fn placement_round_trips_size_location_and_maximized_state() {
        let path = std::env::temp_dir().join(format!(
            "startpoint-window-state-{}.json",
            std::process::id()
        ));
        let saved = Placement {
            bounds: Bounds::new(point(px(-1500.), px(80.)), size(px(1200.), px(780.))),
            maximized: true,
        };
        saved.write(&path);
        assert_eq!(Placement::read(&path), Some(saved));
        std::fs::write(&path, b"{corrupt").unwrap();
        assert!(Placement::read(&path).is_none());
        std::fs::remove_file(path).unwrap();
    }
}
