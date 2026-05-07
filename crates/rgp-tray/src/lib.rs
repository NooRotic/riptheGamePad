pub mod hotkeys;
pub mod menu;

use crossbeam_channel::Sender;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager};
use rgp_config::HotkeyConfig;
use rgp_core::{ControlMsg, ProfileId, RgpError};
use std::time::Duration;
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem},
    TrayIconBuilder,
};

const TRAY_ICON_PNG: &[u8] = include_bytes!("../../../assets/icons/rip_icon.png");

fn build_icon() -> Result<tray_icon::Icon, RgpError> {
    let img = image::load_from_memory(TRAY_ICON_PNG)
        .map_err(|e| RgpError::Channel(format!("decode icon png: {e}")))?
        .to_rgba8();
    let (w, h) = (img.width(), img.height());
    tray_icon::Icon::from_rgba(img.into_raw(), w, h)
        .map_err(|e| RgpError::Channel(format!("build tray icon: {e}")))
}

pub fn run_on_main(
    control_tx: Sender<ControlMsg>,
    profiles: Vec<ProfileId>,
    hotkey_config: HotkeyConfig,
) -> Result<(), RgpError> {
    if profiles.is_empty() {
        return Err(RgpError::Channel("no profiles configured".into()));
    }

    let tray_menu = Menu::new();

    let profile_items: Vec<CheckMenuItem> = profiles
        .iter()
        .enumerate()
        .map(|(i, p)| CheckMenuItem::new(&p.0, true, i == 0, None))
        .collect();

    for item in &profile_items {
        tray_menu
            .append(item)
            .map_err(|e| RgpError::Channel(format!("menu append: {e}")))?;
    }

    let separator = tray_icon::menu::PredefinedMenuItem::separator();
    let _ = tray_menu.append(&separator);

    let quit = MenuItem::new("Quit", true, None);
    tray_menu
        .append(&quit)
        .map_err(|e| RgpError::Channel(format!("menu append quit: {e}")))?;

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("riptheGamePad")
        .with_icon(build_icon()?)
        .build()
        .map_err(|e| RgpError::Channel(format!("tray build: {e}")))?;

    let manager = GlobalHotKeyManager::new()
        .map_err(|e| RgpError::Channel(format!("hotkey manager: {e}")))?;

    let next_hotkey = hotkeys::parse(&hotkey_config.next_profile)
        .map_err(RgpError::Channel)?;
    let prev_hotkey = hotkeys::parse(&hotkey_config.prev_profile)
        .map_err(RgpError::Channel)?;
    let panic_hotkey = hotkeys::parse(&hotkey_config.panic_disconnect)
        .map_err(RgpError::Channel)?;

    manager
        .register(next_hotkey)
        .ok();
    manager
        .register(prev_hotkey)
        .ok();
    manager
        .register(panic_hotkey)
        .ok();

    let menu_rx = MenuEvent::receiver();
    let hot_rx = GlobalHotKeyEvent::receiver();

    let mut current_idx: usize = 0;
    let _ = control_tx.send(ControlMsg::SetActiveProfile(profiles[0].clone()));

    // Windows requires pumping OS messages to receive tray + hotkey events.
    // tray-icon 0.14 dispatches events to crossbeam channels via an internal
    // hidden window proc. The sleep-poll loop below is sufficient as long as
    // tray-icon's internal pump is active (which it is by default on Windows
    // for 0.14+). If events stop arriving in practice, the commented-out
    // windows-sys message pump below should be uncommented.
    loop {
        if let Ok(ev) = menu_rx.try_recv() {
            if ev.id() == quit.id() {
                tracing::info!(target: "rgp::tray", "quit requested");
                break;
            }
            if let Some(idx) = profile_items.iter().position(|item| item.id() == ev.id()) {
                current_idx = idx;
                update_check_state(&profile_items, idx);
                let _ =
                    control_tx.send(ControlMsg::SetActiveProfile(profiles[idx].clone()));
            }
        }

        if let Ok(ev) = hot_rx.try_recv() {
            if ev.id() == next_hotkey.id() {
                current_idx = (current_idx + 1) % profiles.len();
                update_check_state(&profile_items, current_idx);
                let _ = control_tx
                    .send(ControlMsg::SetActiveProfile(profiles[current_idx].clone()));
            } else if ev.id() == prev_hotkey.id() {
                current_idx = (current_idx + profiles.len() - 1) % profiles.len();
                update_check_state(&profile_items, current_idx);
                let _ = control_tx
                    .send(ControlMsg::SetActiveProfile(profiles[current_idx].clone()));
            } else if ev.id() == panic_hotkey.id() {
                tracing::warn!(target: "rgp::tray", "panic_disconnect triggered");
                let _ = control_tx.send(ControlMsg::PanicDisconnect);
            }
        }

        // If tray click events do not arrive, uncomment this block and add
        // windows-sys = { version = "0.59", features = ["Win32_UI_WindowsAndMessaging"] }
        // to Cargo.toml:
        //
        // #[cfg(windows)] unsafe {
        //     use windows_sys::Win32::UI::WindowsAndMessaging::*;
        //     let mut msg = std::mem::zeroed();
        //     while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
        //         TranslateMessage(&msg);
        //         DispatchMessageW(&msg);
        //     }
        // }

        std::thread::sleep(Duration::from_millis(50));
    }

    let _ = control_tx.send(ControlMsg::Quit);
    Ok(())
}

fn update_check_state(items: &[CheckMenuItem], active: usize) {
    for (i, item) in items.iter().enumerate() {
        item.set_checked(i == active);
    }
}

/// Run the tray in error-only mode. Used when a fatal startup precondition
/// fails (e.g., ViGEmBus not installed). The tray shows a tooltip indicating
/// the error and only offers a Quit menu item; no input/router threads run.
///
/// Blocks the calling thread until the user picks Quit.
///
// run_error_mode is verified manually: uninstall ViGEmBus, run the binary,
// confirm the tray shows up with a red error tooltip and only Quit works.
pub fn run_error_mode(error_msg: String) -> Result<(), RgpError> {
    let menu = Menu::new();
    let header = MenuItem::new(format!("Error: {error_msg}"), false, None);
    menu.append(&header).map_err(|e| RgpError::Channel(format!("menu append: {e}")))?;
    let separator = tray_icon::menu::PredefinedMenuItem::separator();
    let _ = menu.append(&separator);
    let quit = MenuItem::new("Quit", true, None);
    menu.append(&quit).map_err(|e| RgpError::Channel(format!("menu append quit: {e}")))?;

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(format!("riptheGamePad — ERROR: {error_msg}"))
        .with_icon(build_icon()?)
        .build()
        .map_err(|e| RgpError::Channel(format!("tray build: {e}")))?;

    let menu_rx = MenuEvent::receiver();
    loop {
        if let Ok(ev) = menu_rx.try_recv() {
            if ev.id() == quit.id() {
                tracing::info!(target: "rgp::tray", "quit requested from error-mode tray");
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Ok(())
}
