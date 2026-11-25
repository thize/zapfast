//! Text that can be selected across bubbles.
//!
//! GPUI shapes and paints text but has no selection of its own for plain
//! elements, so each bubble's text is a [`StyledText`] wrapped in
//! [`SelectableText`]: it paints the selection as a background on the selected
//! byte range (a text-run highlight, so wrapped lines and bidirectional text
//! need no special geometry) and, once painted, records its `TextLayout` in a
//! [`Registry`] so the conversation view can map a pointer position back to a
//! byte offset with `TextLayout::index_for_position`.

use crate::markup::Run;
use gpui::{
    App, Bounds, Element, ElementId, FontStyle, FontWeight, GlobalElementId, HighlightStyle, Hsla,
    InspectorElementId, LayoutId, Pixels, SharedString, StyledText, TextLayout, UnderlineStyle,
    Window, px,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

/// A stretch of text with uniform styling, selected or not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub range: Range<usize>,
    pub bold: bool,
    pub italic: bool,
    pub link: bool,
    pub selected: bool,
}

/// Splits the markup runs at the selection boundaries.
pub fn segments(runs: &[Run], selection: Option<&Range<usize>>) -> Vec<Segment> {
    let mut out = Vec::with_capacity(runs.len() + 2);
    for run in runs {
        let mut cuts = vec![run.range.start];
        if let Some(sel) = selection {
            for cut in [sel.start, sel.end] {
                if cut > run.range.start && cut < run.range.end {
                    cuts.push(cut);
                }
            }
            cuts.sort_unstable();
        }
        cuts.push(run.range.end);
        for pair in cuts.windows(2) {
            let range = pair[0]..pair[1];
            let selected =
                selection.is_some_and(|sel| sel.start <= range.start && range.end <= sel.end);
            out.push(Segment {
                range,
                bold: run.bold,
                italic: run.italic,
                link: run.link,
                selected,
            });
        }
    }
    out
}

/// What the highlights are painted with.
#[derive(Clone, Copy)]
pub struct TextColors {
    pub link: Hsla,
    pub selection: Hsla,
}

pub fn highlights(
    runs: &[Run],
    selection: Option<&Range<usize>>,
    colors: TextColors,
) -> Vec<(Range<usize>, HighlightStyle)> {
    segments(runs, selection)
        .into_iter()
        .filter(|s| s.bold || s.italic || s.link || s.selected)
        .map(|s| {
            let style = HighlightStyle {
                font_weight: s.bold.then_some(FontWeight::BOLD),
                font_style: s.italic.then_some(FontStyle::Italic),
                color: s.link.then_some(colors.link),
                underline: s.link.then(|| UnderlineStyle {
                    thickness: px(1.),
                    color: Some(colors.link),
                    wavy: false,
                }),
                background_color: s.selected.then_some(colors.selection),
                ..Default::default()
            };
            (s.range, style)
        })
        .collect()
}

#[derive(Clone)]
struct Entry {
    layout: TextLayout,
    bounds: Bounds<Pixels>,
}

/// The text layouts of the bubbles painted last, by message index.
#[derive(Clone, Default)]
pub struct Registry(Rc<RefCell<HashMap<usize, Entry>>>);

impl Registry {
    fn record(&self, message: usize, layout: TextLayout, bounds: Bounds<Pixels>) {
        let mut map = self.0.borrow_mut();
        // Rows scrolled far away stop painting; drop their entries.
        if map.len() > 128 {
            map.retain(|_, e| (e.bounds.origin.y - bounds.origin.y).abs() < px(6000.));
        }
        map.insert(message, Entry { layout, bounds });
    }

