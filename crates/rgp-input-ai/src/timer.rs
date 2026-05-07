use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::{Arc, Mutex, Condvar};
use std::thread;
use std::time::{Duration, Instant};
use crossbeam_channel::Sender;
use rgp_core::InputEvent;

#[derive(Debug)]
struct Scheduled {
    deadline: Instant,
    event: InputEvent,
}

impl PartialEq for Scheduled { fn eq(&self, o: &Self) -> bool { self.deadline == o.deadline } }
impl Eq for Scheduled {}
impl Ord for Scheduled { fn cmp(&self, o: &Self) -> std::cmp::Ordering { self.deadline.cmp(&o.deadline) } }
impl PartialOrd for Scheduled { fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(o)) } }

#[derive(Clone)]
pub(crate) struct Timer {
    state: Arc<TimerState>,
}

struct TimerState {
    heap: Mutex<BinaryHeap<Reverse<Scheduled>>>,
    cvar: Condvar,
    shutdown: Mutex<bool>,
}

impl Timer {
    pub fn new(tx: Sender<InputEvent>) -> Self {
        let state = Arc::new(TimerState {
            heap: Mutex::new(BinaryHeap::new()),
            cvar: Condvar::new(),
            shutdown: Mutex::new(false),
        });
        let thread_state = state.clone();
        thread::Builder::new()
            .name("rgp-input-ai-timer".into())
            .spawn(move || timer_loop(thread_state, tx))
            .expect("spawn timer thread");
        Timer { state }
    }

    pub fn schedule(&self, deadline: Instant, event: InputEvent) {
        let mut heap = self.state.heap.lock().unwrap();
        heap.push(Reverse(Scheduled { deadline, event }));
        self.state.cvar.notify_all();
    }

    /// Signal the timer thread to exit at its next wake.
    pub(crate) fn shutdown(&self) {
        let mut sd = self.state.shutdown.lock().unwrap();
        *sd = true;
        drop(sd);
        self.state.cvar.notify_all();
    }
}

fn timer_loop(state: Arc<TimerState>, tx: Sender<InputEvent>) {
    loop {
        // Check shutdown flag.
        {
            let sd = state.shutdown.lock().unwrap();
            if *sd { return; }
        }
        let mut heap = state.heap.lock().unwrap();
        let timeout = match heap.peek() {
            Some(Reverse(s)) => s.deadline.saturating_duration_since(Instant::now()),
            None => Duration::from_secs(60),
        };
        let res = state.cvar.wait_timeout(heap, timeout).unwrap();
        heap = res.0;
        // Re-check shutdown after wait.
        {
            let sd = state.shutdown.lock().unwrap();
            if *sd { return; }
        }
        let now = Instant::now();
        while let Some(Reverse(s)) = heap.peek() {
            if s.deadline <= now {
                let scheduled = heap.pop().unwrap().0;
                drop(heap);
                if tx.send(scheduled.event).is_err() {
                    return;
                }
                heap = state.heap.lock().unwrap();
            } else {
                break;
            }
        }
    }
}
