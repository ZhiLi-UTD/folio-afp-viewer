//! Integration tests: parse the synthetic fixtures produced by `afpgen`.

use afp_core::{Document, ResourceKind};

#[test]
fn simple_has_one_page() {
    let bytes = afpgen::simple();
    let doc = Document::parse(&bytes).unwrap();
    assert!(doc.problems.is_empty());
    assert_eq!(doc.summary(bytes.len()).pages, 1);
}

#[test]
fn multi_page_has_three_pages() {
    let bytes = afpgen::multi_page();
    let doc = Document::parse(&bytes).unwrap();
    assert!(doc.problems.is_empty());
    assert_eq!(doc.summary(bytes.len()).pages, 3);
}

#[test]
fn with_image_exposes_named_image_resource() {
    let bytes = afpgen::with_image();
    let doc = Document::parse(&bytes).unwrap();
    assert!(doc.problems.is_empty());
    let res = doc.resources(&bytes);
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].kind, ResourceKind::ImageObject);
    assert_eq!(res[0].name.as_deref(), Some("PIC1"));
}

#[test]
fn malformed_parses_without_panic_and_reports_problems() {
    let bytes = afpgen::malformed();
    let doc = Document::parse(&bytes).unwrap();
    assert!(!doc.problems.is_empty());
}
