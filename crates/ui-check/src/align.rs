use crate::image::{Image, dist_sq, rgb};
use crate::scene::{PxRect, Scene, bands};
use crate::{Check, Finding};

const FILL_SLACK: i32 = 120;
const SOLID: i32 = 500;
const SHIFT: i32 = 400;
const TALL: u32 = 40;
const PAIR: i32 = 4;
const MIN_BAND: u32 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListAlign {
    pub tolerance: i32,
    pub content: i32,
}

impl Default for ListAlign {
    fn default() -> Self {
        Self {
            tolerance: 1,
            content: 4,
        }
    }
}

#[derive(Clone, Copy)]
struct Band {
    y: i32,
    x: i32,
    h: u32,
    subject_y: i32,
    subject_x: i32,
}

impl Check for ListAlign {
    fn name(&self) -> &'static str {
        "list-align"
    }

    fn run(&self, scene: &Scene<'_>) -> Vec<Finding> {
        let mut findings = Vec::new();
        for group in bands(&scene.items, true) {
            self.edge(scene, &group, true, &mut findings);
            self.content(scene, &group, &mut findings);
        }
        for group in bands(&scene.items, false) {
            self.edge(scene, &group, false, &mut findings);
            self.content(scene, &group, &mut findings);
        }
        findings
    }
}

impl ListAlign {
    fn edge(
        self,
        scene: &Scene<'_>,
        group: &[usize],
        horizontal: bool,
        findings: &mut Vec<Finding>,
    ) {
        if group.len() < 2 {
            return;
        }
        let origin = median(
            group
                .iter()
                .map(|&index| cross(scene.items[index], horizontal)),
        );
        for &index in group {
            let rect = scene.items[index];
            let delta = (cross(rect, horizontal) - origin).abs();
            if delta <= self.tolerance {
                continue;
            }
            let axis = if horizontal { "row" } else { "column" };
            findings.push(Finding {
                check: self.name(),
                message: format!("{axis} is off by {delta}px"),
                bounds: rect,
            });
        }
    }

    fn content(self, scene: &Scene<'_>, group: &[usize], findings: &mut Vec<Finding>) {
        if group.len() < 2 || findings.len() >= 32 {
            return;
        }
        let ink: Vec<Vec<Band>> = group
            .iter()
            .map(|&index| bands_of(scene, scene.items[index]))
            .collect();
        let reference = &ink[0];
        for (item_index, bands) in ink.iter().enumerate().skip(1) {
            let item = scene.items[group[item_index]];
            for band in reference.iter().filter(|band| band.h >= MIN_BAND) {
                let Some(other) = bands
                    .iter()
                    .filter(|other| other.h >= MIN_BAND)
                    .min_by_key(|other| (other.y - band.y).abs())
                else {
                    if band.h >= TALL {
                        findings.push(self.miss(item));
                    }
                    continue;
                };
                if (other.y - band.y).abs() > PAIR {
                    if band.h >= TALL {
                        findings.push(self.miss(item));
                    }
                    continue;
                }
                let delta = (band.x - other.x).abs().max((band.y - other.y).abs());
                let subject = if band.h >= TALL && other.h >= TALL {
                    (band.subject_x - other.subject_x)
                        .abs()
                        .max((band.subject_y - other.subject_y).abs())
                } else {
                    0
                };
                let delta = delta.max(subject);
                if delta <= self.content {
                    continue;
                }
                findings.push(Finding {
                    check: self.name(),
                    message: format!("content is off by {delta}px"),
                    bounds: item,
                });
            }
        }
    }

    fn miss(self, item: PxRect) -> Finding {
        Finding {
            check: self.name(),
            message: "content is off by more than the row".to_string(),
            bounds: item,
        }
    }
}

fn cross(rect: PxRect, horizontal: bool) -> i32 {
    if horizontal { rect.y } else { rect.x }
}

fn median(values: impl Iterator<Item = i32>) -> i32 {
    let mut values: Vec<i32> = values.collect();
    values.sort_unstable();
    values.get(values.len() / 2).copied().unwrap_or(0)
}

