use crate::image::{Image, dist_sq, rgb};
use crate::scene::{PxRect, Scene, bands};
use crate::{Check, Finding};

const FILL_SLACK: i32 = 120;
const SOLID: i32 = 500;
const SHIFT: i32 = 400;
const HOLD: usize = 6;
const EDGE_SLACK: i32 = 2;
const FINDING_CAP: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ElementGap {
    pub min_gap: i32,
}

impl Default for ElementGap {
    fn default() -> Self {
        Self { min_gap: 1 }
    }
}

#[derive(Clone, Copy)]
struct Tone {
    content: bool,
    average: [u8; 3],
    variance: i32,
}

impl Check for ElementGap {
    fn name(&self) -> &'static str {
        "element-gap"
    }

    fn run(&self, scene: &Scene<'_>) -> Vec<Finding> {
        let mut findings = Vec::new();
        self.between_items(scene, true, &mut findings);
        self.between_items(scene, false, &mut findings);
        for item in &scene.items {
            if findings.len() >= FINDING_CAP {
                break;
            }
            self.inside(scene, *item, &mut findings);
        }
        findings
    }
}

impl ElementGap {
    fn between_items(self, scene: &Scene<'_>, horizontal: bool, findings: &mut Vec<Finding>) {
        for group in bands(&scene.items, horizontal) {
            if group.len() < 2 {
                continue;
            }
            let mut order = group;
            order.sort_by(|&left, &right| {
                let a = scene.items[left];
                let b = scene.items[right];
                if horizontal {
                    a.x.cmp(&b.x)
                } else {
                    a.y.cmp(&b.y)
                }
            });
            for pair in order.windows(2) {
                let previous = scene.items[pair[0]];
                let next = scene.items[pair[1]];
                let gap = if horizontal {
                    next.x - previous.right()
                } else {
                    next.y - previous.bottom()
                };
                if gap >= self.min_gap {
                    continue;
                }
                findings.push(Finding {
                    check: self.name(),
                    message: format!("list items have a {gap}px gap"),
                    bounds: next,
                });
            }
        }
    }

    fn inside(self, scene: &Scene<'_>, item: PxRect, findings: &mut Vec<Finding>) {
        let fill = padding_color(scene.image, item);
        let rect = interior(scene.image, item, scene.background, fill);
        if rect.w == 0 || rect.h == 0 {
            findings.push(Finding {
                check: self.name(),
                message: "content meets the card edge".to_string(),
                bounds: item,
            });
            return;
        }
        let rows = tones(scene.image, rect, scene.background, fill, true);
        self.separators(item, rect, &rows, true, true, findings);
        for (start, end) in segments(&rows, true) {
            let band = PxRect {
                x: rect.x,
                y: rect.y + start as i32,
                w: rect.w,
                h: (end - start) as u32,
            };
            let columns = tones(scene.image, band, scene.background, fill, false);
            self.separators(item, band, &columns, false, false, findings);
        }
    }

    fn separators(
        self,
        item: PxRect,
        rect: PxRect,
        tones: &[Tone],
        horizontal: bool,
        split_color: bool,
        findings: &mut Vec<Finding>,
    ) {
        let parts = segments(tones, split_color);
        if parts.is_empty() {
            return;
        }
        let lead = parts[0].0 as i32;
        if lead < self.min_gap {
            findings.push(self.edge(item, rect, horizontal, true));
        }
        let tail = tones.len() as i32 - parts.last().expect("part").1 as i32;
        if tail < self.min_gap {
            findings.push(self.edge(item, rect, horizontal, false));
        }
        for pair in parts.windows(2) {
            let gap = pair[1].0 as i32 - pair[0].1 as i32;
            if gap >= self.min_gap {
                continue;
            }
            let bounds = if horizontal {
                PxRect {
                    x: rect.x,
                    y: rect.y + pair[0].1 as i32,
                    w: rect.w,
                    h: gap.max(1) as u32,
                }
            } else {
                PxRect {
                    x: rect.x + pair[0].1 as i32,
                    y: rect.y,
                    w: gap.max(1) as u32,
                    h: rect.h,
                }
            };
            findings.push(Finding {
                check: self.name(),
                message: format!("content has a {gap}px gap"),
                bounds,
            });
        }
    }

