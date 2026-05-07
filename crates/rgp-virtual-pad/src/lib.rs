pub mod translate;

use std::thread::{self, JoinHandle};
use crossbeam_channel::{Receiver, select};
use rgp_core::{PadState, RgpError};
use vigem_client::{Client, Xbox360Wired, TargetId, XGamepad};

/// Trait for delivering a PadState to a virtual gamepad. The real
/// implementation is `ViGEmPad`; tests use `FakePad` to record submissions.
pub trait PadSink: Send {
    fn submit(&mut self, report: XGamepad) -> Result<(), RgpError>;
}

/// Real implementation backed by ViGEmBus.
pub struct ViGEmPad {
    pad: Xbox360Wired<Client>,
}

impl PadSink for ViGEmPad {
    fn submit(&mut self, report: XGamepad) -> Result<(), RgpError> {
        self.pad.update(&report).map_err(|e| RgpError::VirtualPad(format!("{e:?}")))
    }
}

/// Connect to ViGEmBus and plug in a virtual Xbox 360 pad.
/// Returns RgpError::VirtualPad if the driver is missing or the pad fails to plug in.
pub fn connect() -> Result<ViGEmPad, RgpError> {
    let client = Client::connect()
        .map_err(|e| RgpError::VirtualPad(
            format!("ViGEmBus connect failed: {e:?} (is the driver installed?)")))?;
    let mut pad = Xbox360Wired::new(client, TargetId::XBOX360_WIRED);
    pad.plugin().map_err(|e| RgpError::VirtualPad(format!("plugin failed: {e:?}")))?;
    pad.wait_ready().map_err(|e| RgpError::VirtualPad(format!("wait_ready failed: {e:?}")))?;
    Ok(ViGEmPad { pad })
}

pub fn run(
    pad_rx: Receiver<PadState>,
    shutdown: Receiver<()>,
    mut sink: Box<dyn PadSink>,
) -> JoinHandle<Result<(), RgpError>> {
    thread::Builder::new().name("rgp-virtual-pad".into()).spawn(move || -> Result<(), RgpError> {
        loop {
            select! {
                recv(pad_rx) -> msg => match msg {
                    Ok(state) => sink.submit(translate::pad_state_to_xgamepad(&state))?,
                    Err(_) => break,
                },
                recv(shutdown) -> _ => break,
            }
        }
        // Final all-zero release pass so games don't see stuck buttons.
        let _ = sink.submit(translate::pad_state_to_xgamepad(&PadState::default()));
        Ok(())
    }).expect("spawn virtual-pad thread")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use rgp_core::ButtonId;

    pub struct FakePad {
        pub submitted: Arc<Mutex<Vec<XGamepad>>>,
    }
    impl PadSink for FakePad {
        fn submit(&mut self, r: XGamepad) -> Result<(), RgpError> {
            self.submitted.lock().unwrap().push(r);
            Ok(())
        }
    }

    #[test]
    fn run_processes_states_and_releases_on_shutdown() {
        let (pad_tx, pad_rx) = crossbeam_channel::bounded::<PadState>(8);
        let (sd_tx, sd_rx) = crossbeam_channel::bounded::<()>(0);
        let submitted = Arc::new(Mutex::new(vec![]));
        let fake = FakePad { submitted: submitted.clone() };
        let h = run(pad_rx, sd_rx, Box::new(fake));

        let mut s = PadState::default();
        s.buttons.insert(ButtonId::South, true);
        pad_tx.send(s).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        drop(sd_tx);
        h.join().unwrap().unwrap();

        let recorded = submitted.lock().unwrap();
        assert!(recorded.len() >= 2, "expected ≥2 submissions, got {}", recorded.len());
        // Last submission should be the all-zero final release.
        assert_eq!(recorded.last().unwrap().buttons.raw, 0);
    }
}
