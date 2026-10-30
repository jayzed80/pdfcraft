//! Junk input to the pure text helpers (name escapes, `/DA` strings, date parsing, button
//! scripts) must never panic.

use pdfcraft_forms::af::{button_script, parse_date};
use pdfcraft_forms::appearance::{auto_text_size, comb_auto_text_size, fmt, parse_da};
use pdfcraft_forms::{name_bytes, name_text};
use proptest::prelude::*;

#[test]
fn name_text_round_trips_every_byte_string() {
    let all: Vec<u8> = (0..=255u8).collect();
    for bytes in
        [all, b"#".to_vec(), b"#2".to_vec(), b"a#zz#".to_vec(), "\u{3010}\u{306f}\u{3044}\u{3011}".as_bytes().to_vec(), vec![0xff, b'#', 0x82]]
    {
        assert_eq!(name_bytes(&name_text(&bytes)), bytes);
    }
}

#[test]
fn name_bytes_tolerates_truncated_escapes() {
    for s in ["#", "##", "#1", "#G1", "#1G", "x#", "#ÿ", "\u{0}#\u{0}"] {
        let _ = name_bytes(s);
    }
}

#[test]
fn numeric_helpers_survive_non_finite_and_extreme_values() {
    let vals = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX, f64::MIN, -0.0, 0.0, 1e308, 1e-308];
    for &w in &vals {
        let _ = fmt(w);
        let _ = comb_auto_text_size(w);
        for &h in &vals {
            for multiline in [false, true] {
                let _ = auto_text_size("some text to fit in a box", w, h, multiline, |t, s| t.len() as f64 * s * 0.5);
            }
        }
    }
}

#[test]
fn da_strings_with_junk_tokens() {
    for da in [
        "",
        "Tf",
        "Tf Tf Tf",
        "/F nan Tf",
        "/F inf Tf",
        "/F -inf Tf",
        "/F 1e999 Tf",
        "/F -3 Tf",
        "g",
        "rg",
        "1 2 rg",
        "k k k k k",
        "/F 12 Tf 0 0 0 0 0 k",
        "\u{0}\u{ffff} Tf",
    ] {
        let d = parse_da(da);
        assert!(d.size.is_finite() && d.size >= 0.0 && d.size <= 300.0, "{da:?} gave size {}", d.size);
    }
}

proptest! {
    #[test]
    fn parse_da_never_panics(s in ".{0,200}") {
        let d = parse_da(&s);
        prop_assert!(d.size.is_finite() && (0.0..=300.0).contains(&d.size));
    }

    #[test]
    fn dates_and_scripts_never_panic(value in ".{0,60}", fmt_str in ".{0,30}", js in ".{0,200}") {
        let _ = parse_date(&value, &fmt_str);
        let _ = button_script(&js);
    }

    #[test]
    fn name_text_round_trips(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
        prop_assert_eq!(name_bytes(&name_text(&bytes)), bytes);
    }
}
