//! Shared typography and pause-aware time for artwork scenes.
use std::time::{Duration, Instant};

pub(crate) fn tracked(text: &str) -> String {
    if !text.is_ascii() {
        return text.to_owned();
    }
    text.chars()
        .map(|ch| ch.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Pause-aware local time for artwork materials.
#[derive(Default)]
pub(crate) struct FlowClock {
    elapsed: Duration,
    tick: Option<Instant>,
}

impl FlowClock {
    pub fn update(&mut self, running: bool) {
        self.update_at(running, Instant::now());
    }

    fn update_at(&mut self, running: bool, now: Instant) {
        if let Some(tick) = self.tick {
            self.elapsed += now
                .saturating_duration_since(tick)
                .min(Duration::from_millis(100));
        }
        self.tick = running.then_some(now);
    }

    pub fn seconds(&self) -> f32 {
        self.elapsed.as_secs_f32()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracking_uses_supported_spaces_without_breaking_complex_titles() {
        assert_eq!(tracked("Hades II"), "H a d e s   I I");
        assert_eq!(tracked("Café — 世界"), "Café — 世界");
        assert_eq!(tracked("e\u{301}"), "e\u{301}");
    }

    #[test]
    fn flow_freezes_and_resumes_without_skipping_to_a_wall_clock_phase() {
        let mut clock = FlowClock::default();
        let start = Instant::now();
        clock.update_at(true, start);
        clock.update_at(false, start + Duration::from_millis(50));
        let paused = clock.seconds();
        assert!(paused > 0.);
        clock.update_at(false, start + Duration::from_secs(20));
        clock.update_at(true, start + Duration::from_secs(30));
        assert_eq!(clock.seconds(), paused);
        clock.update_at(true, start + Duration::from_millis(30_050));
        assert!((clock.seconds() - paused - 0.05).abs() < 0.0001);
    }
}
