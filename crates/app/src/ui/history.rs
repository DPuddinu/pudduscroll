use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pudduscroll_core::{FrameId, Layout};

use crate::library::Blob;

pub(super) const HISTORY_WINDOW: Duration = Duration::from_millis(700);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum HistoryKind {
    Zoom,
    Grid,
    Border(u64),
}

#[derive(Clone)]
pub(super) struct Edit {
    pub(super) layout: Layout,
    pub(super) photos: HashMap<FrameId, Blob>,
    pub(super) grid_cells: i32,
}

impl Edit {
    pub(super) fn differs(&self, other: &Self) -> bool {
        self.layout != other.layout
            || self.grid_cells != other.grid_cells
            || photo_stamp(&self.photos) != photo_stamp(&other.photos)
    }
}

pub(super) struct EditHistory<T> {
    undo: Vec<T>,
    redo: Vec<T>,
    open: Option<(HistoryKind, Instant)>,
    limit: usize,
}

impl<T> EditHistory<T> {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            open: None,
            limit,
        }
    }

    pub(super) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.open = None;
    }

    pub(super) fn checkpoint(&mut self, current: T) {
        self.undo.push(current);
        trim(&mut self.undo, self.limit);
        self.redo.clear();
        self.open = None;
    }

    pub(super) fn absorb(&mut self, kind: HistoryKind, now: Instant, window: Duration) -> bool {
        let Some((open, at)) = self.open else {
            return false;
        };
        if open != kind || now.saturating_duration_since(at) >= window {
            return false;
        }
        self.open = Some((kind, now));
        true
    }

    pub(super) fn arm(&mut self, kind: HistoryKind, now: Instant) {
        self.open = Some((kind, now));
    }

    pub(super) fn take_undo(&mut self, current: T) -> Option<T> {
        let previous = self.undo.pop()?;
        self.redo.push(current);
        trim(&mut self.redo, self.limit);
        self.open = None;
        Some(previous)
    }

    pub(super) fn take_redo(&mut self, current: T) -> Option<T> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        trim(&mut self.undo, self.limit);
        self.open = None;
        Some(next)
    }
}

fn trim<T>(stack: &mut Vec<T>, limit: usize) {
    if stack.len() > limit {
        let extra = stack.len() - limit;
        stack.drain(0..extra);
    }
}

pub(super) fn photo_stamp(photos: &HashMap<FrameId, Blob>) -> Vec<(u64, usize)> {
    let mut stamp: Vec<_> = photos
        .iter()
        .map(|(id, blob)| (id.0, Arc::as_ptr(&blob.bytes).cast::<u8>() as usize))
        .collect();
    stamp.sort_unstable();
    stamp
}

#[cfg(test)]
mod tests {
    use super::{EditHistory, HISTORY_WINDOW, HistoryKind};
    use std::time::{Duration, Instant};

    #[test]
    fn undo_and_redo_roundtrip_and_a_new_edit_drops_redo() {
        let mut history = EditHistory::new(50);
        history.checkpoint(0);
        assert_eq!(history.take_undo(1), Some(0));
        assert_eq!(history.take_redo(0), Some(1));
        history.checkpoint(1);
        assert_eq!(history.take_redo(2), None);
        assert_eq!(history.take_undo(2), Some(1));
    }

    #[test]
    fn repeated_edits_inside_the_window_are_one_step() {
        let mut history = EditHistory::new(50);
        let start = Instant::now();
        assert!(!history.absorb(HistoryKind::Zoom, start, HISTORY_WINDOW));
        history.checkpoint(0);
        history.arm(HistoryKind::Zoom, start);
        assert!(history.absorb(
            HistoryKind::Zoom,
            start + Duration::from_millis(200),
            HISTORY_WINDOW
        ));
        assert_eq!(history.take_undo(4), Some(0));
        assert!(!history.absorb(
            HistoryKind::Zoom,
            start + Duration::from_millis(200) + HISTORY_WINDOW,
            HISTORY_WINDOW
        ));
    }

    #[test]
    fn history_keeps_the_newest_steps() {
        let mut history = EditHistory::new(2);
        history.checkpoint(1);
        history.checkpoint(2);
        history.checkpoint(3);
        assert_eq!(history.take_undo(4), Some(3));
        assert_eq!(history.take_undo(3), Some(2));
        assert_eq!(history.take_undo(2), None);
    }
}
