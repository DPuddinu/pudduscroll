mod menu;

#[cfg(windows)]
use std::path::{Path, PathBuf};

pub use menu::{WindowMenuCommand, install_window_menu, take_window_menu};

pub fn profile_bytes(device: Option<&str>) -> Option<Vec<u8>> {
    let bytes = platform_profile(device)?;
    if bytes.len() > 128 && bytes.get(36..40) == Some(b"acsp") {
        Some(bytes)
    } else {
        None
    }
}

pub fn compositor_is_srgb(device: Option<&str>) -> bool {
    #[cfg(windows)]
    {
        advanced_color(device).unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        let _ = device;
        false
    }
}

#[cfg(any(
    test,
    all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    ),
))]
struct DisplayRecord {
    names: Vec<String>,
    kind: String,
    enabled: bool,
    file: Option<String>,
}

#[cfg(any(
    test,
    all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    ),
))]
fn names_output(name: &str, output: &str) -> bool {
    name == output
        || name
            .strip_prefix("xrandr-")
            .is_some_and(|rest| rest == output)
        || name
            .strip_prefix("wayland-")
            .is_some_and(|rest| rest == output)
}

#[cfg(any(
    test,
    all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    ),
))]
fn preferred_file<'a>(records: &'a [DisplayRecord], output: Option<&str>) -> Option<&'a str> {
    let mut fallback = None;
    for record in records {
        if record.kind != "display" || !record.enabled {
            continue;
        }
        let Some(file) = record.file.as_deref() else {
            continue;
        };
        if record
            .names
            .iter()
            .any(|name| output.is_some_and(|output| names_output(name, output)))
        {
            return Some(file);
        }
        if fallback.is_none() {
            fallback = Some(file);
        }
    }
    fallback
}

#[cfg(any(test, windows))]
fn advanced_color_is_active(flags: u32) -> bool {
    const ENABLED: u32 = 1 << 1;
    const FORCED_OFF: u32 = 1 << 3;
    flags & FORCED_OFF == 0 && flags & ENABLED != 0
}

#[cfg(windows)]
fn platform_profile(device: Option<&str>) -> Option<Vec<u8>> {
    let path = device
        .and_then(|device| profile_path(Some(device)))
        .or_else(|| profile_path(None))?;
    read_profile(&path)
}

#[cfg(windows)]
fn read_profile(path: &Path) -> Option<Vec<u8>> {
    if let Ok(bytes) = std::fs::read(path) {
        return Some(bytes);
    }
    let name = path.file_name()?;
    let root = std::env::var_os("SystemRoot")?;
    std::fs::read(
        PathBuf::from(root)
            .join("System32")
            .join("spool")
            .join("drivers")
            .join("color")
            .join(name),
    )
    .ok()
}

#[cfg(windows)]
fn profile_path(device: Option<&str>) -> Option<PathBuf> {
    let driver = device.unwrap_or("DISPLAY");
    let wide: Vec<u16> = driver.encode_utf16().chain(std::iter::once(0)).collect();
    // Safety: `wide` is null-terminated and outlives the DC. The DC is deleted before return.
    // The filename buffer is writable for the character count GDI reports.
    unsafe {
        use windows::Win32::Graphics::Gdi::{CreateDCW, DeleteDC};
        use windows::Win32::UI::ColorSystem::GetICMProfileW;
        use windows::core::{PCWSTR, PWSTR};

        let hdc = CreateDCW(PCWSTR(wide.as_ptr()), PCWSTR::null(), PCWSTR::null(), None);
        if hdc.is_invalid() {
            return None;
        }
        let mut size = 0u32;
        let _ = GetICMProfileW(hdc, &mut size, None);
        if size < 2 {
            let _ = DeleteDC(hdc);
            return None;
        }
        let mut buffer = vec![0u16; size as usize];
        let ok = GetICMProfileW(hdc, &mut size, Some(PWSTR(buffer.as_mut_ptr())));
        let _ = DeleteDC(hdc);
        if !ok.as_bool() {
            return None;
        }
        let end = buffer
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(buffer.len());
        let name = String::from_utf16(&buffer[..end]).ok()?;
        if name.is_empty() {
            None
        } else {
            Some(PathBuf::from(name))
        }
    }
}

