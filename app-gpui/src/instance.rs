//! One running copy of this app per user, through fastframe-instance.
//!
//! A second launch hands "show" to the copy that runs and exits; the running
//! copy brings its window forward. The slot is this app's own
//! (`Identity::instance_slot`): a running ZapFast neither blocks this app nor
//! hears from it, and the two can run at once, each on its own archive.
//!
//! The request reaches the app through a `futures` channel a foreground task
//! awaits, so waiting for one costs nothing.

use fastframe_instance::{Claim, Guard};
use futures::channel::mpsc::{UnboundedReceiver, unbounded};

use crate::identity::Identity;

/// What another launch asked of the running copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// Bring the window forward.
    Show,
}

/// How the launch turned out.
pub enum Launch {
    /// This process is the running copy. Keep the guard until it exits and
    /// await the receiver.
    First(Guard, UnboundedReceiver<Request>),
    /// Another copy of this app runs and was asked to show its window.
    AlreadyRunning,
    /// Another copy holds the slot but did not answer in time.
    Unanswered,
}

/// Becomes the running copy, or hands "show" to the one that runs.
pub fn claim(identity: &Identity) -> Launch {
    let (requests, receiver) = unbounded();
    let claim = identity
        .instance_slot()
        .claim("show", move |request| match request {
            "show" => requests
                .unbounded_send(Request::Show)
                .ok()
                .map(|()| "ok".to_owned()),
            // A liveness check: answered, and nothing happens.
            "ping" => Some("ok".to_owned()),
            _ => None,
        });
    match claim {
        Claim::First(guard) => Launch::First(guard, receiver),
        Claim::Running(_) | Claim::Declined => Launch::AlreadyRunning,
        Claim::Unanswered => Launch::Unanswered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_launch_hands_show_to_the_first_and_does_not_run() {
        let root = tempfile::tempdir().unwrap();
        let identity = Identity::under(root.path());
        let Launch::First(guard, mut requests) = claim(&identity) else {
            panic!("the first launch is the running copy");
        };
        assert!(matches!(claim(&identity), Launch::AlreadyRunning));
        assert_eq!(
            futures::executor::block_on(futures::StreamExt::next(&mut requests)),
            Some(Request::Show)
        );
        drop(guard);
    }

    /// Two apps with slots in different folders do not see each other: the
    /// property that lets this app run beside ZapFast.
    #[test]
    fn apps_in_different_runtime_folders_are_both_the_first() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (one, two) = (Identity::under(a.path()), Identity::under(b.path()));
        let first = claim(&one);
        let second = claim(&two);
        assert!(matches!(first, Launch::First(..)));
        assert!(matches!(second, Launch::First(..)));
    }
}
