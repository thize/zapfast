//! WhatsApp's inline markup: `*bold*` and `_italic_`, plus link detection.
//!
//! The parser removes the markers and describes the result as runs that
//! partition the plain text, so a renderer (and the selection model) only ever
//! deals with the visible text.

use std::ops::Range;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Run {
    pub range: Range<usize>,
    pub bold: bool,
    pub italic: bool,
    pub link: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    /// The text as shown: markers removed.
    pub plain: String,
    /// Runs that cover `plain` end to end, without gaps or overlaps.
    pub runs: Vec<Run>,
}

#[derive(Clone, Copy, Default)]
struct Flags {
    bold: bool,
    italic: bool,
}

#[derive(Default)]
struct Builder {
    plain: String,
    runs: Vec<Run>,
}

impl Builder {
    fn push(&mut self, c: char, flags: Flags) {
        let start = self.plain.len();
        self.plain.push(c);
        let end = self.plain.len();
        match self.runs.last_mut() {
            Some(run) if run.bold == flags.bold && run.italic == flags.italic => {
                run.range.end = end;
            }
            _ => self.runs.push(Run {
                range: start..end,
                bold: flags.bold,
                italic: flags.italic,
                link: false,
            }),
        }
    }
}

pub fn parse(raw: &str) -> Parsed {
    let chars: Vec<char> = raw.chars().collect();
    let mut builder = Builder::default();
    walk(&chars, 0..chars.len(), Flags::default(), &mut builder);
    let urls = url_ranges(&builder.plain);
    let runs = split_runs(builder.runs, &urls);
    Parsed {
        plain: builder.plain,
        runs,
    }
}

fn walk(chars: &[char], range: Range<usize>, flags: Flags, out: &mut Builder) {
    let mut i = range.start;
    while i < range.end {
        let c = chars[i];
        if (c == '*' || c == '_')
            && opens(chars, i, range.end)
            && let Some(j) = find_close(chars, i, range.end)
        {
            let inner = if c == '*' {
                Flags {
                    bold: true,
                    ..flags
                }
            } else {
                Flags {
                    italic: true,
                    ..flags
                }
            };
            walk(chars, i + 1..j, inner, out);
            i = j + 1;
            continue;
        }
        out.push(c, flags);
        i += 1;
    }
}

/// An opening marker follows a boundary and precedes a non-space character.
fn opens(chars: &[char], i: usize, end: usize) -> bool {
    let before_ok = i == 0 || !chars[i - 1].is_alphanumeric();
    let after_ok = i + 1 < end && !chars[i + 1].is_whitespace() && chars[i + 1] != chars[i];
    before_ok && after_ok
}

/// A closing marker follows a non-space character and precedes a boundary;
/// markup never spans a line break.
fn find_close(chars: &[char], open: usize, end: usize) -> Option<usize> {
    let marker = chars[open];
    (open + 2..end)
        .take_while(|&j| chars[j - 1] != '\n')
        .find(|&j| {
            chars[j] == marker
                && !chars[j - 1].is_whitespace()
                && (j + 1 == chars.len() || !chars[j + 1].is_alphanumeric())
        })
}

/// Byte ranges of `http(s)://` and `www.` tokens, without trailing punctuation.
pub fn url_ranges(text: &str) -> Vec<Range<usize>> {
    let mut urls = Vec::new();
    let mut start = None;
    let push = |start: usize, end: usize, urls: &mut Vec<Range<usize>>| {
        let token = &text[start..end];
        if token.starts_with("http://")
            || token.starts_with("https://")
            || token.starts_with("www.")
        {
            let trimmed = token.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '"']);
            if !trimmed.is_empty() {
                urls.push(start..start + trimmed.len());
            }
        }
    };
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                push(s, i, &mut urls);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        push(s, text.len(), &mut urls);
    }
    urls
}

fn split_runs(runs: Vec<Run>, urls: &[Range<usize>]) -> Vec<Run> {
    if urls.is_empty() {
        return runs;
    }
    let mut out = Vec::with_capacity(runs.len() + urls.len() * 2);
    for run in runs {
        let mut at = run.range.start;
        for url in urls {
            let start = url.start.max(at);
            let end = url.end.min(run.range.end);
            if start >= end {
                continue;
            }
            if at < start {
                out.push(Run {
                    range: at..start,
                    link: false,
                    ..run.clone()
                });
            }
            out.push(Run {
                range: start..end,
                link: true,
                ..run.clone()
            });
            at = end;
        }
        if at < run.range.end {
            out.push(Run {
                range: at..run.range.end,
                link: false,
                ..run
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(parsed: &Parsed) -> Vec<(&str, bool, bool, bool)> {
        parsed
            .runs
            .iter()
            .map(|r| (&parsed.plain[r.range.clone()], r.bold, r.italic, r.link))
            .collect()
    }

    #[test]
    fn bold_and_italic_lose_their_markers() {
        let p = parse("um *negrito* e _itálico_ ok");
        assert_eq!(p.plain, "um negrito e itálico ok");
        assert_eq!(
            flags(&p),
            [
                ("um ", false, false, false),
                ("negrito", true, false, false),
                (" e ", false, false, false),
                ("itálico", false, true, false),
                (" ok", false, false, false),
            ]
        );
    }

    #[test]
    fn markers_nest() {
        let p = parse("*_ambos_* e _*também*_");
        assert_eq!(p.plain, "ambos e também");
        assert_eq!(
            flags(&p),
            [
                ("ambos", true, true, false),
                (" e ", false, false, false),
                ("também", true, true, false),
            ]
        );
    }

    #[test]
    fn unmatched_and_word_internal_markers_stay_literal() {
        for raw in [
            "2 * 3 = 6",
            "snake_case_name",
            "*sem fechar",
            "_ tudo _",
            "**",
            "a*b*c",
        ] {
            let p = parse(raw);
            assert_eq!(p.plain, raw, "{raw}");
            assert!(p.runs.iter().all(|r| !r.bold && !r.italic), "{raw}");
        }
    }

    #[test]
    fn markup_does_not_cross_lines() {
        let p = parse("*um\ndois*");
        assert_eq!(p.plain, "*um\ndois*");
    }

    #[test]
    fn runs_partition_the_plain_text() {
        for raw in [
            "",
            "texto *forte* https://exemplo.com.br/a_b_c/d?x=*1* fim.",
            "ação ç ✓ 👨‍👩‍👧‍👦 _é_ 🇧🇷",
            "مرحبا *بكم* שלום",
        ] {
            let p = parse(raw);
            let mut at = 0;
            for run in &p.runs {
                assert_eq!(run.range.start, at);
                assert!(p.plain.is_char_boundary(run.range.end));
                at = run.range.end;
            }
            assert_eq!(at, p.plain.len());
        }
    }

    #[test]
    fn urls_become_links_without_trailing_punctuation() {
        let p = parse("veja https://exemplo.com.br/a_b_c, ou www.x.com.");
        let links: Vec<_> = p
            .runs
            .iter()
            .filter(|r| r.link)
            .map(|r| &p.plain[r.range.clone()])
            .collect();
        assert_eq!(links, ["https://exemplo.com.br/a_b_c", "www.x.com"]);
        assert!(p.runs.iter().all(|r| !r.italic));
    }
}