    /// The painted messages whose text overlaps `viewport` vertically, top to
    /// bottom, with the layout and bounds of their text.
    pub fn visible(&self, viewport: Bounds<Pixels>) -> Vec<(usize, TextLayout, Bounds<Pixels>)> {
        let mut found: Vec<_> = self
            .0
            .borrow()
            .iter()
            .filter(|(_, e)| {
                e.bounds.bottom() > viewport.top() && e.bounds.top() < viewport.bottom()
            })
            .map(|(message, e)| (*message, e.layout.clone(), e.bounds))
            .collect();
        found.sort_by(|a, b| a.2.origin.y.as_f32().total_cmp(&b.2.origin.y.as_f32()));
        found
    }
}

/// The message whose text is nearest to `y`, preferring one that contains it.
pub fn nearest(
    entries: &[(usize, TextLayout, Bounds<Pixels>)],
    y: Pixels,
) -> Option<&(usize, TextLayout, Bounds<Pixels>)> {
    entries.iter().min_by(|a, b| {
        let distance = |bounds: &Bounds<Pixels>| {
            if y < bounds.top() {
                bounds.top() - y
            } else if y > bounds.bottom() {
                y - bounds.bottom()
            } else {
                px(0.)
            }
        };
        distance(&a.2).as_f32().total_cmp(&distance(&b.2).as_f32())
    })
}

/// A bubble's text: a [`StyledText`] that registers its layout once painted.
pub struct SelectableText {
    inner: StyledText,
    message: usize,
    registry: Registry,
}

impl SelectableText {
    pub fn new(
        message: usize,
        text: SharedString,
        highlights: Vec<(Range<usize>, HighlightStyle)>,
        registry: Registry,
    ) -> Self {
        Self {
            inner: StyledText::new(text).with_highlights(highlights),
            message,
            registry,
        }
    }
}

impl Element for SelectableText {
    type RequestLayoutState = <StyledText as Element>::RequestLayoutState;
    type PrepaintState = <StyledText as Element>::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.inner.request_layout(id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.inner
            .prepaint(id, inspector_id, bounds, state, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner
            .paint(id, inspector_id, bounds, state, prepaint, window, cx);
        self.registry
            .record(self.message, self.inner.layout().clone(), bounds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(range: Range<usize>, bold: bool) -> Run {
        Run {
            range,
            bold,
            italic: false,
            link: false,
        }
    }

    #[test]
    fn without_a_selection_segments_are_the_runs() {
        let runs = [run(0..3, false), run(3..8, true)];
        let segs = segments(&runs, None);
        assert_eq!(segs.len(), 2);
        assert!(segs.iter().all(|s| !s.selected));
        assert!(segs[1].bold);
    }

    #[test]
    fn the_selection_splits_runs_and_covers_exactly_its_range() {
        let runs = [run(0..4, false), run(4..10, true), run(10..12, false)];
        let sel = 2..11;
        let segs = segments(&runs, Some(&sel));
        let selected: Vec<_> = segs
            .iter()
            .filter(|s| s.selected)
            .map(|s| s.range.clone())
            .collect();
        assert_eq!(selected, [2..4, 4..10, 10..11]);
        // Still a partition of the text.
        let mut at = 0;
        for s in &segs {
            assert_eq!(s.range.start, at);
            at = s.range.end;
        }
        assert_eq!(at, 12);
        // Bold survives the split.
        assert!(
            segs.iter()
                .filter(|s| s.bold)
                .all(|s| s.range.start >= 4 && s.range.end <= 10)
        );
    }

    #[test]
    fn a_selection_inside_one_run_cuts_it_in_three() {
        let runs = [run(0..10, false)];
        let sel = 3..6;
        let ranges: Vec<_> = segments(&runs, Some(&sel))
            .into_iter()
            .map(|s| (s.range, s.selected))
            .collect();
        assert_eq!(ranges, [(0..3, false), (3..6, true), (6..10, false)]);
    }

    #[test]
    fn plain_unselected_text_needs_no_highlight() {
        let colors = TextColors {
            link: gpui::red(),
            selection: gpui::blue(),
        };
        assert!(highlights(&[run(0..5, false)], None, colors).is_empty());
        assert_eq!(
            highlights(&[run(0..5, false)], Some(&(1..2)), colors).len(),
            1
        );
    }
}
