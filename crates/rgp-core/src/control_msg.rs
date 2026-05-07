use crossbeam_channel::Sender;
use crate::profile::ProfileId;
use crate::source::DeviceInfo;

#[derive(Debug)]
pub enum ControlMsg {
    SetActiveProfile(ProfileId),
    ListDevices(Sender<Vec<DeviceInfo>>),
    /// Immediately zero the virtual pad and emit a release-all snapshot.
    /// Used as a safety hatch when buttons get stuck or for emergency stop.
    PanicDisconnect,
    Quit,
}
