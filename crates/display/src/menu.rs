#[cfg(windows)]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[cfg(any(windows, test))]
const NEW_CAROUSEL: usize = 1001;
#[cfg(any(windows, test))]
const SETTINGS: usize = 1002;
#[cfg(any(windows, test))]
const QUIT: usize = 1003;
#[cfg(any(windows, test))]
const ABOUT: usize = 1004;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowMenuCommand {
    NewCarousel,
    Settings,
    Quit,
    About,
}

#[cfg(any(windows, test))]
#[must_use]
pub fn command_from_id(id: usize) -> Option<WindowMenuCommand> {
    match id {
        NEW_CAROUSEL => Some(WindowMenuCommand::NewCarousel),
        SETTINGS => Some(WindowMenuCommand::Settings),
        QUIT => Some(WindowMenuCommand::Quit),
        ABOUT => Some(WindowMenuCommand::About),
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
    // forwards every message except our File/Help command ids to the previous procedure.
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
        AppendMenuW, CreateMenu, CreatePopupMenu, DrawMenuBar, GWLP_WNDPROC, MF_POPUP,
        MF_SEPARATOR, MF_STRING, SetMenu, SetWindowLongPtrW,
    };
    use windows::core::w;

    let hwnd = HWND(hwnd as *mut core::ffi::c_void);
    // Safety: `wnd_proc` matches the Win32 window-procedure calling convention.
    // SetWindowLongPtrW stores that address and returns the previous procedure.
    let previous = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, wnd_proc as *const () as isize) };
    if previous == 0 {
        return false;
    }
    PREVIOUS.store(previous as usize, Ordering::Release);

    // Safety: CreateMenu/CreatePopupMenu allocate HMENUs owned by this window after SetMenu.
    let Ok(bar) = (unsafe { CreateMenu() }) else {
        return true;
    };
    let Ok(file) = (unsafe { CreatePopupMenu() }) else {
        return true;
    };
    let Ok(help) = (unsafe { CreatePopupMenu() }) else {
        return true;
    };

    let _ = unsafe { AppendMenuW(file, MF_STRING, NEW_CAROUSEL, w!("New carousel")) };
    let _ = unsafe { AppendMenuW(file, MF_STRING, SETTINGS, w!("Settings")) };
    let _ = unsafe { AppendMenuW(file, MF_SEPARATOR, 0, None) };
    let _ = unsafe { AppendMenuW(file, MF_STRING, QUIT, w!("Quit")) };
    let _ = unsafe { AppendMenuW(help, MF_STRING, ABOUT, w!("About")) };
    let _ = unsafe { AppendMenuW(bar, MF_POPUP, file.0 as usize, w!("File")) };
    let _ = unsafe { AppendMenuW(bar, MF_POPUP, help.0 as usize, w!("Help")) };
    let _ = unsafe { SetMenu(hwnd, Some(bar)) };
    let _ = unsafe { DrawMenuBar(hwnd) };
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
    use windows::Win32::UI::WindowsAndMessaging::{CallWindowProcW, DefWindowProcW, WM_COMMAND};

    if msg == WM_COMMAND {
        let id = wparam.0 & 0xFFFF;
        if let Some(command) = command_from_id(id) {
            let stored = match command {
                WindowMenuCommand::NewCarousel => NEW_CAROUSEL,
                WindowMenuCommand::Settings => SETTINGS,
                WindowMenuCommand::Quit => QUIT,
                WindowMenuCommand::About => ABOUT,
            };
            PENDING.store(stored, Ordering::Release);
            return LRESULT(0);
        }
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
    fn menu_command_ids_map_from_wm_command_low_word() {
        assert_eq!(command_from_id(1001), Some(WindowMenuCommand::NewCarousel));
        assert_eq!(command_from_id(1002), Some(WindowMenuCommand::Settings));
        assert_eq!(command_from_id(1003), Some(WindowMenuCommand::Quit));
        assert_eq!(command_from_id(1004), Some(WindowMenuCommand::About));
        assert_eq!(command_from_id(1001 | (1 << 16)), None);
        assert_eq!(command_from_id(0), None);
        assert_eq!(command_from_id(0xF060), None);
    }
}
