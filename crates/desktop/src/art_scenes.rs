//! Pure geometry and pause-aware, monotonic motion for Wall, Reel and Art Book.
pub(crate) mod composition;
pub(crate) mod material;
use startpoint_sdk::{Game, GameId};
use std::time::{Duration, Instant};
use std::{collections::BTreeMap, sync::Arc};

pub(crate) struct Games<'a> {
    source: &'a [Game],
    order: &'a [usize],
}
impl Games<'_> {
    pub fn len(&self) -> usize {
        self.order.len()
    }
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}
impl std::ops::Index<usize> for Games<'_> {
    type Output = Game;
    fn index(&self, index: usize) -> &Game {
        &self.source[self.order[index]]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Wall,
    Reel,
    Book,
    Diagonal,
    Mosaic,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Rect {
    pub fn mix(self, to: Self, t: f32) -> Self {
        Self {
            x: self.x + (to.x - self.x) * t,
            y: self.y + (to.y - self.y) * t,
            w: self.w + (to.w - self.w) * t,
            h: self.h + (to.h - self.h) * t,
        }
    }
}

#[derive(Default)]
pub(crate) struct Tween {
    from: f32,
    pub target: f32,
    started: Option<Instant>,
    duration: Duration,
    paused: Option<Instant>,
}
impl Tween {
    pub fn value(&self, now: Instant) -> f32 {
        let now = self.paused.unwrap_or(now);
        let t = self
            .started
            .map_or(1., |at| {
                now.saturating_duration_since(at).as_secs_f32()
                    / self.duration.as_secs_f32().max(0.001)
            })
            .clamp(0., 1.);
        let t = t * t * (3. - 2. * t);
        self.from + (self.target - self.from) * t
    }
    pub fn set(&mut self, to: f32, duration: Duration, reduced: bool, now: Instant) {
        self.from = self.value(now);
        self.target = to;
        self.duration = duration;
        self.started =
            (!reduced && (self.from - to).abs() > 0.0001).then_some(self.paused.unwrap_or(now));
        if reduced {
            self.from = to;
        }
    }
    pub fn snap(&mut self, to: f32) {
        self.from = to;
        self.target = to;
        self.started = None;
    }
    pub fn active(&self, now: Instant) -> bool {
        self.paused.is_none()
            && self
                .started
                .is_some_and(|at| now.saturating_duration_since(at) < self.duration)
    }
    pub fn pause(&mut self, paused: bool, now: Instant) {
        match (paused, self.paused) {
            (true, None) => self.paused = Some(now),
            (false, Some(at)) => {
                if let Some(start) = &mut self.started {
                    *start += now.saturating_duration_since(at);
                }
                self.paused = None;
            }
            _ => {}
        }
    }
}

pub(crate) struct Focus {
    pub game: Game,
    pub origin: Rect,
    pub amount: Tween,
    pub hero: Tween,
    pub launch: bool,
    pub dispatched: bool,
}

#[derive(Default)]
pub(crate) struct State {
    source: Option<Arc<Vec<Game>>>,
    order: Vec<usize>,
    positions: BTreeMap<GameId, usize>,
    pub viewport: [f32; 2],
    pub selected: Option<GameId>,
    pub previous: Option<Game>,
    pub current: Option<Game>,
    pub count: usize,
    pub index: Tween,
    pub page: Tween,
    pub book_direction: f32,
    pub composition_pointer: Option<GameId>,
    pub scroll: Tween,
    pub hover: Tween,
    hover_exit: Tween,
    pub hovered: Option<GameId>,
    pub old_hover: Option<GameId>,
    hover_from: f32,
    old_hover_from: f32,
    pub overview: Tween,
    pub underline: Tween,
    pub focus: Option<Focus>,
    pub ambient: crate::cinematic::Scene,
    pub drift: crate::artwork_motion::FlowClock,
}
impl State {
    pub fn refresh_games(&mut self, source: &Arc<Vec<Game>>) {
        if self
            .source
            .as_ref()
            .is_some_and(|old| Arc::ptr_eq(old, source))
        {
            return;
        }
        self.order = (0..source.len())
            .filter(|i| source[*i].available && !source[*i].hidden)
            .collect();
        self.order
            .sort_by_key(|i| std::cmp::Reverse(source[*i].last_launched.unwrap_or(0)));
        self.positions = self
            .order
            .iter()
            .enumerate()
            .map(|(index, i)| (source[*i].id.clone(), index))
            .collect();
        self.source = Some(source.clone());
    }
    pub fn games(&self) -> Games<'_> {
        Games {
            source: self.source.as_ref().map_or(&[], |s| s.as_slice()),
            order: &self.order,
        }
    }
    pub fn position(&self, id: &GameId) -> Option<usize> {
        self.positions.get(id).copied()
    }
    pub fn select(
        &mut self,
        game: &Game,
        index: usize,
        count: usize,
        mode: Mode,
        reduced: bool,
        now: Instant,
    ) {
        if self.selected.as_ref() == Some(&game.id) && self.count == count {
            // A launch or refreshed snapshot can reorder an unchanged-size
            // collection. Keep the same game centered, not its old slot.
            if (self.index.target.rem_euclid(count.max(1) as f32) - index as f32).abs() > 0.001 {
                self.index.snap(index as f32);
            }
            if self.current.as_ref().is_some_and(|current| {
                current.artwork != game.artwork || current.title() != game.title()
            }) {
                self.current = Some(game.clone());
            }
            return;
        }
        let initial = self.selected.is_none() || self.count != count;
        let reverse_page = mode == Mode::Book
            && self
                .previous
                .as_ref()
                .is_some_and(|previous| previous.id == game.id)
            && self.page.active(now);
        let page_start = if reverse_page {
            1. - self.page.value(now)
        } else {
            0.
        };
        let direction = crate::gallery::relative(index, self.index.target, count).signum();
        self.book_direction = if reverse_page {
            -self.book_direction
        } else {
            direction
        };
        self.previous = self.current.take();
        self.current = Some(game.clone());
        self.selected = Some(game.id.clone());
        self.count = count;
        if initial {
            self.index.snap(index as f32);
            self.page.snap(1.);
        } else {
            let position = self.index.value(now);
            let delta = crate::gallery::relative(index, position, count);
            if matches!(mode, Mode::Diagonal | Mode::Mosaic) {
                self.index.snap(index as f32);
                self.page.snap(1.);
                return;
            } else if mode == Mode::Book {
                self.index.snap(index as f32);
            } else {
                self.index
                    .set(position + delta, Duration::from_millis(500), reduced, now);
            }
            self.page.snap(page_start);
            self.page.set(
                1.,
                Duration::from_millis(if mode == Mode::Book { 280 } else { 500 }),
                reduced,
                now,
            );
        }
    }
    pub fn hover(&mut self, id: Option<GameId>, reduced: bool, now: Instant) {
        if self.hovered == id {
            return;
        }
        let new_from = id.as_ref().map_or(0., |id| self.emphasis(id, now));
        let old_from = self
            .hovered
            .as_ref()
            .map_or(0., |id| self.emphasis(id, now));
        self.old_hover = self.hovered.take();
        self.hovered = id;
        self.hover_from = new_from;
        self.old_hover_from = old_from;
        self.hover.snap(0.);
        if self.hovered.is_some() {
            self.hover.set(
                1.,
                Duration::from_millis(if reduced { 1 } else { 160 }),
                false,
                now + Duration::from_millis(180),
            );
        }
        self.hover_exit.snap(0.);
        self.hover_exit
            .set(1., Duration::from_millis(160), reduced, now);
    }
    pub fn emphasis(&self, id: &GameId, now: Instant) -> f32 {
        if self.hovered.as_ref() == Some(id) {
            self.hover_from + (1. - self.hover_from) * self.hover.value(now)
        } else if self.old_hover.as_ref() == Some(id) {
            self.old_hover_from * (1. - self.hover_exit.value(now))
        } else {
            0.
        }
    }
    pub fn surrounding_shade(&self, id: &GameId, now: Instant) -> f32 {
        let strength = self
            .hovered
            .iter()
            .chain(&self.old_hover)
            .map(|id| self.emphasis(id, now))
            .fold(0., f32::max);
        wall_shade((strength - self.emphasis(id, now)).max(0.))
    }
    pub fn pause(&mut self, paused: bool, now: Instant) {
        for tween in [
            &mut self.index,
            &mut self.page,
            &mut self.scroll,
            &mut self.hover,
            &mut self.hover_exit,
            &mut self.overview,
            &mut self.underline,
        ] {
            tween.pause(paused, now);
        }
        if let Some(focus) = &mut self.focus {
            focus.amount.pause(paused, now);
            focus.hero.pause(paused, now);
        }
        self.ambient.set_paused(paused, now);
        if paused {
            self.drift.update(false);
        }
    }
    pub fn active(&self, now: Instant) -> bool {
        [
            &self.index,
            &self.page,
            &self.scroll,
            &self.hover,
            &self.hover_exit,
            &self.overview,
            &self.underline,
        ]
        .iter()
        .any(|t| t.active(now))
            || self
                .focus
                .as_ref()
                .is_some_and(|focus| focus.amount.active(now) || focus.hero.active(now))
            || self.ambient.animating(now)
    }
    pub fn open(&mut self, game: Game, origin: Rect, launch: bool, reduced: bool, now: Instant) {
        if self
            .focus
            .as_ref()
            .is_some_and(|focus| focus.game.id == game.id)
        {
            let focus = self.focus.as_mut().unwrap();
            focus.launch |= launch;
            focus
                .amount
                .set(1., Duration::from_millis(420), reduced, now);
            return;
        }
        let mut amount = Tween::default();
        amount.set(1., Duration::from_millis(420), reduced, now);
        self.focus = Some(Focus {
            game,
            origin,
            amount,
            hero: Tween::default(),
            launch,
            dispatched: false,
        });
    }
    pub fn dismiss(&mut self, reduced: bool, now: Instant) -> bool {
        if let Some(focus) = &mut self.focus {
            focus.launch = false;
            focus
                .amount
                .set(0., Duration::from_millis(420), reduced, now);
            return true;
        }
        if self.overview.target > 0. {
            self.overview
                .set(0., Duration::from_millis(400), reduced, now);
            return true;
        }
        false
    }
}

