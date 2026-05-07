use crossbeam_channel::Sender;
use crate::profile::ProfileId;
use crate::source::DeviceInfo;

#[derive(Debug)]
pub enum ControlMsg {
    SetActiveProfile(ProfileId),
    ListDevices(Sender<Vec<DeviceInfo>>),
    Quit,
}
