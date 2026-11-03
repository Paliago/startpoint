//! Headless GPUI checks: real layout, hitboxes and painted interaction styles.
use super::*;
use gpui_component::menu::ContextMenuExt;
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
struct Painted {
    bounds: Bounds<Pixels>,
    fill: Option<Fill>,
}

/// Observe the same computed style that Div paints, without replacing its
/// hitboxes, event handlers or layout with a mock.
struct Probe {
    child: Stateful<Div>,
    painted: Rc<RefCell<Painted>>,
}

impl IntoElement for Probe {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Probe {
    type RequestLayoutState = <Stateful<Div> as Element>::RequestLayoutState;
    type PrepaintState = <Stateful<Div> as Element>::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        Element::id(&self.child)
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.child.request_layout(id, inspector, window, cx)
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.child
            .prepaint(id, inspector, bounds, state, window, cx)
    }
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child
            .paint(id, inspector, bounds, state, prepaint, window, cx);
        let style = self
            .child
            .interactivity()
            .compute_style(id, prepaint.as_ref(), window, cx);
        *self.painted.borrow_mut() = Painted {
            bounds,
            fill: style.background,
        };
    }
}

impl Styled for Probe {
    fn style(&mut self) -> &mut StyleRefinement {
        self.child.style()
    }
}
impl InteractiveElement for Probe {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.child.interactivity()
    }
}
impl ParentElement for Probe {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.child.extend(elements);
    }
}

struct Cards {
    selected: usize,
    list: ListState,
    painted: [Rc<RefCell<Painted>>; 2],
}

impl Render for Cards {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        list(
            self.list.clone(),
            cx.processor(|view, _, _, cx| {
                div()
                    .w_full()
                    .h(px(220.))
                    .flex()
                    .gap(px(12.))
                    .children((0..2).map(|index| {
                        let card = artwork_card_states(
                            artwork_card_frame(px(12.))
                                .id(index)
                                .flex_1()
                                .h(px(200.))
                                .flex()
                                .flex_col(),
                            view.selected == index,
                            cx.theme().colors,
                        )
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.selected = index;
                            cx.notify();
                        }));
                        div().id(("scope", index)).flex_1().min_w_0().flex().child(
                            Probe {
                                child: card,
                                painted: view.painted[index].clone(),
                            }
                            .context_menu(|menu, _, _| menu),
                        )
                    }))
                    .into_any_element()
            }),
        )
        .w(px(460.))
        .h(px(240.))
    }
}

#[::core::prelude::v1::test]
fn selected_library_cards_repaint_on_pointer_enter_leave_and_release() {
    for mode in [
        gpui_component::ThemeMode::Light,
        gpui_component::ThemeMode::Dark,
    ] {
        let mut app = TestApp::new();
        app.update(|cx| {
            gpui_component::init(cx);
            let catalog = crate::theme::Catalog::default();
            gpui_component::Theme::global_mut(cx)
                .apply_config(&catalog.configuration("Startpoint", mode));
        });
        let mut window = app.open_window(|_, _| Cards {
            selected: 1,
            list: ListState::new(1, ListAlignment::Top, px(0.)),
            painted: Default::default(),
        });
        window.draw();
        let colors = window.read(|_, cx| cx.theme().colors);
        let selected_hover = colors
            .secondary
            .alpha(1.)
            .blend(colors.secondary_hover.alpha(0.5));
        assert_eq!(selected_hover.a, 1.);
        assert_ne!(selected_hover, colors.secondary.alpha(1.));
        assert_ne!(selected_hover, colors.secondary_active.alpha(1.));
        let assert_fill = |window: &TestAppWindow<Cards>, index: usize, color: Hsla| {
            window
                .read(|view, _| assert_eq!(view.painted[index].borrow().fill, Some(color.into())));
        };
        let outside = point(px(500.), px(250.));
        window.simulate_mouse_move(outside);
        assert_fill(&window, 1, colors.secondary.alpha(1.));
        let inside = window.read(|view, _| view.painted[1].borrow().bounds.center());
        window.simulate_mouse_move(inside);
        assert_fill(&window, 1, selected_hover);
        window.simulate_mouse_down(inside, MouseButton::Left);
        assert_fill(&window, 1, colors.secondary_active.alpha(1.));
        window.simulate_mouse_up(inside, MouseButton::Left);
        assert_fill(&window, 1, selected_hover);
        window.simulate_mouse_move(outside);
        assert_fill(&window, 1, colors.secondary.alpha(1.));
        let other = window.read(|view, _| view.painted[0].borrow().bounds.center());
        window.simulate_mouse_move(other);
        assert_fill(&window, 0, colors.secondary.alpha(0.45));
        window.simulate_click(other, MouseButton::Left);
        assert_fill(&window, 0, selected_hover);
        window.simulate_mouse_move(outside);
        assert_fill(&window, 0, colors.secondary.alpha(1.));
        window.read(|view, _| assert!(view.painted[1].borrow().fill.is_none()));
    }
}

