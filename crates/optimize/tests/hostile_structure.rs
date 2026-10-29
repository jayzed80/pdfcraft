//! The optimizer on malformed document structure: cyclic trees, links to nothing, fragment runs
//! with absurd geometry. It may leave things alone, but never panics or hangs.

use pdfcraft_cos::{Dict, Document, Object, Stream};
use pdfcraft_optimize::{Settings, optimize};

fn image_dict(w: i64, h: i64) -> Dict {
    let mut d = Dict::new();
    d.set(b"Type".to_vec(), Object::name("XObject"));
    d.set(b"Subtype".to_vec(), Object::name("Image"));
    d.set(b"Width".to_vec(), Object::Int(w));
    d.set(b"Height".to_vec(), Object::Int(h));
    d.set(b"BitsPerComponent".to_vec(), Object::Int(8));
    d.set(b"ColorSpace".to_vec(), Object::name("DeviceGray"));
    d
}

/// A one-page document whose resources hold `images` named `I0`, `I1`, ... and whose content is
/// `content`; `extra` may add entries to the page and catalog.
fn doc_with(images: usize, content: &str, extra: impl FnOnce(&mut Document, &mut Dict, &mut Dict)) -> Document {
    let mut doc = Document::new_empty();
    let root = doc.root().unwrap();
    let pages = doc.get(root).as_dict().and_then(|d| d.reference(b"Pages")).unwrap();
    let mut x = Dict::new();
    for i in 0..images {
        let im = doc.add(Object::Stream(Stream::flate(image_dict(1, 1), &[7])));
        x.set(format!("I{i}").into_bytes(), Object::Ref(im));
    }
    let mut res = Dict::new();
    res.set(b"XObject".to_vec(), Object::Dict(x));
    let c = doc.add(Object::Stream(Stream::from_raw(Dict::new(), content.as_bytes().to_vec())));
    let mut p = Dict::new();
    p.set(b"Type".to_vec(), Object::name("Page"));
    p.set(b"Parent".to_vec(), Object::Ref(pages));
    p.set(b"MediaBox".to_vec(), Object::Array(vec![0.into(), 0.into(), 612.into(), 792.into()]));
    p.set(b"Resources".to_vec(), Object::Dict(res));
    p.set(b"Contents".to_vec(), Object::Ref(c));
    let mut cat = doc.get(root).as_dict().unwrap().clone();
    extra(&mut doc, &mut p, &mut cat);
    let r = doc.add(Object::Dict(p));
    doc.update_dict(root, |d| *d = cat).unwrap();
    doc.update_dict(pages, |d| {
        d.set(b"Kids".to_vec(), Object::Array(vec![Object::Ref(r)]));
        d.set(b"Count".to_vec(), Object::Int(1));
    })
    .unwrap();
    doc
}

#[test]
fn fragment_runs_with_absurd_geometry_never_panic() {
    let geoms = [
        |i: usize| format!("q 100 0 0 1 0 {i} cm /I{i} Do Q "),
        |i: usize| format!("q 1e300 0 0 1e300 0 {}e300 cm /I{i} Do Q ", i),
        |i: usize| format!("q 1e-300 0 0 1e-300 0 {} cm /I{i} Do Q ", i),
        |i: usize| format!("q 100 0 0 1 0 -{i}e999 cm /I{i} Do Q "),
        |i: usize| format!("q 0 0 0 0 0 0 cm /I{i} Do Q {i}"),
        |_| "q 100 0 0 1 0 0 cm /I0 Do Q ".to_string(),
    ];
    for g in geoms {
        let content: String = (0..12).map(g).collect();
        let mut doc = doc_with(12, &content, |_, _, _| {});
        let _ = optimize(&mut doc, &Settings::default());
    }
}

#[test]
fn cyclic_name_tree_and_junk_links_never_hang() {
    let mut doc = doc_with(0, "", |doc, page, cat| {
        // A name tree whose kid is itself, and a destinations dictionary of junk.
        let node = doc.add(Object::Dict(Dict::new()));
        doc.update_dict(node, |d| {
            d.set(b"Kids".to_vec(), Object::Array(vec![Object::Ref(node), Object::Ref(node)]));
            d.set(b"Names".to_vec(), Object::Array(vec![Object::Int(1), Object::Ref(node), Object::name("x")]));
        })
        .unwrap();
        let mut names = Dict::new();
        names.set(b"Dests".to_vec(), Object::Ref(node));
        cat.set(b"Names".to_vec(), Object::Dict(names));
        cat.set(b"Dests".to_vec(), Object::Ref(node));
        let mut links = Vec::new();
        for dest in [Object::Int(-1), Object::Ref(node), Object::Array(vec![Object::Ref(node)]), Object::Null, Object::name("nope")] {
            let mut l = Dict::new();
            l.set(b"Type".to_vec(), Object::name("Annot"));
            l.set(b"Subtype".to_vec(), Object::name("Link"));
            l.set(b"Dest".to_vec(), dest);
            links.push(Object::Ref(doc.add(Object::Dict(l))));
        }
        links.push(Object::Int(5));
        page.set(b"Annots".to_vec(), Object::Array(links));
    });
    let _ = optimize(&mut doc, &Settings::default());
}

#[test]
fn self_drawing_form_and_cyclic_resources_never_hang() {
    let mut doc = doc_with(1, "q /F Do Q /I0 Do", |doc, page, _| {
        let f = doc.add(Object::Dict(Dict::new()));
        let mut res = Dict::new();
        let mut x = Dict::new();
        x.set(b"F".to_vec(), Object::Ref(f));
        res.set(b"XObject".to_vec(), Object::Dict(x));
        let mut d = Dict::new();
        d.set(b"Subtype".to_vec(), Object::name("Form"));
        d.set(b"BBox".to_vec(), Object::Array(vec![0.into(), 0.into(), 1.into(), 1.into()]));
        d.set(b"Resources".to_vec(), Object::Dict(res));
        let form = Stream::from_raw(d, b"q /F Do Q".to_vec());
        doc.set(f, Object::Stream(form));
        if let Some(Object::Dict(r)) = page.get(b"Resources").cloned()
            && let Some(Object::Dict(mut x)) = r.get(b"XObject").cloned()
        {
            x.set(b"F".to_vec(), Object::Ref(f));
            let mut r = r;
            r.set(b"XObject".to_vec(), Object::Dict(x));
            page.set(b"Resources".to_vec(), Object::Dict(r));
        }
    });
    let _ = optimize(&mut doc, &Settings::default());
}
