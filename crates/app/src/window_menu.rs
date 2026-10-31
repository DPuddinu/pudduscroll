pub use pudduscroll_display::WindowMenuCommand as SystemCommand;

pub fn install(frame: &eframe::Frame) {
    #[cfg(windows)]
    install_windows(frame);
    #[cfg(not(windows))]
    let _ = frame;
}

#[must_use]
pub fn take() -> Option<SystemCommand> {
    pudduscroll_display::take_window_menu()
}

#[cfg(windows)]
fn install_windows(frame: &eframe::Frame) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = frame.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(window) = handle.as_raw() else {
        return;
    };
    let _ = pudduscroll_display::install_window_menu(window.hwnd.get());
}