struct CaptionLayout {
    geometry: crate::library_layout::Geometry,
    lines: usize,
    recent: bool,
    frame: Rc<RefCell<Painted>>,
    artwork: Rc<RefCell<Painted>>,
    first: Rc<RefCell<Painted>>,
    last: Rc<RefCell<Painted>>,
}

impl Render for CaptionLayout {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let geometry = self.geometry;
        let title = Probe {
            child: div()
                .id("title")
                .w_full()
                .h(px((geometry.title_font_size * 1.2).ceil()))
                .flex_shrink_0()
                .text_size(px(geometry.title_font_size))
                .line_height(relative(1.2))
                .child("A caption"),
            painted: self.first.clone(),
        };
        let caption = artwork_card_caption()
            .when(self.recent, |caption| caption.gap_2())
            .when(!self.recent, |caption| {
                caption.h(px(geometry.caption_height_for(self.lines)))
            })
            .child(title)
            .children((0..self.lines).map(|index| {
                Probe {
                    child: div()
                        .id(index)
                        .w_full()
                        .h(px(geometry.metadata_height))
                        .flex_shrink_0()
                        .text_size(px(geometry.metadata_font_size))
                        .line_height(relative(1.2))
                        .child("Metadata"),
                    painted: self.last.clone(),
                }
            }));
        div().size_full().flex().items_start().child(Probe {
            child: artwork_card_frame(px(12.))
                .id("frame")
                .w(px(geometry.card_width))
                .flex()
                .flex_col()
                .gap(px(crate::library_layout::CARD_CAPTION_GAP))
                .when(!self.recent, |card| {
                    card.h(px(geometry.card_height_for(self.lines)))
                })
                .child(Probe {
                    child: div()
                        .id("artwork")
                        .w_full()
                        .h(px(geometry.artwork_height))
                        .flex_shrink_0(),
                    painted: self.artwork.clone(),
                })
                .child(caption),
            painted: self.frame.clone(),
        })
    }
}

#[::core::prelude::v1::test]
fn laid_out_captions_have_a_small_side_inset_and_keep_bottom_padding() {
    use startpoint_sdk::{AppearanceSettings, LibraryDensity, LibraryLayout};
    let mut app = TestApp::new();
    let mut window = app.open_window(|_, _| CaptionLayout {
        geometry: crate::library_layout::Geometry::new(840., &AppearanceSettings::default(), 14.),
        lines: 0,
        recent: false,
        frame: Default::default(),
        artwork: Default::default(),
        first: Default::default(),
        last: Default::default(),
    });
    for layout in [LibraryLayout::Covers, LibraryLayout::Tiles] {
        for density in [
            LibraryDensity::Small,
            LibraryDensity::Medium,
            LibraryDensity::Large,
        ] {
            for rem in [12., 14., 16., 20.] {
                for lines in 0..=2 {
                    for recent in [false, true] {
                        let settings = AppearanceSettings {
                            library_layout: layout,
                            library_density: density,
                            library_last_played: true,
                            library_sources: true,
                            ..Default::default()
                        };
                        window.update(|view, _, cx| {
                            view.geometry =
                                crate::library_layout::Geometry::new(840., &settings, rem);
                            view.lines = lines;
                            view.recent = recent;
                            cx.notify();
                        });
                        window.draw();
                        window.read(|view, _| {
                            let frame = view.frame.borrow().bounds;
                            let artwork = view.artwork.borrow().bounds;
                            let first = view.first.borrow().bounds;
                            let last = if lines == 0 {
                                first
                            } else {
                                view.last.borrow().bounds
                            };
                            // Check actual Taffy/GPUI bounds, not just height arithmetic.
                            assert_eq!(first.left(), artwork.left() + px(2.));
                            assert_eq!(first.right(), artwork.right() - px(2.));
                            assert_eq!(last.left(), first.left());
                            assert_eq!(last.right(), first.right());
                            assert!((f32::from(frame.bottom() - last.bottom()) - 12.).abs() <= 1.);
                            assert!(last.bottom() <= frame.bottom() - px(11.));
                        });
                    }
                }
            }
        }
    }
}
