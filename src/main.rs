use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::task::{Poll, Waker, Context};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::future::Future;
use std::thread;
use std::time::Duration;
use futures::future::{BoxFuture};
use futures::FutureExt;
use futures::task::{ArcWake, waker_ref};

pub struct TimerFuture {
    shared_state: Arc<Mutex<SharedState>>
}

struct SharedState {
    // Whether or not the sleep time has elapsed
    completed: bool,
    // The Waker to wake the Future up
    waker: Option<Waker>
}

// Task executor that receives off the receiving end of a channel
// and executes them
struct Executor {
    ready_queue: Receiver<Arc<Task>>
}

impl Executor {
    fn run(&self) {
        while let Ok(task) = self.ready_queue.recv() {
            // Take the future and if it has not completed 
            // poll in an attempt to complete it.
            let mut future_slot = task.future.lock().unwrap();

            if let Some(mut future) = future_slot.take() {
                // Create a local waker from the task itself
                let waker = waker_ref(&task);
                let context = &mut Context::from_waker(&*waker);

                if let Poll::Pending = future.as_mut().poll(context) {
                    // We're not done processing the future so put back in
                    // its task to be run again later
                    *future_slot = Some(future);
                }
            }
        }
    }
}
// A future that can reschedule itself to be polled
// by an Executor
struct Task {
    // In-Progress future that should be pushed to completion
    future: Mutex<Option<BoxFuture<'static, ()>>>,
    // Handle to place the task itself back on the task queue.
    task_sender: SyncSender<Arc<Task>>,
}

impl ArcWake for Task {
    // This plays the role of the reactor
    fn wake_by_ref(arc_self: &Arc<Self>) {
        let cloned = arc_self.clone();

        arc_self.task_sender.send(cloned).expect("Too many tasks queued.");
    }
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

// Spawns new futures onto the task channel
#[derive(Clone)]
struct Spawner {
    task_sender: SyncSender<Arc<Task>>
}

impl Spawner {
    fn spawn(&self, future: impl Future<Output = ()> + 'static + Send) {
        let future = future.boxed();

        let task = Arc::new(Task {
            future: Mutex::new(Some(future)),
            task_sender: self.task_sender.clone(),
        });

        self.task_sender.send(task).expect("Too many tasks queued.")
    }
}

fn new_executer_and_spawner() -> (Executor, Spawner) {
    // Maximum number of tasks to allow queueing in the channel at once.
    const MAX_QUEUED_TASKS: usize = 10_000;
    let (task_sender, ready_queue) = sync_channel(MAX_QUEUED_TASKS);
    (Executor { ready_queue }, Spawner { task_sender })
}

fn main() {
    let (executor, spawner) = new_executer_and_spawner();
    // Spawn a task to print before and after waiting on a timer
    spawner.spawn(async {
        println!("Howdy!");
        // Wait for our TimerFuture to complete after 2 seconds
        TimerFuture::new(Duration::new(2, 0)).await;
        println!("Done");
    });
    // Drop the spawner so that our Executor knows the it is finished
    // and wont receive more incoming tasks to run.
    drop(spawner);
    // Run the executor until the task queue is empty.
    // This will print howdy, pause, and then print done.
    executor.run();
}
