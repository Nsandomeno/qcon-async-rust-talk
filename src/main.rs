use std::task::{Poll, Waker, Context};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::future::Future;
use std::thread;
use std::time::Duration;

pub struct TimerFuture {
    shared_state: Arc<Mutex<SharedState>>
}

struct SharedState {
    // Whether or not the sleep time has elapsed
    completed: bool,
    // The Waker to wake the Future up
    waker: Option<Waker>
}

impl TimerFuture {
    fn new(duration: Duration) -> Self {
        let shared_state = Arc::new(
            Mutex::new(
                SharedState {
                    completed: false,
                    waker: None,
                }
            )
        );
        let thread_shared_state = shared_state.clone();
        thread::spawn(move || {
            thread::sleep(duration);
            let mut shared_state = thread_shared_state.lock().unwrap();
            shared_state.completed = true;

            if let Some(waker) = shared_state.waker.take() {
                waker.wake()
            }
        });

        TimerFuture { shared_state: shared_state }
    }
}

impl Future for TimerFuture {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut shared_state = self.shared_state.lock().unwrap();

        if shared_state.completed {
            Poll::Ready(())
        } else {
            shared_state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

fn main() {
    println!("Hello, world!");
}