#[derive(Clone, Copy)]
pub(crate) struct WallLayout {
    pub height: f32,
    pub columns: usize,
    pub cell_w: f32,
    pub cell_h: f32,
    pub gap: f32,
    pub stride: f32,
}
impl WallLayout {
    pub fn new(width: f32, height: f32) -> Self {
        let gap = 10.;
        let columns = ((width - 10.) / 170.).floor().max(2.) as usize;
        let cell_w = ((width - gap * (columns + 1) as f32) / columns as f32).max(1.);
        let cell_h = cell_w * 1.5;
        Self {
            height,
            columns,
            cell_w,
            cell_h,
            gap,
            stride: cell_h + gap,
        }
    }
    pub fn max_scroll(&self, count: usize) -> f32 {
        ((count.div_ceil(self.columns) as f32 * self.stride + self.gap) - self.height).max(0.)
    }
    pub fn rect(&self, index: usize, scroll: f32) -> Rect {
        Rect {
            x: self.gap + (index % self.columns) as f32 * (self.cell_w + self.gap),
            y: self.gap + (index / self.columns) as f32 * self.stride - scroll,
            w: self.cell_w,
            h: self.cell_h,
        }
    }
    pub fn visible(&self, scroll: f32, count: usize) -> std::ops::Range<usize> {
        let start = (scroll / self.stride).floor().max(0.) as usize;
        let end = ((scroll + self.height) / self.stride).ceil() as usize + 1;
        (start.saturating_sub(1) * self.columns).min(count)..(end * self.columns).min(count)
    }
}
pub(crate) fn reel_rect(width: f32, height: f32, offset: f32) -> Rect {
    let focus = (1. - offset.abs()).clamp(0., 1.);
    let w = width * 0.71;
    let h = (w * 0.43).min(height * 0.56);
    let scale = 0.97 + focus * 0.03;
    Rect {
        x: width / 2. + offset * (w + 3.) - w * scale / 2.,
        y: height * 0.49 - h / 2.,
        w: w * scale,
        h,
    }
}
pub(crate) fn text_phase(progress: f32) -> (f32, f32) {
    (
        (1. - progress / 0.26).clamp(0., 1.),
        ((progress - 0.72) / 0.28).clamp(0., 1.),
    )
}

