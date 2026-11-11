//! A batch of form values shares one glyph pool (#6943): filling a large form with one
//! `form_fill` (one batch of value edits) reads the form's glyph procedures once, not once per
//! value, and stores what it would have stored filling the values one at a time.

use std::sync::Arc;

use pdfcraft_engine::{Edit, Session};
use pdfcraft_forms::FieldValue;

/// A PDF of `objs`, numbered from 1, with a cross-reference table.
fn pdf_of(objs: &[String]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

/// A form of `n` text fields on one page, each with its own name and widget.
fn many_text_fields(n: usize) -> Vec<u8> {
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", i + 4)).collect();
    let mut objs = vec![
        format!("<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [{}] /DA (/Helv 10 Tf 0 g) >> >>", kids.join(" ")),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Annots [{}] >>", kids.join(" ")),
    ];
    for i in 0..n {
        let y = (i % 70) * 11;
        objs.push(format!("<< /Type /Annot /Subtype /Widget /FT /Tx /T (f{i}) /Rect [50 {y} 300 {top}] /P 3 0 R >>", top = y + 10));
    }
    pdf_of(&objs)
}

/// Greek for field `i`: text the standard fonts can't draw, so its appearance builds Type 3 fonts.
fn greek(i: usize) -> FieldValue {
    FieldValue::Text(format!("Καλημέρα κόσμε {i}"))
}

fn fill_all(n: usize) -> Edit {
    Edit::Batch { label: "Fill in form".into(), edits: (0..n).map(|i| Edit::SetFieldValue { name: format!("f{i}"), value: greek(i) }).collect() }
}

/// `edits` applied to the form of `n` fields as one batch, or one apply each: the saved file and
/// how many times the form was read for its glyph procedures.
fn fill(n: usize, edits: Vec<Edit>, batched: bool) -> (Vec<u8>, usize) {
    // A fixed clock: the saved `/ModDate` would differ when the two fills straddle a second.
    let mut s = Session::new().with_clock(|| 1_800_000_000);
    let id = s.open("form.pdf", None, Arc::new(many_text_fields(n)), None).expect("opens");
    let before = pdfcraft_forms::glyph_pool_seeds();
    if batched {
        s.apply(id, Edit::Batch { label: "Fill in form".into(), edits }).expect("fills");
    } else {
        for e in edits {
            s.apply(id, e).expect("fills");
        }
    }
    let seeds = pdfcraft_forms::glyph_pool_seeds() - before;
    (s.save_bytes(id).expect("saves").to_vec(), seeds)
}

/// The batch saved the same bytes as one apply per edit; if not, where they first differ.
fn assert_same_file(batched: &[u8], one_by_one: &[u8]) {
    if batched != one_by_one {
        let at = batched.iter().zip(one_by_one).take_while(|(a, b)| a == b).count();
        let around = |b: &[u8]| String::from_utf8_lossy(b.get(at.saturating_sub(80)..(at + 80).min(b.len())).unwrap_or_default()).into_owned();
        panic!("the files differ at byte {at}:\nbatched: {}\none by one: {}", around(batched), around(one_by_one));
    }
}

/// One batch of value edits reads the form for glyph procedures once, however many values it
/// draws, and saves the same file, byte for byte, as filling the values one apply each, which
/// reads it once per value. The timing check is the count, not the clock.
#[test]
fn a_batch_of_values_reads_the_form_for_glyphs_once_and_saves_what_one_by_one_saves() {
    let n = 40;
    let edits = || (0..n).map(|i| Edit::SetFieldValue { name: format!("f{i}"), value: greek(i) }).collect::<Vec<_>>();
    let (batched, batch_seeds) = fill(n, edits(), true);
    let (one_by_one, single_seeds) = fill(n, edits(), false);
    assert_eq!(batch_seeds, 1, "one reading for the batch");
    assert_eq!(single_seeds, n, "one reading per value, one apply each");
    assert_same_file(&batched, &one_by_one);
    // Each glyph procedure is stored once: a glyph stream starts `… d1`.
    let doc = pdfcraft_cos::Document::open(Arc::new(batched)).expect("reopens");
    let mut glyphs = std::collections::HashSet::new();
    let mut count = 0;
    for (_, o) in doc.scan_objects() {
        let pdfcraft_cos::Object::Stream(s) = &*o else { continue };
        let Ok(data) = s.decoded() else { continue };
        if data.split(|&b| b == b'\n').next().is_some_and(|l| l.ends_with(b" d1")) {
            count += 1;
            glyphs.insert(data);
        }
    }
    assert!(count > 0, "the Greek is drawn with Type 3 glyphs");
    assert_eq!(count, glyphs.len(), "no glyph procedure is stored twice");
}

/// A batch that changes a field's font between its value edits fills as one apply per edit does:
/// the font change ends the shared pool, and the next value edit reads the form again.
#[test]
fn a_batch_that_changes_a_font_between_values_saves_what_one_by_one_saves() {
    use pdfcraft_forms::{FieldFont, FieldProps, LookPatch};
    let times =
        Box::new(FieldProps { appearance: Some(LookPatch { font: Some(FieldFont::Times), ..LookPatch::default() }), ..FieldProps::default() });
    let edits = || {
        vec![
            Edit::SetFieldValue { name: "f0".into(), value: greek(0) },
            Edit::SetFieldValue { name: "f1".into(), value: greek(1) },
            Edit::SetFieldProps { name: "f1".into(), props: times.clone() },
            Edit::SetFieldProps { name: "f2".into(), props: times.clone() },
            Edit::SetFieldValue { name: "f2".into(), value: greek(2) },
            Edit::SetFieldValue { name: "f1".into(), value: FieldValue::Text("Γειά σου".into()) },
            Edit::Batch { label: "inner".into(), edits: vec![Edit::SetFieldValue { name: "f3".into(), value: greek(3) }] },
            Edit::SetFieldValue { name: "f4".into(), value: greek(4) },
        ]
    };
    let (batched, batch_seeds) = fill(6, edits(), true);
    let (one_by_one, _) = fill(6, edits(), false);
    // f0–f1, the font change (redraws f1: one), f2 and f1 again, the inner batch, f4.
    assert_eq!(batch_seeds, 5, "a new pool after each edit that isn't a value");
    assert_same_file(&batched, &one_by_one);
}

/// How long one batch of value edits (what `form_fill` sends) takes on a large form:
/// `cargo test --release -p pdfcraft-engine --test form_fill_glyph_pool -- --ignored --nocapture`.
#[test]
#[ignore = "timing, run by hand in release"]
fn form_fill_timing() {
    for n in [500usize, 1000] {
        let mut s = Session::new();
        let id = s.open("form.pdf", None, Arc::new(many_text_fields(n)), None).expect("opens");
        let started = std::time::Instant::now();
        s.apply(id, fill_all(n)).expect("fills");
        println!("{n} fields: {:?}", started.elapsed());
    }
}