#[cfg(windows)]
fn advanced_color(device: Option<&str>) -> Option<bool> {
    let paths = active_paths()?;
    let mut saw = false;
    let mut all_managed = true;
    for path in &paths {
        let Some(name) = source_gdi_name(path) else {
            continue;
        };
        if device.is_some_and(|device| !name.eq_ignore_ascii_case(device)) {
            continue;
        }
        let managed = target_is_managed(path);
        if device.is_some() {
            return Some(managed);
        }
        saw = true;
        all_managed &= managed;
    }
    saw.then_some(all_managed)
}

#[cfg(windows)]
fn active_paths() -> Option<Vec<windows::Win32::Devices::Display::DISPLAYCONFIG_PATH_INFO>> {
    use windows::Win32::Devices::Display::{
        DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, GetDisplayConfigBufferSizes,
        QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
    };
    use windows::Win32::Foundation::ERROR_SUCCESS;

    // Safety: the path and mode buffers are live for the call, and their lengths are the counts
    // passed to QueryDisplayConfig. A null topology id is valid for active-path queries.
    unsafe {
        for _ in 0..2 {
            let mut path_count = 0u32;
            let mut mode_count = 0u32;
            if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
                != ERROR_SUCCESS
                || path_count == 0
            {
                return None;
            }
            let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
            let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
            let status = QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut path_count,
                paths.as_mut_ptr(),
                &mut mode_count,
                modes.as_mut_ptr(),
                None,
            );
            if status == ERROR_SUCCESS {
                paths.truncate(path_count as usize);
                return Some(paths);
            }
        }
        None
    }
}

#[cfg(windows)]
fn source_gdi_name(
    path: &windows::Win32::Devices::Display::DISPLAYCONFIG_PATH_INFO,
) -> Option<String> {
    use std::mem::size_of;

    use windows::Win32::Devices::Display::{
        DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER,
        DISPLAYCONFIG_SOURCE_DEVICE_NAME, DisplayConfigGetDeviceInfo,
    };

    let mut name = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
            size: u32::try_from(size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>()).unwrap_or(0),
            adapterId: path.sourceInfo.adapterId,
            id: path.sourceInfo.id,
        },
        ..DISPLAYCONFIG_SOURCE_DEVICE_NAME::default()
    };
    // Safety: `header` is the first field of `name`, and `header.size` is the size of that whole
    // object. `name` stays live for the call.
    let status = unsafe { DisplayConfigGetDeviceInfo(&mut name.header) };
    if status != 0 {
        return None;
    }
    let end = name
        .viewGdiDeviceName
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(name.viewGdiDeviceName.len());
    let text = String::from_utf16(&name.viewGdiDeviceName[..end]).ok()?;
    if text.is_empty() { None } else { Some(text) }
}

#[cfg(windows)]
fn target_is_managed(path: &windows::Win32::Devices::Display::DISPLAYCONFIG_PATH_INFO) -> bool {
    use std::mem::size_of;

    use windows::Win32::Devices::Display::{
        DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_DEVICE_INFO_HEADER,
        DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO, DisplayConfigGetDeviceInfo,
    };

    let mut info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
            size: u32::try_from(size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>()).unwrap_or(0),
            adapterId: path.targetInfo.adapterId,
            id: path.targetInfo.id,
        },
        ..DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO::default()
    };
    // Safety: `header` is the first field of `info`, and `header.size` is the size of that whole
    // object. `info` stays live for the call. The flag word is the union's integer view.
    unsafe {
        if DisplayConfigGetDeviceInfo(&mut info.header) != 0 {
            return false;
        }
        advanced_color_is_active(info.Anonymous.value)
    }
}

#[cfg(target_os = "macos")]
fn platform_profile(device: Option<&str>) -> Option<Vec<u8>> {
    let chosen = device
        .and_then(|device| device.parse::<u32>().ok())
        .filter(|id| *id != 0);
    // Safety: each color space and profile blob is released before the next call or return.
    unsafe {
        let main = CGMainDisplayID();
        let id = chosen.unwrap_or(main);
        copy_icc(id).or_else(|| if id == main { None } else { copy_icc(main) })
    }
}