    fn edge(self, item: PxRect, rect: PxRect, horizontal: bool, leading: bool) -> Finding {
        let side = match (horizontal, leading) {
            (true, true) => "top",
            (true, false) => "bottom",
            (false, true) => "left",
            (false, false) => "right",
        };
        Finding {
            check: self.name(),
            message: format!("content meets the {side} edge"),
            bounds: if horizontal { rect } else { item },
        }
    }
}

fn padding_color(image: &Image, bounds: PxRect) -> [u8; 3] {
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

fn interior(image: &Image, bounds: PxRect, background: [u8; 3], fill: [u8; 3]) -> PxRect {
    let top = depth(image, bounds, background, fill, true, true) + EDGE_SLACK;
    let bottom = depth(image, bounds, background, fill, true, false) + EDGE_SLACK;
    let left = depth(image, bounds, background, fill, false, true) + EDGE_SLACK;
    let right = depth(image, bounds, background, fill, false, false) + EDGE_SLACK;
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
    let limit = limit.min(12);
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

fn tones(image: &Image, rect: PxRect, background: [u8; 3], fill: [u8; 3], rows: bool) -> Vec<Tone> {
    let length = if rows { rect.h } else { rect.w };
    (0..length)
        .map(|step| tone(image, rect, background, fill, rows, step as i32))
        .collect()
}

fn tone(
    image: &Image,
    rect: PxRect,
    background: [u8; 3],
    fill: [u8; 3],
    rows: bool,
    step: i32,
) -> Tone {
    let mut sum = [0i32; 3];
    let mut count = 0i32;
    let mut squares = 0i32;
    let (from, to) = sample_range(rect, rows);
    for offset in from..to {
        let (x, y) = if rows {
            (rect.x + offset, rect.y + step)
        } else {
            (rect.x + step, rect.y + offset)
        };
        let color = rgb(image.get(x, y));
        if dist_sq(color, background) <= FILL_SLACK || dist_sq(color, fill) <= FILL_SLACK {
            continue;
        }
        sum[0] += i32::from(color[0]);
        sum[1] += i32::from(color[1]);
        sum[2] += i32::from(color[2]);
        count += 1;
    }
    if count == 0 {
        return Tone {
            content: false,
            average: fill,
            variance: 0,
        };
    }
    let average = [
        (sum[0] / count) as u8,
        (sum[1] / count) as u8,
        (sum[2] / count) as u8,
    ];
    for offset in from..to {
        let (x, y) = if rows {
            (rect.x + offset, rect.y + step)
        } else {
            (rect.x + step, rect.y + offset)
        };
        let color = rgb(image.get(x, y));
        if dist_sq(color, background) <= FILL_SLACK || dist_sq(color, fill) <= FILL_SLACK {
            continue;
        }
        squares += dist_sq(color, average);
    }
    Tone {
        content: true,
        average,
        variance: squares / count,
    }
}

fn sample_range(rect: PxRect, rows: bool) -> (i32, i32) {
    let span = if rows { rect.w } else { rect.h } as i32;
    if rows {
        let inset = span / 4;
        (inset, span - inset)
    } else {
        (0, span)
    }
}

fn segments(tones: &[Tone], split_color: bool) -> Vec<(usize, usize)> {
    let mut parts = Vec::new();
    let mut index = 0;
    while index < tones.len() {
        if !tones[index].content {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        while index < tones.len() && tones[index].content {
            if split_color && splits(tones, index) {
                break;
            }
            index += 1;
        }
        parts.push((start, index));
    }
    parts
}

fn splits(tones: &[Tone], index: usize) -> bool {
    if index < HOLD || index + HOLD > tones.len() {
        return false;
    }
    let previous = &tones[index - HOLD..index];
    let next = &tones[index..index + HOLD];
    let solid = |window: &[Tone]| {
        window
            .iter()
            .all(|tone| tone.content && tone.variance <= SOLID)
    };
    if !solid(previous) || !solid(next) {
        return false;
    }
    dist_sq(previous[HOLD / 2].average, next[0].average) > SHIFT
}
