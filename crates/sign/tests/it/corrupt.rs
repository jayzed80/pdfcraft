//! Damaged input: every reader that takes bytes from a file must answer a corrupted copy of a good
//! file with `Ok` or `Err`, never a panic. Each good file is truncated at every length and has
//! every byte flipped in turn.

use pdfcraft_sign::der::Tlv;
use pdfcraft_sign::revocation::{CertificateList, OcspResponse};
use pdfcraft_sign::{Certificate, cms, pkcs12, timestamp};

fn data(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// The DER inside a PEM file.
fn pem_der(name: &str) -> Vec<u8> {
    use base64::Engine as _;
    let text = String::from_utf8(data(name)).unwrap();
    let b64: String = text.lines().filter(|l| !l.starts_with("-----")).collect();
    base64::engine::general_purpose::STANDARD.decode(b64).unwrap()
}

/// Every truncation of `good`, and a copy of it with each byte (every `step`th) inverted.
fn damaged(good: &[u8], step: usize, mut read: impl FnMut(&[u8])) {
    for len in 0..good.len() {
        read(&good[..len]);
    }
    for i in (0..good.len()).step_by(step) {
        let mut bad = good.to_vec();
        bad[i] ^= 0xFF;
        read(&bad);
    }
}

#[test]
fn damaged_certificates_and_der_never_panic() {
    for name in ["rsa.crt.pem", "ca.crt.pem"] {
        let der = pem_der(name);
        damaged(&der, 1, |b| {
            let _ = Certificate::parse(b);
            let _ = cms::certificate_bundle(b);
            let _ = cms::SignedData::parse(b);
            let _ = timestamp::parse_token(b);
            let _ = timestamp::parse_request(b);
            let _ = timestamp::token_certs(b);
            let _ = CertificateList::parse(b);
            let _ = OcspResponse::parse(b);
            if let Ok((t, _)) = Tlv::parse_ber(b) {
                let _ = t.children();
            }
        });
    }
}

#[test]
fn damaged_pkcs12_files_never_panic() {
    // Opening derives keys, so flip a sample of bytes rather than all of them.
    for name in ["ec-p256.p12", "rsa-aes.p12", "ec-legacy.p12"] {
        let good = data(name);
        assert!(pkcs12::open(&good, "test").is_ok(), "{name} must open undamaged");
        let step = (good.len() / 40).max(1);
        for len in (0..good.len()).step_by(step) {
            let _ = pkcs12::open(&good[..len], "test");
        }
        for i in (0..good.len()).step_by(step) {
            let mut bad = good.clone();
            bad[i] ^= 0xFF;
            let _ = pkcs12::open(&bad, "test");
        }
    }
}
