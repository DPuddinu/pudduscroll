use std::collections::VecDeque;

use crate::image::{Image, dist_sq, rgb};

const ABSORB_SLACK: i32 = 80;
const MIN_SIDE: u32 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PxRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl PxRect {
    #[must_use]
    pub fn right(self) -> i32 {
        self.x + self.w as i32
    }

    #[must_use]
    pub fn bottom(self) -> i32 {
        self.y + self.h as i32
    }
}

#[derive(Clone, Debug)]
pub struct Scene<'a> {
    pub image: &'a Image,
    pub background: [u8; 3],
    pub items: Vec<PxRect>,
}

impl<'a> Scene<'a> {
    #[must_use]
    pub fn read(image: &'a Image) -> Self {
        let background = background(image);
        let items = list_items(image);
        Self {
            image,
            background,
            items,
        }
    }
}

pub(crate) fn bands(items: &[PxRect], horizontal: bool) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by(|&left, &right| {
        let a = items[left];
        let b = items[right];
        if horizontal {
            a.y.cmp(&b.y).then(a.x.cmp(&b.x))
        } else {
            a.x.cmp(&b.x).then(a.y.cmp(&b.y))
        }
    });
    let mut groups = Vec::<Vec<usize>>::new();
    for index in order {
        let join = groups
            .last()
            .is_some_and(|group| shares_track(items, group, index, horizontal));
        if join {
            groups.last_mut().expect("group").push(index);
        } else {
            groups.push(vec![index]);
        }
    }
    groups
}

fn shares_track(items: &[PxRect], group: &[usize], index: usize, horizontal: bool) -> bool {
    let item = items[index];
    let (start, end) = span(item, horizontal);
    let mut group_start = i32::MAX;
    let mut group_end = i32::MIN;
    for &other in group {
        let (other_start, other_end) = span(items[other], horizontal);
        group_start = group_start.min(other_start);
        group_end = group_end.max(other_end);
    }
    let overlap = (group_end.min(end) - group_start.max(start)).max(0);
    let smaller = (group_end - group_start).min(end - start).max(1);
    overlap * 2 > smaller
}

fn span(rect: PxRect, horizontal: bool) -> (i32, i32) {
    if horizontal {
        (rect.y, rect.bottom())
    } else {
        (rect.x, rect.right())
    }
}

fn background(image: &Image) -> [u8; 3] {
    let width = image.width() as i32;
    let height = image.height() as i32;
    if width == 0 || height == 0 {
        return [0, 0, 0];
    }
    let mut counts: Vec<([u8; 3], u32)> = Vec::new();
    let mut add = |x: i32, y: i32| {
        let color = quant(rgb(image.get(x, y)));
        if let Some((_, count)) = counts.iter_mut().find(|(known, _)| *known == color) {
            *count += 1;
        } else {
            counts.push((color, 1));
        }
    };
    for x in 0..width {
        add(x, 0);
        if height > 1 {
            add(x, height - 1);
        }
    }
    for y in 1..height.saturating_sub(1) {
        add(0, y);
        if width > 1 {
            add(width - 1, y);
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map_or([0, 0, 0], |(color, _)| color)
}

fn quant(color: [u8; 3]) -> [u8; 3] {
    color.map(|channel| channel / 8 * 8)
}

fn list_items(image: &Image) -> Vec<PxRect> {
    let width = image.width() as i32;
    let height = image.height() as i32;
    if width <= 1 || height <= 1 {
        return Vec::new();
    }
    let index = |x: i32, y: i32| (y * width + x) as usize;
    let mut absorbed = vec![false; (width * height) as usize];
    let mut queue = VecDeque::new();
    let push = |queue: &mut VecDeque<(i32, i32, [u8; 3])>,
                absorbed: &mut [bool],
                x: i32,
                y: i32,
                seed: [u8; 3]| {
        if x < 0 || y < 0 || x >= width || y >= height {
            return;
        }
        let slot = index(x, y);
        if absorbed[slot] || dist_sq(rgb(image.get(x, y)), seed) > ABSORB_SLACK {
            return;
        }
        absorbed[slot] = true;
        queue.push_back((x, y, seed));
    };
    for x in 0..width {
        let top = rgb(image.get(x, 0));
        push(&mut queue, &mut absorbed, x, 0, top);
        let bottom = rgb(image.get(x, height - 1));
        push(&mut queue, &mut absorbed, x, height - 1, bottom);
    }
    for y in 1..height - 1 {
        let left = rgb(image.get(0, y));
        push(&mut queue, &mut absorbed, 0, y, left);
        let right = rgb(image.get(width - 1, y));
        push(&mut queue, &mut absorbed, width - 1, y, right);
    }
    while let Some((x, y, seed)) = queue.pop_front() {
        push(&mut queue, &mut absorbed, x - 1, y, seed);
        push(&mut queue, &mut absorbed, x + 1, y, seed);
        push(&mut queue, &mut absorbed, x, y - 1, seed);
        push(&mut queue, &mut absorbed, x, y + 1, seed);
    }

    let mut seen = absorbed;
    let mut found = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let start = index(x, y);
            if seen[start] {
                continue;
            }
            let mut pending = VecDeque::from([(x, y)]);
            seen[start] = true;
            let mut min_x = x;
            let mut min_y = y;
            let mut max_x = x;
            let mut max_y = y;
            let mut count = 0u32;
            while let Some((cx, cy)) = pending.pop_front() {
                count += 1;
                min_x = min_x.min(cx);
                min_y = min_y.min(cy);
                max_x = max_x.max(cx);
                max_y = max_y.max(cy);
                for (nx, ny) in [(cx - 1, cy), (cx + 1, cy), (cx, cy - 1), (cx, cy + 1)] {
                    if nx < 0 || ny < 0 || nx >= width || ny >= height {
                        continue;
                    }
                    let slot = index(nx, ny);
                    if seen[slot] {
                        continue;
                    }
                    seen[slot] = true;
                    pending.push_back((nx, ny));
                }
            }
            let rect_w = (max_x - min_x + 1) as u32;
            let rect_h = (max_y - min_y + 1) as u32;
            if rect_w >= MIN_SIDE && rect_h >= MIN_SIDE && count * 2 >= rect_w * rect_h {
                found.push(PxRect {
                    x: min_x,
                    y: min_y,
                    w: rect_w,
                    h: rect_h,
                });
            }
        }
    }
    cluster(found)
}

fn cluster(items: Vec<PxRect>) -> Vec<PxRect> {
    let mut groups: Vec<Vec<PxRect>> = Vec::new();
    for item in items {
        if let Some(group) = groups.iter_mut().find(|group| {
            let sample = group[0];
            near(sample.w, item.w) && near(sample.h, item.h)
        }) {
            group.push(item);
        } else {
            groups.push(vec![item]);
        }
    }
    groups.into_iter().max_by_key(Vec::len).unwrap_or_default()
}

fn near(a: u32, b: u32) -> bool {
    let big = a.max(b);
    let small = a.min(b);
    big - small <= (big / 4).max(8)
}
