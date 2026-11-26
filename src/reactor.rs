//! The async reactor: tasks are plain futures, polled once per frame.

use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};

use bwapi::Game;

/// The tasks of one match. Each is polled once per frame until it completes,
/// then dropped.
pub struct Reactor<'game> {
    tasks: Vec<Pin<Box<dyn Future<Output = ()> + 'game>>>,
    waker: Waker,
}

/// Nothing to wake: every task is polled again next frame anyway.
struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

impl<'game> Reactor<'game> {
    pub fn new() -> Self {
        Reactor { tasks: Vec::new(), waker: Waker::from(Arc::new(NoopWake)) }
    }

    /// Adds a task. It is polled from the next frame on.
    pub fn spawn(&mut self, task: impl Future<Output = ()> + 'game) {
        self.tasks.push(Box::pin(task));
    }

    /// Polls every task once and drops the finished ones.
    pub fn tick(&mut self) {
        let mut cx = Context::from_waker(&self.waker);
        self.tasks.retain_mut(|task| task.as_mut().poll(&mut cx).is_pending());
    }
}

/// Resolves on the poll after the first one. The reactor polls each task once
/// per frame, so awaiting it yields until the next frame.
struct YieldNow(bool);

impl Future for YieldNow {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

/// Yields until the next frame.
pub async fn next_frame() {
    YieldNow(false).await
}

/// Yields until the condition holds. Checked once per frame.
pub async fn until<'game>(game: Game<'game>, mut cond: impl FnMut(Game<'game>) -> bool) {
    while !cond(game) {
        next_frame().await;
    }
}

/// Yields for the given number of frames, by the game clock.
pub async fn sleep<'game>(game: Game<'game>, frames: i32) {
    let wake_at = game.frame_count() + frames;
    until(game, move |game| game.frame_count() >= wake_at).await;
}
