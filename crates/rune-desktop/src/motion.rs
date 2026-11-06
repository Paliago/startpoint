use num_traits::ToPrimitive;
use rune::{Curve, Easing, Spring, SpringPair, Transition, Tx};

pub struct PanelFold {
    pub progress: f32,
    from: f32,
    target: f32,
    started: f64,
}

impl PanelFold {
    pub fn new(open: bool) -> Self {
        let progress = if open { 1.0 } else { 0.0 };
        Self {
            progress,
            from: progress,
            target: progress,
            started: 0.0,
        }
    }

    pub fn update(&mut self, open: bool, now: f64, reduced: bool, tx: &mut Tx) -> bool {
        let target = if open { 1.0 } else { 0.0 };
        if target != self.target {
            self.from = self.progress;
            self.target = target;
            self.started = now;
        }
        let previous = self.progress;
        if reduced {
            self.progress = target;
        } else {
            let elapsed = ((now - self.started) / 0.2)
                .clamp(0.0, 1.0)
                .to_f32()
                .unwrap_or(1.0);
            self.progress = self.from
                + (target - self.from) * Easing::cubic(0.32, 0.72, 0.0, 1.0).eval(elapsed);
        }
        if self.progress != target {
            tx.wake_after(std::time::Duration::from_millis(8));
        }
        previous != self.progress
    }
}

pub fn fold(tx: &mut Tx) {
    tx.motion_curve(Curve::tween(0.2, Easing::cubic(0.32, 0.72, 0.0, 1.0)));
}

pub fn image_transition() -> Transition {
    Transition::opacity().springs(SpringPair::new(Spring::new(0.18, 0.0), Spring::NONE))
}

pub fn presentation_transition() -> Transition {
    Transition::offset(0.0, 8.0)
        .combined(Transition::opacity())
        .springs(SpringPair::new(
            Spring::new(0.2, 0.0),
            Spring::new(0.16, 0.0),
        ))
}
