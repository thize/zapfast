//! What Cmd+C puts on the clipboard.

use crate::views::selection::Selection;
use crate::views::timefmt;

/// What the formatter needs to know about one message.
pub struct MessageView<'a> {
    pub sender: &'a str,
    /// Local wall-clock seconds since the epoch.
    pub timestamp: i64,
    /// The plain text, as shown.
    pub text: &'a str,
}

/// Within one message: the selected text as is. Across messages: one entry per
/// message in WhatsApp's export format, `[HH:MM, D/M/YYYY] Name: text`, with
/// only the selected part of the first and last message. Messages with nothing
/// selected are left out.
pub fn format_selection<'a>(
    selection: &Selection,
    message: impl Fn(usize) -> MessageView<'a>,
) -> String {
    let parts: Vec<(MessageView<'a>, std::ops::Range<usize>)> = {
        let (start, end) = selection.ordered();
        (start.msg..=end.msg)
            .filter_map(|index| {
                let view = message(index);
                selection
                    .range_in(index, view.text.len())
                    .map(|range| (view, range))
            })
            .collect()
    };
    match parts.as_slice() {
        [] => String::new(),
        [(view, range)] => view.text[range.clone()].to_owned(),
        _ => parts
            .iter()
            .map(|(view, range)| {
                format!(
                    "[{}, {}] {}: {}",
                    timefmt::clock(view.timestamp),
                    timefmt::short_date(view.timestamp),
                    view.sender,
                    &view.text[range.clone()]
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::selection::Pos;

    fn at(h: i64, m: i64) -> i64 {
        timefmt::days_from_civil(2026, 3, 5) * timefmt::SECONDS_PER_DAY + h * 3600 + m * 60
    }

    fn message(index: usize) -> MessageView<'static> {
        match index {
            0 => MessageView {
                sender: "Ana",
                timestamp: at(9, 5),
                text: "bom dia, tudo bem?",
            },
            1 => MessageView {
                sender: "Você",
                timestamp: at(9, 6),
                text: "tudo ótimo!\nE você?",
            },
            2 => MessageView {
                sender: "Bruno",
                timestamp: at(23, 59),
                text: "também 🇧🇷",
            },
            _ => unreachable!(),
        }
    }

    #[test]
    fn one_message_copies_the_plain_selection() {
        let sel = Selection::new(Pos::new(0, 4), Pos::new(0, 8));
        assert_eq!(format_selection(&sel, message), "dia,");
    }

    #[test]
    fn several_messages_copy_one_line_each() {
        let sel = Selection::new(Pos::new(0, 4), Pos::new(2, 7));
        assert_eq!(
            format_selection(&sel, message),
            "[09:05, 5/3/2026] Ana: dia, tudo bem?\n\
             [09:06, 5/3/2026] Você: tudo ótimo!\nE você?\n\
             [23:59, 5/3/2026] Bruno: também"
        );
    }

    #[test]
    fn dragging_backward_copies_the_same_text() {
        let forward = Selection::new(Pos::new(0, 4), Pos::new(2, 7));
        let backward = Selection::new(Pos::new(2, 7), Pos::new(0, 4));
        assert_eq!(
            format_selection(&forward, message),
            format_selection(&backward, message)
        );
    }

    #[test]
    fn a_message_touched_only_at_its_edge_is_left_out() {
        // Ends exactly at the start of message 2: nothing of it is selected,
        // so the result is a single message and copies as plain text.
        let sel = Selection::new(Pos::new(1, 0), Pos::new(2, 0));
        assert_eq!(format_selection(&sel, message), "tudo ótimo!\nE você?");
    }

    #[test]
    fn a_collapsed_selection_copies_nothing() {
        let sel = Selection::collapsed(Pos::new(1, 3));
        assert_eq!(format_selection(&sel, message), "");
    }
}
