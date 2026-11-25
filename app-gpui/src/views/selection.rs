//! The text selection model, independent of GPUI.
//!
//! A selection is a pair of positions, each a message index plus a UTF-8 byte
//! offset into that message's *plain* text (the text as shown, markup markers
//! removed). Offsets are logical, so right-to-left text selects the same way
//! as left-to-right text; how a logical range is painted is the renderer's job.
//! Every offset the model hands out sits on a grapheme cluster boundary.

use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

/// A point in the conversation: message index and byte offset into its text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pos {
    pub msg: usize,
    pub offset: usize,
}

impl Pos {
    pub fn new(msg: usize, offset: usize) -> Self {
        Self { msg, offset }
    }
}

/// How far a gesture grows the selection from the point it started at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Char,
    Word,
    /// The whole text of one message (triple click).
    Message,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    /// Where the gesture started; stays put while the selection is extended.
    pub anchor: Pos,
    /// The end that moves with the pointer.
    pub focus: Pos,
}

impl Selection {
    pub fn new(anchor: Pos, focus: Pos) -> Self {
        Self { anchor, focus }
    }

    #[cfg(test)]
    pub fn collapsed(at: Pos) -> Self {
        Self::new(at, at)
    }

    /// `(start, end)` in document order, whichever way it was dragged.
    pub fn ordered(&self) -> (Pos, Pos) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }

    /// Moves both ends `count` messages later: `count` older messages were
    /// put before the ones the selection refers to.
    pub fn shift(&mut self, count: usize) {
        self.anchor.msg += count;
        self.focus.msg += count;
    }

    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.focus
    }

    /// Shift-click: keep the anchor, move the focus.
    pub fn extend_to(&mut self, to: Pos) {
        self.focus = to;
    }

    /// The part of message `msg` (whose text is `len` bytes long) that is
    /// selected, or `None` when nothing in it is.
    pub fn range_in(&self, msg: usize, len: usize) -> Option<Range<usize>> {
        let (start, end) = self.ordered();
        if msg < start.msg || msg > end.msg {
            return None;
        }
        let from = if msg == start.msg { start.offset } else { 0 };
        let to = if msg == end.msg { end.offset } else { len };
        let range = from.min(len)..to.min(len);
        (range.start < range.end).then_some(range)
    }

    #[cfg(test)]
    /// The selected ranges, message by message, skipping messages where
    /// nothing is selected. `text_of` gives the plain text of a message.
    pub fn ranges<'a>(&self, text_of: impl Fn(usize) -> &'a str) -> Vec<(usize, Range<usize>)> {
        let (start, end) = self.ordered();
        (start.msg..=end.msg)
            .filter_map(|msg| {
                self.range_in(msg, text_of(msg).len())
                    .map(|range| (msg, range))
            })
            .collect()
    }
}

/// The nearest grapheme boundary at or before `offset`, clamped to the text.
pub fn snap_to_boundary(text: &str, offset: usize) -> usize {
    if offset >= text.len() {
        return text.len();
    }
    let mut last = 0;
    for (start, _) in text.grapheme_indices(true) {
        if start > offset {
            break;
        }
        last = start;
    }
    last
}

/// The word around `offset`, by Unicode word boundaries (UAX #29), so it works
/// for Latin, CJK, Arabic and Hebrew alike. A URL counts as one word. A click
/// between words picks the run of spaces or punctuation it landed on, as most
/// platforms do.
pub fn word_range(text: &str, offset: usize) -> Range<usize> {
    if text.is_empty() {
        return 0..0;
    }
    let offset = snap_to_boundary(text, offset);
    // A click at the very end belongs to the last segment.
    let probe = if offset >= text.len() {
        snap_to_boundary(text, text.len() - 1)
    } else {
        offset
    };

    if let Some(url) = url_token_around(text, probe) {
        return url;
    }
    text.split_word_bound_indices()
        .find(|(start, word)| *start <= probe && probe < start + word.len())
        .map(|(start, word)| start..start + word.len())
        .unwrap_or(0..text.len())
}

