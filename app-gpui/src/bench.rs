//! What `--bench idle` counts: renders of the root view while the app sits
//! alone. Idle must be zero, which is the CPU-at-rest promise (no timer, no
//! polling, no repaint).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// GPUI redraws every view when the window gains or loses focus; that is the
/// user's doing, not idle repainting, so renders this soon after one are not
/// counted.
pub const ACTIVATION_GRACE: Duration = Duration::from_millis(300);

#[derive(Default)]
pub struct Stats {
    renders: RefCell<Vec<Instant>>,
    activations: RefCell<Vec<Instant>>,
}

impl Stats {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// The root view rendered.
    pub fn render(&self) {
        self.renders.borrow_mut().push(Instant::now());
    }

    /// The window gained or lost focus.
    pub fn activation_changed(&self) {
        self.activations.borrow_mut().push(Instant::now());
    }

    /// Renders at or after `since` that are not within [`ACTIVATION_GRACE`]
    /// of a focus change.
    pub fn renders_since(&self, since: Instant) -> usize {
        let activations = self.activations.borrow();
        self.renders
            .borrow()
            .iter()
            .filter(|&&at| at >= since)
            .filter(|&&at| {
                !activations
                    .iter()
                    .any(|&change| at >= change && at - change <= ACTIVATION_GRACE)
            })
            .count()
    }
}

pub fn idle_line(renders: usize, seconds: u64) -> String {
    format!("BENCH idle renders={renders} seconds={seconds}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_after_a_focus_change_do_not_count() {
        let stats = Stats::new();
        let start = Instant::now();
        stats.render();
        stats.activation_changed();
        stats.render();
        assert_eq!(stats.renders_since(start), 1);
        assert_eq!(
            stats.renders_since(Instant::now() + Duration::from_secs(1)),
            0
        );
    }

    #[test]
    fn the_line_has_the_shape_the_scripts_read() {
        assert_eq!(idle_line(0, 8), "BENCH idle renders=0 seconds=8");
    }
}
