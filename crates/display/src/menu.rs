#[cfg(windows)]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[cfg(any(windows, test))]
const NEW_CAROUSEL: usize = 0x10;
#[cfg(any(windows, test))]
const SETTINGS: usize = 0x20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowMenuCommand {
    NewCarousel,
    Settings,
}

#[cfg(any(windows, test))]
#[must_use]
pub fn command_from_id(wparam: usize) -> Option<WindowMenuCommand> {
    match wparam & 0xFFF0 {
        NEW_CAROUSEL => Some(WindowMenuCommand::NewCarousel),
        SETTINGS => Some(WindowMenuCommand::Settings),
        _ => None,
    }
}

pub fn install_window_menu(hwnd: isize) -> bool {
    #[cfg(windows)]
    {
        install_windows(hwnd)
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        false
    }
}

#[must_use]
pub fn take_window_menu() -> Option<WindowMenuCommand> {
    #[cfg(windows)]
    {
        command_from_id(PENDING.swap(0, Ordering::AcqRel))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
static INSTALLED: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
static PREVIOUS: AtomicUsize = AtomicUsize::new(0);

#[cfg(windows)]
static PENDING: AtomicUsize = AtomicUsize::new(0);

#[cfg(windows)]
fn install_windows(hwnd: isize) -> bool {
    if INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    // Safety: `hwnd` is the eframe root window. The subclass is installed once and
    // forwards every message except the two added system-menu ids to the previous procedure.
    if unsafe { subclass(hwnd) } {
        INSTALLED.store(true, Ordering::Release);
        true
    } else {
        false
    }
}

#[cfg(windows)]
unsafe fn subclass(hwnd: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GWLP_WNDPROC, GetSystemMenu, InsertMenuW, MF_BYPOSITION, MF_SEPARATOR, MF_STRING,
        SetWindowLongPtrW,
    };
    use windows::core::{PCWSTR, w};

    let hwnd = HWND(hwnd as *mut core::ffi::c_void);
    // Safety: `wnd_proc` matches the Win32 window-procedure calling convention.
    // SetWindowLongPtrW stores that address and returns the previous procedure.
    let previous = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, wnd_proc as *const () as isize) };
    if previous == 0 {
        return false;
    }
    PREVIOUS.store(previous as usize, Ordering::Release);
    let menu = unsafe { GetSystemMenu(hwnd, false) };
    if menu.is_invalid() {
        return true;
    }
    let item = MF_BYPOSITION | MF_STRING;
    let _ = unsafe { InsertMenuW(menu, 0, item, NEW_CAROUSEL, w!("New carousel")) };
    let _ = unsafe { InsertMenuW(menu, 1, item, SETTINGS, w!("Settings")) };
    let _ = unsafe { InsertMenuW(menu, 2, MF_BYPOSITION | MF_SEPARATOR, 0, PCWSTR::null()) };
    true
}

#[cfg(windows)]
unsafe extern "system" fn wnd_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::LRESULT;
    use windows::Win32::UI::WindowsAndMessaging::{CallWindowProcW, DefWindowProcW, WM_SYSCOMMAND};

    if msg == WM_SYSCOMMAND
        && let Some(command) = command_from_id(wparam.0)
    {
        let id = match command {
            WindowMenuCommand::NewCarousel => NEW_CAROUSEL,
            WindowMenuCommand::Settings => SETTINGS,
        };
        PENDING.store(id, Ordering::Release);
        return LRESULT(0);
    }
    let previous = PREVIOUS.load(Ordering::Acquire);
    if previous == 0 {
        // Safety: no previous procedure was stored, so the system procedure handles the message.
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    type Proc = unsafe extern "system" fn(
        windows::Win32::Foundation::HWND,
        u32,
        windows::Win32::Foundation::WPARAM,
        windows::Win32::Foundation::LPARAM,
    ) -> LRESULT;
    // Safety: `previous` is the window procedure address returned by SetWindowLongPtrW for this HWND.
    let previous: Proc = unsafe { std::mem::transmute(previous) };
    // Safety: `previous` is the procedure this subclass replaced, and it accepts this message.
    unsafe { CallWindowProcW(Some(previous), hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::{WindowMenuCommand, command_from_id};

    #[test]
    fn system_menu_ids_ignore_the_low_bits_windows_uses() {
        assert_eq!(
            command_from_id(0x10 | 0x3),
            Some(WindowMenuCommand::NewCarousel)
        );
        assert_eq!(command_from_id(0x20), Some(WindowMenuCommand::Settings));
        assert_eq!(command_from_id(0xF060), None);
        assert_eq!(command_from_id(0), None);
    }
}