pub(crate) fn wall_shade(emphasis: f32) -> f32 {
    emphasis.clamp(0., 1.) * 0.20
}

pub(crate) fn book_page_positions(width: f32, progress: f32, direction: f32) -> [f32; 2] {
    [
        -direction * width * progress,
        direction * width * (1. - progress),
    ]
}

pub(crate) fn hero_mix(expansion: f32, decoded: f32) -> f32 {
    let t = ((expansion - 0.12) / 0.70).clamp(0., 1.);
    t * t * (3. - 2. * t) * decoded
}

pub(crate) struct Paper;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PaperLayer {
    Sheet,
    Edge,
    Grain,
}
impl gpui::Asset for Paper {
    type Source = PaperLayer;
    type Output = std::sync::Arc<gpui::RenderImage>;
    fn load(
        layer: PaperLayer,
        _: &mut gpui::App,
    ) -> impl std::future::Future<Output = Self::Output> + Send + 'static {
        async move {
            let pixels = image::RgbaImage::from_fn(1536, 1024, |x, y| {
                let [r, g, b, a] = material::paper_pixel(x, y, layer);
                let premultiply =
                    |channel| u8::try_from(u32::from(channel) * u32::from(a) / 255).unwrap_or(255);
                image::Rgba([premultiply(b), premultiply(g), premultiply(r), a])
            });
            std::sync::Arc::new(gpui::RenderImage::new([image::Frame::new(pixels)]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn motion_is_monotonic_reversible_and_pause_aware() {
        let now = Instant::now();
        let mut t = Tween::default();
        t.set(1., Duration::from_millis(500), false, now);
        let mid = now + Duration::from_millis(250);
        assert_eq!(t.value(mid), 0.5);
        t.set(0., Duration::from_millis(420), false, mid);
        assert_eq!(t.value(mid), 0.5);
        t.pause(true, mid);
        let later = mid + Duration::from_secs(500);
        assert_eq!(t.value(later), 0.5);
        assert!(!t.active(later));
        t.pause(false, later);
        assert_eq!(t.value(later), 0.5);
        assert_eq!(t.value(later + Duration::from_millis(420)), 0.);
        t.set(1., Duration::from_millis(500), true, later);
        assert_eq!(t.value(later), 1.);
        assert!(!t.active(later));
    }
    #[test]
    fn wall_is_viewport_bounded_and_anchor_survives_scroll() {
        for (w, h) in [(960., 620.), (1280., 820.), (2560., 1440.)] {
            let layout = WallLayout::new(w, h);
            let count = 100_000;
            let scroll = 12_345.;
            assert!(layout.visible(scroll, count).len() < 150);
            let i = layout.visible(scroll, count).start + layout.columns;
            let rect = layout.rect(i, scroll);
            assert_eq!(rect, layout.rect(i, scroll));
            assert!(rect.x >= 10. && rect.x + rect.w <= w);
            assert!(layout.max_scroll(count) > scroll);
        }
    }
    #[test]
    fn reel_centers_exactly_and_text_stages_do_not_overlap() {
        let center = reel_rect(1280., 820., 0.);
        assert!((center.x + center.w / 2. - 640.).abs() < 0.01);
        assert!(center.y > 100.);
        assert!(center.h < 820. * 0.56);
        assert!((center.w / center.h - 1. / 0.43).abs() < 0.01);
        assert_eq!(WallLayout::new(1280., 820.).rect(0, 0.).y, 10.);
        for i in 0..100 {
            let (old, new) = text_phase(i as f32 / 99.);
            assert!(old == 0. || new == 0.);
        }
    }
    #[test]
    fn wall_shading_waits_for_dwell_and_cancels_when_the_pointer_leaves() {
        let now = Instant::now();
        let a = GameId("a".into());
        let b = GameId("b".into());
        let mut state = State::default();
        assert_eq!(wall_shade(state.emphasis(&a, now)), 0.);
        state.hover(Some(a.clone()), false, now);
        let mid = now + Duration::from_millis(75);
        assert_eq!(state.emphasis(&a, mid), 0.);
        assert_eq!(state.surrounding_shade(&b, mid), 0.);
        let shade = wall_shade(state.emphasis(&a, mid));
        state.hover(None, false, mid);
        assert!((wall_shade(state.emphasis(&a, mid)) - shade).abs() < 0.00001);
        assert_eq!(
            wall_shade(state.emphasis(&a, mid + Duration::from_secs(1))),
            0.
        );
        state.hover(Some(a.clone()), false, mid);
        let settled = mid + Duration::from_millis(500);
        assert_eq!(state.emphasis(&a, settled), 1.);
        assert_eq!(state.surrounding_shade(&a, settled), 0.);
        assert!(state.surrounding_shade(&b, settled) > 0.);
        state.hover(None, false, settled);
        assert_eq!(state.emphasis(&a, settled), 1.);
        assert_eq!(state.emphasis(&a, settled + Duration::from_millis(160)), 0.);
        assert_eq!(
            state.surrounding_shade(&b, settled + Duration::from_millis(160)),
            0.
        );
        assert!(wall_shade(1.) > 0.);
        assert_eq!(hero_mix(0., 1.), 0.);
        assert_eq!(hero_mix(1., 1.), 1.);
        assert_eq!(hero_mix(1., 0.), 0.);
    }
}
