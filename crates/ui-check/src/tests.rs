use crate::image::Image;
use crate::{Check, ElementGap, ListAlign, standard};

fn backdrop() -> Image {
    Image::filled(300, 160, [16, 16, 16, 255])
}

fn card(image: &mut Image, x: u32, y: u32, gap: u32) {
    image.fill_rect(x, y, 80, 110, [200, 200, 200, 255]);
    image.fill_rect(x + 2, y + 2, 76, 106, [80, 80, 80, 255]);
    image.fill_rect(x + 12, y + 12, 56, 30, [240, 240, 240, 255]);
    image.fill_rect(x + 12, y + 12 + 30 + gap, 56, 24, [40, 80, 220, 255]);
}

#[test]
fn aligned_cards_with_gaps_pass() {
    let mut image = backdrop();
    card(&mut image, 16, 20, 8);
    card(&mut image, 112, 20, 8);
    card(&mut image, 208, 20, 8);
    let report = standard().run(&image);
    assert!(report.ok(), "{report:?}");
    assert_eq!(report.items, 3);
}

#[test]
fn a_shifted_card_fails_alignment() {
    let mut image = backdrop();
    card(&mut image, 16, 20, 8);
    card(&mut image, 112, 28, 8);
    card(&mut image, 208, 20, 8);
    let report = ListAlign::default().run(&crate::Scene::read(&image));
    assert!(!report.is_empty(), "{report:?}");
}

#[test]
fn content_against_the_border_fails_the_gap_check() {
    let mut image = backdrop();
    image.fill_rect(20, 20, 90, 100, [200, 200, 200, 255]);
    image.fill_rect(22, 22, 86, 96, [80, 80, 80, 255]);
    image.fill_rect(22, 22, 60, 40, [240, 240, 240, 255]);
    let report = ElementGap::default().run(&crate::Scene::read(&image));
    assert!(
        report
            .iter()
            .any(|finding| finding.message.contains("edge")),
        "{report:?}"
    );
}

#[test]
fn solid_blocks_that_touch_fail_the_gap_check() {
    let mut image = backdrop();
    image.fill_rect(20, 20, 90, 120, [200, 200, 200, 255]);
    image.fill_rect(22, 22, 86, 116, [80, 80, 80, 255]);
    image.fill_rect(32, 32, 66, 30, [240, 240, 240, 255]);
    image.fill_rect(32, 62, 66, 30, [40, 80, 220, 255]);
    let report = ElementGap::default().run(&crate::Scene::read(&image));
    assert!(
        report.iter().any(|finding| finding.message.contains("gap")),
        "{report:?}"
    );
}

fn shell(image: &mut Image, x: u32, y: u32) {
    image.fill_rect(x, y, 80, 110, [200, 200, 200, 255]);
    image.fill_rect(x + 2, y + 2, 76, 106, [80, 80, 80, 255]);
}

#[test]
fn centered_text_fails_alignment() {
    let mut image = backdrop();
    shell(&mut image, 16, 20);
    image.fill_rect(28, 80, 24, 16, [240, 240, 240, 255]);
    shell(&mut image, 140, 20);
    image.fill_rect(168, 80, 24, 16, [240, 240, 240, 255]);
    let report = ListAlign::default().run(&crate::Scene::read(&image));
    assert!(
        report
            .iter()
            .any(|finding| finding.message.contains("content")),
        "{report:?}"
    );
}

#[test]
fn a_letterboxed_cover_fails_alignment() {
    let mut image = backdrop();
    shell(&mut image, 16, 20);
    image.fill_rect(28, 32, 56, 50, [30, 60, 140, 255]);
    shell(&mut image, 140, 20);
    image.fill_rect(152, 32, 56, 50, [230, 220, 200, 255]);
    image.fill_rect(152, 48, 56, 18, [30, 60, 140, 255]);
    let report = ListAlign::default().run(&crate::Scene::read(&image));
    assert!(
        report
            .iter()
            .any(|finding| finding.message.contains("content")),
        "{report:?}"
    );
}

#[test]
fn a_tight_list_gap_fails_a_raised_minimum() {
    let mut image = backdrop();
    card(&mut image, 16, 20, 8);
    card(&mut image, 100, 20, 8);
    let report = ElementGap { min_gap: 8 }.run(&crate::Scene::read(&image));
    assert!(
        report
            .iter()
            .any(|finding| finding.message.contains("list")),
        "{report:?}"
    );
}
