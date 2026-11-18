//! A dynamic XFA form's own Romanian text is drawn and read back as written (#7526): the letters
//! WinAnsiEncoding has no code for (ă ș ț, and ş ţ with a cedilla) were written as `?`.

use std::sync::Arc;

use pdfcraft_engine::Session;
use pdfcraft_render::{PageRenderer, PageText, RenderConfig, RenderRequest, RenderedPage, RequestKind};

const TEXT: &str = "Vă rog să confirmaţi că, la eliberarea certificatului digital ĂȘȚ șț ŞŢ ş";

/// Every accented letter the form writes with the standard fonts beyond WinAnsiEncoding.
const LETTERS: &str = "ĀāĂăĄąĆćĈĉĊċČčĎĒēĔĕĖėĘęĚěĜĝĞğĠġĢĤĥĨĪĬĮįİĴĶķĹĺĻļŃńŅņŇňŌōŎŏŐőŔŕŖŗŘřŚśŜŝŞşŢţŤŨũŪūŬŭŮůŰűŲųŴŵŶŷŹźŻżȘșȚț";

/// The first page of an XFA form drawing `text` in Helvetica and in Times bold, laid out by the
/// engine, rendered as `kind` asks.
fn rendered(text: &str, kind: RequestKind) -> RenderedPage {
    let xdp = format!(
        r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/">
<template xmlns="http://www.xfa.org/schema/xfa-template/3.3/"><subform name="form" layout="tb">
 <draw w="7in" h="0.4in"><font typeface="Helvetica" size="10pt"/><value><text>{text}</text></value></draw>
 <draw w="7in" h="0.4in"><font typeface="Times New Roman" size="10pt" weight="bold"/><value><text>{text}</text></value></draw>
</subform></template>
</xdp:xdp>"#
    );
    let mut s = Session::new();
    let id = s.open("confirmare.pdf", None, Arc::new(pdfcraft_xfa::fixtures::shell(&xdp)), None).expect("opens");
    let doc = s.get(id).expect("open");
    let mut r = PageRenderer::new(doc.bytes.clone(), RenderConfig::default());
    r.render(RenderRequest { page: 0, kind, scale: 2.0, ..Default::default() })
}

fn page_text(text: &str) -> Arc<PageText> {
    rendered(text, RequestKind::Text).text.expect("the page's text")
}

#[test]
fn romanian_letters_in_a_dynamic_form_read_back_as_written() {
    for text in [TEXT, LETTERS] {
        let plain = page_text(text).plain_text();
        assert!(!plain.contains('?'), "no letter is written as ?: {plain}");
        // The letters wrap onto a second line in each face.
        let letters = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        assert_eq!(letters(&plain), letters(&text.repeat(2)), "{plain}");
    }
}

/// Each letter advances as far as the letter it is built on, in both faces, so the text the
/// layout measured is the text the page draws, with no letter overlapping the next.
#[test]
fn romanian_letters_take_the_width_of_their_base_letter() {
    let text = page_text("aă sș tț ţ ş AĂ SȘ TȚ Ţ Ş");
    for line in 0..2 {
        let width = |c: &str| {
            let (i, _) =
                text.glyphs.iter().enumerate().find(|(i, g)| text.line_of[*i] == line && g.text == c).unwrap_or_else(|| panic!("{c} on line {line}"));
            let next = text.glyphs.get(i + 1).filter(|_| text.line_of.get(i + 1) == Some(&line)).expect("a glyph after it");
            next.rect[0] - text.glyphs[i].rect[0]
        };
        for (base, accented) in [("a", "ă"), ("s", "ș"), ("t", "ț"), ("A", "Ă"), ("S", "Ș"), ("T", "Ț")] {
            let (b, a) = (width(base), width(accented));
            assert!(b > 0.0 && (a - b).abs() < 0.01, "line {line}: {accented} advances {a}, {base} {b}");
        }
    }
}

/// The letters are drawn: each puts more ink on the page than its base letter alone, the accent.
#[test]
fn romanian_letters_are_drawn_with_their_accents() {
    let ink = |text: &str| {
        let page = rendered(text, RequestKind::Pixels);
        assert_eq!(page.error, None);
        let bytes: &[u8] = page.rgba.as_ref();
        bytes.as_chunks::<4>().0.iter().filter(|p| p[3] > 0 && p[0] < 128).count()
    };
    for (base, accented) in [("a", "ă"), ("s", "ș"), ("t", "ț"), ("s", "ş"), ("t", "ţ"), ("A", "Ă"), ("S", "Ș"), ("T", "Ț")] {
        let (b, a) = (ink(base), ink(accented));
        assert!(b > 0 && a > b, "{accented} has {a} dark pixels, {base} {b}");
    }
}