fn bands_of(scene: &Scene<'_>, item: PxRect) -> Vec<Band> {
    let fill = fill_color(scene.image, item);
    let rect = interior(scene.image, item, scene.background, fill);
    if rect.w == 0 || rect.h == 0 {
        return Vec::new();
    }
    let mut rows = Vec::new();
    for step in 0..rect.h as i32 {
        if let Some(x) = ink_x(scene, rect, scene.background, fill, step) {
            rows.push((step, x));
        }
    }
    let mut bands = Vec::new();
    let mut index = 0;
    while index < rows.len() {
        let start = index;
        index += 1;
        while index < rows.len()
            && rows[index].0 == rows[index - 1].0 + 1
            && (rows[index].1 - rows[start].1).abs() <= PAIR
        {
            index += 1;
        }
        let y = rect.y - item.y + rows[start].0;
        let x = rows[start..index]
            .iter()
            .map(|(_, column)| *column - item.x)
            .min()
            .unwrap_or(0);
        let h = (rows[index - 1].0 - rows[start].0 + 1) as u32;
        let (subject_y, subject_x) = if h >= TALL {
            subject(scene, item, rect, scene.background, fill, rows[start].0, h)
        } else {
            (y, x)
        };
        bands.push(Band {
            y,
            x,
            h,
            subject_y,
            subject_x,
        });
    }
    bands
}

fn subject(
    scene: &Scene<'_>,
    item: PxRect,
    rect: PxRect,
    background: [u8; 3],
    fill: [u8; 3],
    start: i32,
    h: u32,
) -> (i32, i32) {
    let top = rect.y + start;
    let subject_y = walk_rows(scene.image, rect, background, fill, top, h);
    let subject_x = walk_columns(scene.image, rect, background, fill, top, h);
    (subject_y - item.y, subject_x - item.x)
}

#[derive(Clone, Copy)]
struct Probe {
    across: bool,
    at: i32,
    origin: i32,
    span: i32,
}

fn walk_rows(
    image: &Image,
    rect: PxRect,
    background: [u8; 3],
    fill: [u8; 3],
    top: i32,
    h: u32,
) -> i32 {
    let probe = |step: i32| Probe {
        across: true,
        at: top + step,
        origin: rect.x,
        span: rect.w as i32,
    };
    let Some(start) = solid_start(image, background, fill, h as i32, &probe) else {
        return top;
    };
    let first = line_tone(image, background, fill, probe(start)).expect("solid row");
    for step in (start + 1)..h as i32 {
        let Some((solid, average)) = line_tone(image, background, fill, probe(step)) else {
            continue;
        };
        if !solid || dist_sq(average, first.1) > SHIFT {
            return top + step;
        }
    }
    top + start
}

fn solid_start(
    image: &Image,
    background: [u8; 3],
    fill: [u8; 3],
    limit: i32,
    probe: &dyn Fn(i32) -> Probe,
) -> Option<i32> {
    (0..limit).find(|&step| {
        line_tone(image, background, fill, probe(step)).is_some_and(|(solid, _)| solid)
    })
}

fn walk_columns(
    image: &Image,
    rect: PxRect,
    background: [u8; 3],
    fill: [u8; 3],
    top: i32,
    h: u32,
) -> i32 {
    let probe = |step: i32| Probe {
        across: false,
        at: rect.x + step,
        origin: top,
        span: h as i32,
    };
    let Some(start) = solid_start(image, background, fill, rect.w as i32, &probe) else {
        return rect.x;
    };
    let first = line_tone(image, background, fill, probe(start)).expect("solid column");
    for step in (start + 1)..rect.w as i32 {
        let Some((solid, average)) = line_tone(image, background, fill, probe(step)) else {
            continue;
        };
        if !solid || dist_sq(average, first.1) > SHIFT {
            return rect.x + step;
        }
    }
    rect.x + start
}

