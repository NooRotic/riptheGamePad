pub mod timer;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::collections::HashSet;
use crossbeam_channel::Sender;
use rgp_core::{InputEvent, Control, ButtonId, AxisId, TriggerId, SourceId};

pub struct AiInputHandle {
    tx: Sender<InputEvent>,
    source_id: String,
    timer: timer::Timer,
    /// Buttons currently held by this handle. Used by release_all().
    /// Note: the timer-scheduled release does NOT update this set (the timer
    /// thread has no reference to it). release_all() may therefore emit an
    /// extra release for a button the timer already fired — this is idempotent
    /// (0.0 → 0.0 downstream) and acceptable as a "panic reset" operation.
    held: Arc<Mutex<HashSet<ButtonId>>>,
}

impl AiInputHandle {
    pub fn press(&self, button: ButtonId, duration: Duration) {
        let now = Instant::now();
        self.held.lock().unwrap().insert(button);
        let _ = self.tx.send(InputEvent {
            source: SourceId::Ai(self.source_id.clone()),
            control: Control::Button(button),
            value: 1.0,
            timestamp: now,
        });
        let release_event = InputEvent {
            source: SourceId::Ai(self.source_id.clone()),
            control: Control::Button(button),
            value: 0.0,
            timestamp: now + duration,
        };
        self.timer.schedule(now + duration, release_event);
    }

    pub fn release(&self, button: ButtonId) {
        self.held.lock().unwrap().remove(&button);
        let _ = self.tx.send(InputEvent {
            source: SourceId::Ai(self.source_id.clone()),
            control: Control::Button(button),
            value: 0.0,
            timestamp: Instant::now(),
        });
    }

    pub fn axis(&self, axis: AxisId, value: f32) {
        let _ = self.tx.send(InputEvent {
            source: SourceId::Ai(self.source_id.clone()),
            control: Control::Axis(axis),
            value,
            timestamp: Instant::now(),
        });
    }

    pub fn trigger(&self, t: TriggerId, value: f32) {
        let _ = self.tx.send(InputEvent {
            source: SourceId::Ai(self.source_id.clone()),
            control: Control::Trigger(t),
            value,
            timestamp: Instant::now(),
        });
    }

    pub fn raw(&self, event: InputEvent) {
        let _ = self.tx.send(event);
    }

    /// Release all currently-held buttons. Idempotent.
    /// Used by rgp-input-ai-server on client disconnect.
    pub fn release_all(&self) {
        let buttons: Vec<ButtonId> = {
            let mut held = self.held.lock().unwrap();
            held.drain().collect()
        };
        let now = Instant::now();
        for button in buttons {
            let _ = self.tx.send(InputEvent {
                source: SourceId::Ai(self.source_id.clone()),
                control: Control::Button(button),
                value: 0.0,
                timestamp: now,
            });
        }
    }
}

pub fn handle(events_tx: Sender<InputEvent>, source_id: impl Into<String>) -> AiInputHandle {
    let source_id = source_id.into();
    let timer = timer::Timer::new(events_tx.clone());
    AiInputHandle {
        tx: events_tx,
        source_id,
        timer,
        held: Arc::new(Mutex::new(HashSet::new())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rgp_core::{ButtonId, Control, InputEvent, AxisId};
    use std::time::{Duration, Instant};

    #[test]
    fn press_emits_press_then_release_after_duration() {
        let (tx, rx) = crossbeam_channel::unbounded::<InputEvent>();
        let h = handle(tx, "test_agent");
        h.press(ButtonId::South, Duration::from_millis(50));

        let press_ev = rx.recv_timeout(Duration::from_millis(100)).expect("press event");
        assert!(matches!(press_ev.control, Control::Button(ButtonId::South)));
        assert_eq!(press_ev.value, 1.0);

        let release_ev = rx.recv_timeout(Duration::from_millis(150)).expect("release event");
        assert!(matches!(release_ev.control, Control::Button(ButtonId::South)));
        assert_eq!(release_ev.value, 0.0);

        let elapsed = release_ev.timestamp.duration_since(press_ev.timestamp);
        assert!(elapsed >= Duration::from_millis(40),
                "release was only {:?} after press", elapsed);
        assert!(elapsed <= Duration::from_millis(150),
                "release was {:?} after press (too late)", elapsed);
    }

    #[test]
    fn axis_emits_immediately() {
        let (tx, rx) = crossbeam_channel::unbounded::<InputEvent>();
        let h = handle(tx, "test_agent");
        h.axis(AxisId::LeftStickX, -0.7);
        let ev = rx.recv_timeout(Duration::from_millis(50)).unwrap();
        assert!(matches!(ev.control, Control::Axis(AxisId::LeftStickX)));
        assert!((ev.value - -0.7).abs() < 1e-6);
    }

    #[test]
    fn release_emits_zero_value() {
        let (tx, rx) = crossbeam_channel::unbounded::<InputEvent>();
        let h = handle(tx, "test_agent");
        h.release(ButtonId::South);
        let ev = rx.recv_timeout(Duration::from_millis(50)).unwrap();
        assert_eq!(ev.value, 0.0);
    }

    #[test]
    fn concurrent_press_release_stays_consistent() {
        use std::sync::Arc;
        let (tx, rx) = crossbeam_channel::unbounded::<InputEvent>();
        let h = Arc::new(handle(tx, "concurrent"));
        let mut threads = vec![];
        for _ in 0..4 {
            let h = h.clone();
            threads.push(std::thread::spawn(move || {
                for _ in 0..100 {
                    h.press(ButtonId::South, Duration::from_millis(10));
                }
            }));
        }
        for t in threads { t.join().unwrap(); }
        std::thread::sleep(Duration::from_millis(100));
        let mut press_count = 0;
        let mut release_count = 0;
        while let Ok(ev) = rx.try_recv() {
            if ev.value == 1.0 { press_count += 1; } else { release_count += 1; }
        }
        assert_eq!(press_count, 400);
        assert_eq!(release_count, 400);
    }

    #[test]
    fn release_all_releases_every_held_button() {
        let (tx, rx) = crossbeam_channel::unbounded::<InputEvent>();
        let h = handle(tx, "agent");
        // Hold several buttons by pressing them with a long duration.
        h.press(ButtonId::South, Duration::from_secs(60));
        h.press(ButtonId::East,  Duration::from_secs(60));
        h.press(ButtonId::DPadUp,Duration::from_secs(60));
        // Drain the press events.
        for _ in 0..3 { rx.recv_timeout(Duration::from_millis(50)).unwrap(); }
        // Now release_all.
        h.release_all();
        // Collect release events for ~50ms; expect releases for each held button (3).
        std::thread::sleep(Duration::from_millis(50));
        let mut released = std::collections::HashSet::new();
        while let Ok(ev) = rx.try_recv() {
            if ev.value == 0.0 {
                if let Control::Button(b) = ev.control {
                    released.insert(b);
                }
            }
        }
        assert!(released.contains(&ButtonId::South));
        assert!(released.contains(&ButtonId::East));
        assert!(released.contains(&ButtonId::DPadUp));
    }
}