fn url_token_around(text: &str, probe: usize) -> Option<Range<usize>> {
    let start = text[..probe]
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map_or(0, |(i, c)| i + c.len_utf8());
    let end = text[probe..]
        .char_indices()
        .find(|(_, c)| c.is_whitespace())
        .map_or(text.len(), |(i, _)| probe + i);
    let token = &text[start..end];
    if token.starts_with("http://") || token.starts_with("https://") || token.starts_with("www.") {
        Some(start..end)
    } else {
        None
    }
}

/// The unit of `unit` around `offset` in `text`.
pub fn unit_range(unit: Unit, text: &str, offset: usize) -> Range<usize> {
    match unit {
        Unit::Char => {
            let at = snap_to_boundary(text, offset);
            at..at
        }
        Unit::Word => word_range(text, offset),
        Unit::Message => 0..text.len(),
    }
}

/// Grows a selection that started as the unit `initial` (a double or triple
/// click) toward `unit_at_pointer`, the unit under the pointer now: the
/// selection always contains both, and the anchor stays on the side of
/// `initial` away from the pointer.
pub fn extend_by_unit(initial: (Pos, Pos), unit_at_pointer: (Pos, Pos)) -> Selection {
    if unit_at_pointer.0 < initial.0 {
        Selection::new(initial.1, unit_at_pointer.0)
    } else {
        Selection::new(initial.0, unit_at_pointer.1.max(initial.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(msg: usize, offset: usize) -> Pos {
        Pos::new(msg, offset)
    }

    const TEXTS: [&str; 4] = ["primeira linha", "segunda", "terceira mensagem", "quarta"];

    fn text_of(msg: usize) -> &'static str {
        TEXTS[msg]
    }

    #[test]
    fn selection_orders_whichever_way_it_was_dragged() {
        let forward = Selection::new(p(1, 2), p(3, 1));
        let backward = Selection::new(p(3, 1), p(1, 2));
        assert_eq!(forward.ordered(), (p(1, 2), p(3, 1)));
        assert_eq!(backward.ordered(), (p(1, 2), p(3, 1)));
        assert!(Selection::collapsed(p(2, 5)).is_collapsed());
        assert!(!forward.is_collapsed());
    }

    #[test]
    fn positions_order_by_message_then_offset() {
        assert!(p(1, 99) < p(2, 0));
        assert!(p(2, 3) < p(2, 4));
    }

    #[test]
    fn ranges_span_whole_middle_messages() {
        let sel = Selection::new(p(3, 2), p(0, 9));
        assert_eq!(
            sel.ranges(text_of),
            vec![(0, 9..14), (1, 0..7), (2, 0..17), (3, 0..2)]
        );
        assert_eq!(sel.range_in(4, 6), None);
    }

    #[test]
    fn ranges_inside_one_message() {
        let sel = Selection::new(p(2, 4), p(2, 9));
        assert_eq!(sel.ranges(text_of), vec![(2, 4..9)]);
        assert_eq!(Selection::collapsed(p(2, 4)).ranges(text_of), vec![]);
    }

    #[test]
    fn a_selection_ending_at_a_message_start_leaves_it_out() {
        let sel = Selection::new(p(0, 5), p(1, 0));
        assert_eq!(sel.ranges(text_of), vec![(0, 5..14)]);
    }

    #[test]
    fn ranges_clamp_offsets_past_the_end() {
        let sel = Selection::new(p(0, 100), p(1, 100));
        assert_eq!(sel.ranges(text_of), vec![(1, 0..7)]);
    }

    #[test]
    fn extending_moves_only_the_focus() {
        let mut sel = Selection::new(p(1, 3), p(1, 4));
        sel.extend_to(p(0, 1));
        assert_eq!(sel.anchor, p(1, 3));
        assert_eq!(sel.focus, p(0, 1));
        assert_eq!(sel.ordered(), (p(0, 1), p(1, 3)));
    }

    #[test]
    fn snapping_never_lands_inside_a_cluster() {
        let family = "a👨‍👩‍👧‍👦b";
        let end_of_family = family.len() - 1;
        for offset in 1..end_of_family {
            assert_eq!(snap_to_boundary(family, offset), 1, "{offset}");
        }
        assert_eq!(snap_to_boundary(family, end_of_family), end_of_family);
        assert_eq!(snap_to_boundary(family, 999), family.len());

        // Skin tone modifier and regional indicator pairs stay whole.
        let tone = "👍🏽x";
        assert_eq!(snap_to_boundary(tone, 4), 0);
        let flag = "🇧🇷x";
        assert_eq!(snap_to_boundary(flag, 4), 0);
        // Combining accent.
        let accent = "e\u{301}x";
        assert_eq!(snap_to_boundary(accent, 1), 0);
    }

    #[test]
    fn words_in_portuguese() {
        let text = "não faço ideia, açúcar!";
        assert_eq!(&text[word_range(text, 1)], "não");
        assert_eq!(&text[word_range(text, 5)], "faço");
        assert_eq!(&text[word_range(text, 16)], ",");
        assert_eq!(&text[word_range(text, 19)], "açúcar");
        assert_eq!(&text[word_range(text, text.len())], "!");
        // On the space between words, the space is the "word".
        assert_eq!(&text[word_range(text, 4)], " ");
    }

    #[test]
    fn words_in_right_to_left_scripts() {
        let arabic = "مرحبا بكم في المحادثة";
        let hebrew = "שלום עולם";
        for text in [arabic, hebrew] {
            let mut covered = 0;
            let mut at = 0;
            while at < text.len() {
                let range = word_range(text, at);
                assert!(range.start <= at && at < range.end, "{text}: {at}");
                assert!(text.is_char_boundary(range.start));
                assert!(text.is_char_boundary(range.end));
                covered += range.len();
                at = range.end;
            }
            assert_eq!(covered, text.len());
        }
        assert_eq!(&hebrew[word_range(hebrew, 1)], "שלום");
    }

    #[test]
    fn words_in_cjk() {
        let text = "今日は会議があります";
        let word = word_range(text, 0);
        assert!(!word.is_empty());
        assert!(text.is_char_boundary(word.end));
    }

    #[test]
    fn an_emoji_cluster_is_one_word() {
        let text = "oi 👨‍👩‍👧‍👦 tchau";
        let start = text.find('👨').unwrap();
        let word = word_range(text, start + 5);
        assert_eq!(&text[word], "👨‍👩‍👧‍👦");
    }

    #[test]
    fn a_url_is_one_word() {
        let text = "veja https://exemplo.com.br/a/b?c=d_e agora";
        let at = text.find("exemplo").unwrap();
        assert_eq!(
            &text[word_range(text, at)],
            "https://exemplo.com.br/a/b?c=d_e"
        );
    }

    #[test]
    fn word_of_empty_text() {
        assert_eq!(word_range("", 0), 0..0);
        assert_eq!(unit_range(Unit::Message, "abc", 1), 0..3);
        assert_eq!(unit_range(Unit::Char, "a👍🏽b", 3), 1..1);
    }

    #[test]
    fn extending_by_words_keeps_the_first_word() {
        let initial = (p(1, 2), p(1, 6));
        // Dragging forward: from the start of the first word to the end of the last.
        assert_eq!(
            extend_by_unit(initial, (p(2, 0), p(2, 4))),
            Selection::new(p(1, 2), p(2, 4))
        );
        // Dragging back: anchored at the end of the first word.
        assert_eq!(
            extend_by_unit(initial, (p(0, 1), p(0, 3))),
            Selection::new(p(1, 6), p(0, 1))
        );
        // Inside the first word: just the word.
        assert_eq!(
            extend_by_unit(initial, (p(1, 3), p(1, 5))),
            Selection::new(p(1, 2), p(1, 6))
        );
    }
}
