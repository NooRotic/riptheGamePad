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
    state: Arc<(Mutex<BinaryHeap<Reverse<Scheduled>>>, Condvar)>,
}

impl Timer {
    pub fn new(tx: Sender<InputEvent>) -> Self {
        let state: Arc<(Mutex<BinaryHeap<Reverse<Scheduled>>>, Condvar)> =
            Arc::new((Mutex::new(BinaryHeap::new()), Condvar::new()));
        let state_clone = state.clone();
        thread::Builder::new()
            .name("rgp-input-ai-timer".into())
            .spawn(move || timer_loop(state_clone, tx))
            .expect("spawn timer thread");
        Timer { state }
    }

    pub fn schedule(&self, deadline: Instant, event: InputEvent) {
        let (lock, cvar) = &*self.state;
        let mut heap = lock.lock().unwrap();
        heap.push(Reverse(Scheduled { deadline, event }));
        cvar.notify_one();
    }
}

fn timer_loop(state: Arc<(Mutex<BinaryHeap<Reverse<Scheduled>>>, Condvar)>, tx: Sender<InputEvent>) {
    let (lock, cvar) = &*state;
    loop {
        let mut heap = lock.lock().unwrap();
        let timeout = match heap.peek() {
            Some(Reverse(s)) => s.deadline.saturating_duration_since(Instant::now()),
            None => Duration::from_secs(60),
        };
        let res = cvar.wait_timeout(heap, timeout).unwrap();
        heap = res.0;
        let now = Instant::now();
        while let Some(Reverse(s)) = heap.peek() {
            if s.deadline <= now {
                let s = heap.pop().unwrap().0;
                if tx.send(s.event).is_err() {
                    return; // receiver dropped; exit thread
                }
            } else {
                break;
            }
        }
    }
}