fn line_tone(
    image: &Image,
    background: [u8; 3],
    fill: [u8; 3],
    probe: Probe,
) -> Option<(bool, [u8; 3])> {
    let inset = if probe.across { probe.span / 4 } else { 0 };
    let end = (probe.span - inset).max(inset);
    let mut sum = [0i32; 3];
    let mut count = 0i32;
    for offset in inset..end {
        let color = sample(image, probe, offset);
        if dist_sq(color, background) <= FILL_SLACK || dist_sq(color, fill) <= FILL_SLACK {
            continue;
        }
        sum[0] += i32::from(color[0]);
        sum[1] += i32::from(color[1]);
        sum[2] += i32::from(color[2]);
        count += 1;
    }
    if count == 0 {
        return None;
    }
    let average = [
        (sum[0] / count) as u8,
        (sum[1] / count) as u8,
        (sum[2] / count) as u8,
    ];
    let mut squares = 0i32;
    for offset in inset..end {
        let color = sample(image, probe, offset);
        if dist_sq(color, background) <= FILL_SLACK || dist_sq(color, fill) <= FILL_SLACK {
            continue;
        }
        squares += dist_sq(color, average);
    }
    Some((squares / count <= SOLID, average))
}

fn sample(image: &Image, probe: Probe, offset: i32) -> [u8; 3] {
    let (x, y) = if probe.across {
        (probe.origin + offset, probe.at)
    } else {
        (probe.at, probe.origin + offset)
    };
    rgb(image.get(x, y))
}

fn ink_x(
    scene: &Scene<'_>,
    rect: PxRect,
    background: [u8; 3],
    fill: [u8; 3],
    step: i32,
) -> Option<i32> {
    let y = rect.y + step;
    (0..rect.w as i32)
        .find(|&offset| {
            let color = rgb(scene.image.get(rect.x + offset, y));
            dist_sq(color, background) > FILL_SLACK && dist_sq(color, fill) > FILL_SLACK
        })
        .map(|offset| rect.x + offset)
}

fn interior(image: &Image, bounds: PxRect, background: [u8; 3], fill: [u8; 3]) -> PxRect {
    let top = depth(image, bounds, background, fill, true, true);
    let bottom = depth(image, bounds, background, fill, true, false);
    let left = depth(image, bounds, background, fill, false, true);
    let right = depth(image, bounds, background, fill, false, false);
    PxRect {
        x: bounds.x + left,
        y: bounds.y + top,
        w: (bounds.w as i32 - left - right).max(0) as u32,
        h: (bounds.h as i32 - top - bottom).max(0) as u32,
    }
}

fn depth(
    image: &Image,
    bounds: PxRect,
    background: [u8; 3],
    fill: [u8; 3],
    horizontal: bool,
    leading: bool,
) -> i32 {
    let center_x = bounds.x + bounds.w as i32 / 2;
    let center_y = bounds.y + bounds.h as i32 / 2;
    let limit = if horizontal {
        bounds.h as i32
    } else {
        bounds.w as i32
    };
    let limit = limit.min(16);
    for step in 0..limit {
        let (x, y) = match (horizontal, leading) {
            (true, true) => (center_x, bounds.y + step),
            (true, false) => (center_x, bounds.bottom() - 1 - step),
            (false, true) => (bounds.x + step, center_y),
            (false, false) => (bounds.right() - 1 - step, center_y),
        };
        let color = rgb(image.get(x, y));
        if dist_sq(color, background) <= FILL_SLACK {
            continue;
        }
        if dist_sq(color, fill) <= FILL_SLACK {
            return step;
        }
    }
    limit
}

fn fill_color(image: &Image, bounds: PxRect) -> [u8; 3] {
    let mut counts: Vec<([u8; 3], u32)> = Vec::new();
    for depth in [2, 4, 8, 12] {
        for color in edge_samples(image, bounds, depth) {
            let key = color.map(|channel| channel / 8 * 8);
            if let Some((_, count)) = counts.iter_mut().find(|(known, _)| *known == key) {
                *count += 1;
            } else {
                counts.push((key, 1));
            }
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map_or([0, 0, 0], |(color, _)| color)
}

fn edge_samples(image: &Image, bounds: PxRect, depth: i32) -> [[u8; 3]; 4] {
    let cx = bounds.x + bounds.w as i32 / 2;
    let cy = bounds.y + bounds.h as i32 / 2;
    [
        rgb(image.get(cx, bounds.y + depth)),
        rgb(image.get(cx, bounds.bottom() - 1 - depth)),
        rgb(image.get(bounds.x + depth, cy)),
        rgb(image.get(bounds.right() - 1 - depth, cy)),
    ]
}