#[cfg(target_os = "macos")]
unsafe fn copy_icc(display: u32) -> Option<Vec<u8>> {
    // Safety: the color space and profile blob created here are released before return. The byte
    // pointer is only read for the length CoreFoundation reports, and those bytes are copied first.
    unsafe {
        let space = CGDisplayCopyColorSpace(display);
        if space.is_null() {
            return None;
        }
        let data = CGColorSpaceCopyICCData(space);
        CFRelease(space);
        if data.is_null() {
            return None;
        }
        let len = CFDataGetLength(data);
        if len <= 0 {
            CFRelease(data);
            return None;
        }
        let ptr = CFDataGetBytePtr(data);
        if ptr.is_null() {
            CFRelease(data);
            return None;
        }
        let bytes = std::slice::from_raw_parts(ptr, len as usize).to_vec();
        CFRelease(data);
        Some(bytes)
    }
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGDisplayCopyColorSpace(display: u32) -> *mut std::ffi::c_void;
    fn CGColorSpaceCopyICCData(space: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataGetBytePtr(data: *mut std::ffi::c_void) -> *const u8;
    fn CFDataGetLength(data: *mut std::ffi::c_void) -> isize;
    fn CFRelease(value: *mut std::ffi::c_void);
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
fn platform_profile(device: Option<&str>) -> Option<Vec<u8>> {
    colord_profile(device).or_else(x11_profile)
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
fn colord_profile(device: Option<&str>) -> Option<Vec<u8>> {
    let records = colord_records()?;
    let path = preferred_file(&records, device)?;
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > 128 && bytes.get(36..40) == Some(b"acsp") {
        Some(bytes)
    } else {
        None
    }
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
fn colord_records() -> Option<Vec<DisplayRecord>> {
    use zbus::blocking::Connection;
    use zbus::zvariant::OwnedObjectPath;

    let connection = Connection::session().ok()?;
    let reply = connection
        .call_method(
            Some("org.freedesktop.ColorManager"),
            "/org/freedesktop/ColorManager",
            Some("org.freedesktop.ColorManager"),
            "GetDevices",
            &(),
        )
        .ok()?;
    let paths: Vec<OwnedObjectPath> = reply.body().deserialize().ok()?;
    let mut records = Vec::new();
    for path in paths {
        let Some(record) = device_record(&connection, &path) else {
            continue;
        };
        records.push(record);
    }
    Some(records)
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
fn device_record(
    connection: &zbus::blocking::Connection,
    path: &zbus::zvariant::OwnedObjectPath,
) -> Option<DisplayRecord> {
    use std::collections::HashMap;

    use zbus::zvariant::OwnedValue;

    let reply = connection
        .call_method(
            Some("org.freedesktop.ColorManager"),
            path.as_str(),
            Some("org.freedesktop.DBus.Properties"),
            "GetAll",
            &("org.freedesktop.ColorManager.Device",),
        )
        .ok()?;
    let props: HashMap<String, OwnedValue> = reply.body().deserialize().ok()?;
    let kind = owned_string(props.get("Kind")?)?;
    let enabled = owned_bool(props.get("Enabled")?)?;
    let mut names = Vec::new();
    if let Some(id) = props.get("Id").and_then(owned_string) {
        if !id.is_empty() {
            names.push(id);
        }
    }
    if let Some(model) = props.get("Model").and_then(owned_string) {
        if !model.is_empty() {
            names.push(model);
        }
    }
    if let Some(metadata) = props.get("Metadata").and_then(owned_map) {
        names.extend(metadata.into_values().filter(|name| !name.is_empty()));
    }
    let file = props
        .get("Profiles")
        .and_then(owned_paths)
        .and_then(|profiles| profiles.first().cloned())
        .and_then(|profile| profile_filename(connection, &profile));
    Some(DisplayRecord {
        names,
        kind,
        enabled,
        file,
    })
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
fn profile_filename(
    connection: &zbus::blocking::Connection,
    path: &zbus::zvariant::OwnedObjectPath,
) -> Option<String> {
    use zbus::zvariant::OwnedValue;

    let reply = connection
        .call_method(
            Some("org.freedesktop.ColorManager"),
            path.as_str(),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.ColorManager.Profile", "Filename"),
        )
        .ok()?;
    let value: OwnedValue = reply.body().deserialize().ok()?;
    let name = String::try_from(value).ok()?;
    let name = name.strip_prefix("file://").unwrap_or(&name);
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
fn owned_string(value: &zbus::zvariant::OwnedValue) -> Option<String> {
    String::try_from(value.try_clone().ok()?).ok()
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
fn owned_bool(value: &zbus::zvariant::OwnedValue) -> Option<bool> {
    bool::try_from(value.try_clone().ok()?).ok()
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
fn owned_map(
    value: &zbus::zvariant::OwnedValue,
) -> Option<std::collections::HashMap<String, String>> {
    std::collections::HashMap::try_from(value.try_clone().ok()?).ok()
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
fn owned_paths(value: &zbus::zvariant::OwnedValue) -> Option<Vec<zbus::zvariant::OwnedObjectPath>> {
    Vec::try_from(value.try_clone().ok()?).ok()
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
fn x11_profile() -> Option<Vec<u8>> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::ConnectionExt;

    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots.get(screen)?.root;
    let atom = conn
        .intern_atom(true, b"_ICC_PROFILE")
        .ok()?
        .reply()
        .ok()?
        .atom;
    if atom == x11rb::NONE {
        return None;
    }
    let mut length = 65_536u32;
    loop {
        let reply = conn
            .get_property(false, root, atom, x11rb::NONE, 0, length)
            .ok()?
            .reply()
            .ok()?;
        if reply.format != 8 {
            return None;
        }
        if reply.bytes_after == 0 {
            return Some(reply.value);
        }
        let next = length.saturating_add(reply.bytes_after.div_ceil(4));
        if next == length || next > 4_000_000 {
            return None;
        }
        length = next;
    }
}

#[cfg(not(any(
    windows,
    target_os = "macos",
    all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    ),
)))]
fn platform_profile(_device: Option<&str>) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::{DisplayRecord, advanced_color_is_active, preferred_file};

    #[test]
    fn system_display_profile_is_icc_when_present() {
        let _ = crate::compositor_is_srgb(None);
        let Some(bytes) = crate::profile_bytes(None) else {
            return;
        };
        assert!(bytes.len() > 128);
        assert_eq!(&bytes[36..40], b"acsp");
    }

    #[test]
    fn advanced_color_flags_follow_the_enabled_bits() {
        assert!(!advanced_color_is_active(0));
        assert!(!advanced_color_is_active(1));
        assert!(!advanced_color_is_active(0x5));
        assert!(advanced_color_is_active(1 << 1));
        assert!(!advanced_color_is_active(1 << 2));
        assert!(!advanced_color_is_active((1 << 1) | (1 << 3)));
    }

    #[test]
    fn preferred_display_matches_the_output_name() {
        let records = vec![
            display(&["eDP-1", "xrandr-eDP-1"], "/icc/internal.icc"),
            display(&["xrandr-HDMI-A-1"], "/icc/external.icc"),
            DisplayRecord {
                names: vec!["eDP-10".to_string()],
                kind: "display".to_string(),
                enabled: true,
                file: Some("/icc/other.icc".to_string()),
            },
            DisplayRecord {
                names: vec!["printer".to_string()],
                kind: "printer".to_string(),
                enabled: true,
                file: Some("/icc/printer.icc".to_string()),
            },
            DisplayRecord {
                names: vec!["eDP-2".to_string()],
                kind: "display".to_string(),
                enabled: false,
                file: Some("/icc/disabled.icc".to_string()),
            },
        ];
        assert_eq!(
            preferred_file(&records, Some("HDMI-A-1")),
            Some("/icc/external.icc")
        );
        assert_eq!(
            preferred_file(&records, Some("eDP-1")),
            Some("/icc/internal.icc")
        );
        assert_eq!(
            preferred_file(&records, Some("eDP-10")),
            Some("/icc/other.icc")
        );
        assert_eq!(preferred_file(&records, None), Some("/icc/internal.icc"));
        assert_eq!(
            preferred_file(&records, Some("missing")),
            Some("/icc/internal.icc")
        );
    }

    fn display(names: &[&str], file: &str) -> DisplayRecord {
        DisplayRecord {
            names: names.iter().map(|name| (*name).to_string()).collect(),
            kind: "display".to_string(),
            enabled: true,
            file: Some(file.to_string()),
        }
    }
}
