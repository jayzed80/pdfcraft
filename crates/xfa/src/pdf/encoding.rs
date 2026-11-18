//! How one standard font's text is encoded on one page: WinAnsiEncoding, plus a `/Differences`
//! array giving a code to each accented letter of the page's text that WinAnsiEncoding has none
//! for ([`crate::text::accented`]: Romanian ă ș ț, #7526), and the `/Widths` that go with it.

use std::collections::{BTreeMap, BTreeSet};

use pdfcraft_cos::{Dict, Object};
use pdfcraft_fonts::{Std14, win_ansi_byte, win_ansi_char};

use crate::text::accented;

/// The first code `/Widths` gives a width for: the space, the first WinAnsiEncoding writes.
const FIRST_CHAR: u8 = 32;

/// The encoding of one standard font on one page.
#[derive(Debug, Default)]
pub(crate) struct FaceEncoding {
    /// The code each accented letter is written as.
    codes: BTreeMap<char, u8>,
}

impl FaceEncoding {
    /// The encoding for `texts`, every string the font draws on the page. An accented letter
    /// takes a code from 0x80 up that the page's text doesn't write in WinAnsiEncoding, so no
    /// character it uses changes; when none is left, the letter stays `?`
    /// ([`Self::first_undrawable`] says so).
    pub(crate) fn of<'a>(texts: impl IntoIterator<Item = &'a str>) -> Self {
        let mut used = [false; 256];
        let mut wanted = BTreeSet::new();
        for c in texts.into_iter().flat_map(str::chars) {
            match win_ansi_char(c) {
                Some(code) => used[usize::from(code)] = true,
                None if accented(c).is_some() => {
                    wanted.insert(c);
                }
                None => {}
            }
        }
        let mut free = (0x80..=0xFFu8).filter(|&code| !used[usize::from(code)]);
        let codes = wanted.into_iter().map_while(|c| Some((c, free.next()?))).collect();
        Self { codes }
    }

    /// `text` in this encoding: a character neither it nor WinAnsiEncoding holds is `?`, as
    /// [`pdfcraft_fonts::win_ansi`] writes it (tabs are spaces).
    pub(crate) fn encode(&self, text: &str) -> Vec<u8> {
        text.chars().map(|c| self.codes.get(&c).copied().or_else(|| win_ansi_char(c)).unwrap_or(b'?')).collect()
    }

    /// The first character of `text` that [`Self::encode`] writes as `?`. Line breaks don't count.
    pub(crate) fn first_undrawable(&self, text: &str) -> Option<char> {
        text.chars().find(|c| !matches!(c, '\n' | '\r') && !self.codes.contains_key(c) && win_ansi_char(*c).is_none())
    }

    /// The dictionary of `font` with this encoding: `/WinAnsiEncoding` when no letter needs a code
    /// of its own; otherwise WinAnsiEncoding with `/Differences` naming the letters' glyphs, and
    /// `/Widths` giving each letter its base letter's width, which the layout measured it at.
    pub(crate) fn font(&self, font: Std14) -> Object {
        let mut f = Dict::new();
        f.set(b"Type".to_vec(), Object::name("Font"));
        f.set(b"Subtype".to_vec(), Object::name("Type1"));
        f.set(b"BaseFont".to_vec(), Object::name(font.base_font()));
        if self.codes.is_empty() {
            f.set(b"Encoding".to_vec(), Object::name("WinAnsiEncoding"));
            return Object::Dict(f);
        }
        let mut by_code: Vec<(u8, char)> = self.codes.iter().map(|(&c, &code)| (code, c)).collect();
        by_code.sort_unstable();
        let mut differences = Vec::with_capacity(by_code.len() * 2);
        let mut widths: Vec<f64> = (FIRST_CHAR..=0xFF).map(|code| font.width(code)).collect();
        for &(code, c) in &by_code {
            let Some((name, base)) = accented(c) else { continue };
            differences.push(Object::Int(i64::from(code)));
            differences.push(Object::name(name));
            if let (Some(slot), Some(base)) = (widths.get_mut(usize::from(code.saturating_sub(FIRST_CHAR))), win_ansi_byte(base)) {
                *slot = font.width(base);
            }
        }
        let mut encoding = Dict::new();
        encoding.set(b"Type".to_vec(), Object::name("Encoding"));
        encoding.set(b"BaseEncoding".to_vec(), Object::name("WinAnsiEncoding"));
        encoding.set(b"Differences".to_vec(), Object::Array(differences));
        f.set(b"Encoding".to_vec(), Object::Dict(encoding));
        f.set(b"FirstChar".to_vec(), Object::Int(i64::from(FIRST_CHAR)));
        f.set(b"LastChar".to_vec(), Object::Int(255));
        f.set(b"Widths".to_vec(), Object::Array(widths.into_iter().map(|w| Object::Int(w as i64)).collect()));
        Object::Dict(f)
    }
}
