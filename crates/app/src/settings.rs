use std::fmt::Write;
use std::path::{Path, PathBuf};

use eframe::egui::{Key, KeyboardShortcut, Modifiers};
use pudduscroll_core::ExportFormat;

use crate::library::library_directory;

pub const QUALITY_MIN: u8 = 0;
pub const QUALITY_MAX: u8 = 100;
pub const DEFAULT_RAIL_WEIGHTS: [f32; 2] = [0.28 / 0.74, 0.46 / 0.74];
const COUNT: usize = 11;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Command {
    NewCarousel = 0,
    Settings = 1,
    Export = 2,
    Undo = 3,
    Redo = 4,
    Delete = 5,
    AddFrame = 6,
    AddCanvas = 7,
    ToggleGrid = 8,
    Home = 9,
    Paste = 10,
}

impl Command {
    pub const ALL: [Self; COUNT] = [
        Self::NewCarousel,
        Self::Settings,
        Self::Export,
        Self::Undo,
        Self::Redo,
        Self::Delete,
        Self::AddFrame,
        Self::AddCanvas,
        Self::ToggleGrid,
        Self::Home,
        Self::Paste,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        spec(self).label
    }

    #[must_use]
    pub const fn id(self) -> &'static str {
        spec(self).id
    }

    #[must_use]
    pub const fn rebindable(self) -> bool {
        spec(self).rebindable
    }

    const fn index(self) -> usize {
        self as usize
    }

    const fn default_shortcut(self) -> KeyboardShortcut {
        let spec = spec(self);
        KeyboardShortcut::new(spec.modifiers, spec.key)
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.id() == id)
    }
}

struct Spec {
    id: &'static str,
    label: &'static str,
    modifiers: Modifiers,
    key: Key,
    rebindable: bool,
}

const fn spec(command: Command) -> Spec {
    match command {
        Command::NewCarousel => Spec {
            id: "new-carousel",
            label: "New carousel",
            modifiers: Modifiers::COMMAND,
            key: Key::N,
            rebindable: true,
        },
        Command::Settings => Spec {
            id: "settings",
            label: "Settings",
            modifiers: Modifiers::COMMAND,
            key: Key::Comma,
            rebindable: true,
        },
        Command::Export => Spec {
            id: "export",
            label: "Export",
            modifiers: Modifiers::COMMAND,
            key: Key::E,
            rebindable: true,
        },
        Command::Undo => Spec {
            id: "undo",
            label: "Undo",
            modifiers: Modifiers::COMMAND,
            key: Key::Z,
            rebindable: true,
        },
        Command::Redo => Spec {
            id: "redo",
            label: "Redo",
            modifiers: Modifiers::COMMAND.plus(Modifiers::SHIFT),
            key: Key::Z,
            rebindable: true,
        },
        Command::Delete => Spec {
            id: "delete",
            label: "Delete",
            modifiers: Modifiers::NONE,
            key: Key::Delete,
            rebindable: true,
        },
        Command::AddFrame => Spec {
            id: "add-frame",
            label: "Add frame",
            modifiers: Modifiers::NONE,
            key: Key::F,
            rebindable: true,
        },
        Command::AddCanvas => Spec {
            id: "add-canvas",
            label: "Add canvas",
            modifiers: Modifiers::NONE,
            key: Key::CloseBracket,
            rebindable: true,
        },
        Command::ToggleGrid => Spec {
            id: "toggle-grid",
            label: "Toggle grid",
            modifiers: Modifiers::NONE,
            key: Key::G,
            rebindable: true,
        },
        Command::Home => Spec {
            id: "home",
            label: "Home",
            modifiers: Modifiers::COMMAND,
            key: Key::H,
            rebindable: true,
        },
        Command::Paste => Spec {
            id: "paste",
            label: "Paste image",
            modifiers: Modifiers::COMMAND,
            key: Key::V,
            rebindable: false,
        },
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppSettings {
    pub quality: u8,
    pub format: ExportFormat,
    pub folder: Option<PathBuf>,
    pub rail: [f32; 2],
    bindings: [KeyboardShortcut; COUNT],
}

impl Default for AppSettings {
    fn default() -> Self {
        let mut bindings = [KeyboardShortcut::new(Modifiers::NONE, Key::A); COUNT];
        for command in Command::ALL {
            bindings[command.index()] = command.default_shortcut();
        }
        Self {
            quality: 90,
            format: ExportFormat::Jpeg,
            folder: None,
            rail: DEFAULT_RAIL_WEIGHTS,
            bindings,
        }
    }
}

impl AppSettings {
    #[must_use]
    pub fn load() -> Self {
        Self::load_from(&library_directory().join("settings.txt"))
    }

    #[must_use]
    pub fn load_from(path: &Path) -> Self {
        let Ok(text) = crate::disk::read_to_string(path) else {
            return Self::default();
        };
        Self::parse(&text)
    }

    pub fn save(&self) -> Result<(), String> {
        let root = library_directory();
        crate::disk::create_dir_all(&root).map_err(|error| error.to_string())?;
        self.save_to(&root.join("settings.txt"))
    }

    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            crate::disk::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let temporary = path.with_extension("txt.tmp");
        crate::disk::write(&temporary, self.encode()).map_err(|error| error.to_string())?;
        let _ = crate::disk::remove_file(path);
        crate::disk::rename(&temporary, path).map_err(|error| error.to_string())?;
        crate::disk::sync();
        Ok(())
    }

    #[must_use]
    pub fn binding(&self, command: Command) -> KeyboardShortcut {
        self.bindings[command.index()]
    }

    #[must_use]
    pub fn owner(&self, shortcut: KeyboardShortcut) -> Option<Command> {
        let shortcut = normalize_shortcut(shortcut);
        Command::ALL
            .into_iter()
            .find(|command| self.binding(*command) == shortcut)
    }

    pub fn assign(&mut self, command: Command, shortcut: KeyboardShortcut) -> Result<(), Command> {
        if !command.rebindable() {
            return Err(command);
        }
        let shortcut = normalize_shortcut(shortcut);
        if let Some(owner) = self.owner(shortcut)
            && owner != command
        {
            return Err(owner);
        }
        self.bindings[command.index()] = shortcut;
        Ok(())
    }

    fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once(' ') else {
                continue;
            };
            let value = value.trim();
            match key {
                "quality" => {
                    if let Ok(quality) = value.parse::<u8>() {
                        settings.quality = quality.min(QUALITY_MAX);
                    }
                }
                "format" => match value {
                    "jpeg" | "jpg" => settings.format = ExportFormat::Jpeg,
                    "png" => settings.format = ExportFormat::Png,
                    _ => {}
                },
                "folder" => {
                    if !value.is_empty() {
                        settings.folder = Some(PathBuf::from(value));
                    }
                }
                "rail" => {
                    let mut parts = value.split_whitespace();
                    let Some(tool) = parts.next().and_then(|part| part.parse::<f32>().ok()) else {
                        continue;
                    };
                    let Some(layers) = parts.next().and_then(|part| part.parse::<f32>().ok())
                    else {
                        continue;
                    };
                    if tool.is_finite() && layers.is_finite() && tool > 0.0 && layers > 0.0 {
                        let sum = tool + layers;
                        settings.rail = [tool / sum, layers / sum];
                    }
                }
                "bind" => {
                    let Some((id, shortcut)) = value.split_once(' ') else {
                        continue;
                    };
                    let Some(command) = Command::from_id(id.trim()) else {
                        continue;
                    };
                    if !command.rebindable() {
                        continue;
                    }
                    if let Some(shortcut) = parse_shortcut(shortcut.trim()) {
                        let _ = settings.assign(command, shortcut);
                    }
                }
                _ => {}
            }
        }
        settings
    }

    fn encode(&self) -> String {
        let mut text = String::new();
        let _ = writeln!(text, "quality {}", self.quality);
        let format = match self.format {
            ExportFormat::Jpeg => "jpeg",
            ExportFormat::Png => "png",
        };
        let _ = writeln!(text, "format {format}");
        if let Some(folder) = &self.folder {
            let _ = writeln!(text, "folder {}", folder.display());
        }
        let _ = writeln!(text, "rail {:.6} {:.6}", self.rail[0], self.rail[1]);
        for command in Command::ALL {
            if !command.rebindable() {
                continue;
            }
            let _ = writeln!(
                text,
                "bind {} {}",
                command.id(),
                encode_shortcut(self.binding(command))
            );
        }
        text
    }
}

#[must_use]
pub fn normalize_shortcut(shortcut: KeyboardShortcut) -> KeyboardShortcut {
    let modifiers = shortcut.modifiers;
    let modifiers = if modifiers.command || modifiers.mac_cmd {
        Modifiers {
            alt: modifiers.alt,
            ctrl: false,
            shift: modifiers.shift,
            mac_cmd: false,
            command: true,
        }
    } else {
        Modifiers {
            alt: modifiers.alt,
            ctrl: modifiers.ctrl,
            shift: modifiers.shift,
            mac_cmd: false,
            command: false,
        }
    };
    KeyboardShortcut::new(modifiers, shortcut.logical_key)
}

#[must_use]
pub fn shortcut_rank(shortcut: KeyboardShortcut) -> u8 {
    let modifiers = shortcut.modifiers;
    u8::from(modifiers.command)
        + u8::from(modifiers.ctrl)
        + u8::from(modifiers.alt)
        + u8::from(modifiers.shift)
        + u8::from(modifiers.mac_cmd)
}

#[must_use]
pub fn redo_alias() -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::COMMAND, Key::Y)
}

fn encode_shortcut(shortcut: KeyboardShortcut) -> String {
    let shortcut = normalize_shortcut(shortcut);
    let mut parts = Vec::new();
    if shortcut.modifiers.command {
        parts.push("command");
    }
    if shortcut.modifiers.ctrl {
        parts.push("ctrl");
    }
    if shortcut.modifiers.alt {
        parts.push("alt");
    }
    if shortcut.modifiers.shift {
        parts.push("shift");
    }
    parts.push(shortcut.logical_key.name());
    parts.join("+")
}

fn parse_shortcut(text: &str) -> Option<KeyboardShortcut> {
    let mut modifiers = Modifiers::NONE;
    let mut key = None;
    for part in text.split('+') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match part {
            "command" => modifiers.command = true,
            "ctrl" => modifiers.ctrl = true,
            "alt" => modifiers.alt = true,
            "shift" => modifiers.shift = true,
            "mac" => modifiers.mac_cmd = true,
            other => key = Key::from_name(other),
        }
    }
    key.map(|key| normalize_shortcut(KeyboardShortcut::new(modifiers, key)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_order_matches_the_binding_slots() {
        assert_eq!(Command::ALL.len(), COUNT);
        for (index, command) in Command::ALL.into_iter().enumerate() {
            assert_eq!(command.index(), index);
            assert_ne!(command.id(), "");
            assert_ne!(command.label(), "");
        }
    }

    #[test]
    fn settings_roundtrip_keeps_export_defaults_and_bindings() {
        let dir = std::env::temp_dir().join(format!("pudduscroll-settings-{}", std::process::id()));
        let _ = crate::disk::remove_dir_all(&dir);
        crate::disk::create_dir_all(&dir).unwrap();
        let mut settings = AppSettings {
            quality: 70,
            format: ExportFormat::Png,
            folder: Some(dir.join("My Photos")),
            ..AppSettings::default()
        };
        let shortcut = KeyboardShortcut::new(Modifiers::ALT.plus(Modifiers::SHIFT), Key::E);
        settings.assign(Command::Export, shortcut).unwrap();
        settings.rail = [0.25, 0.75];
        let path = dir.join("settings.txt");
        settings.save_to(&path).unwrap();
        let loaded = AppSettings::load_from(&path);
        assert_eq!(loaded.quality, 70);
        assert_eq!(loaded.format, ExportFormat::Png);
        assert_eq!(loaded.folder, Some(dir.join("My Photos")));
        assert!((loaded.rail[0] - 0.25).abs() < 1e-4);
        assert!((loaded.rail[1] - 0.75).abs() < 1e-4);
        assert!(loaded.binding(Command::Export).modifiers.alt);
        assert!(loaded.binding(Command::Export).modifiers.shift);
        assert_eq!(loaded.binding(Command::Export).logical_key, Key::E);
        assert_eq!(
            loaded.binding(Command::Undo),
            AppSettings::default().binding(Command::Undo)
        );
        assert_eq!(loaded, AppSettings::parse(&loaded.encode()));
        let _ = crate::disk::remove_dir_all(&dir);
    }

    #[test]
    fn assign_rejects_a_shortcut_another_command_uses() {
        let mut settings = AppSettings::default();
        let undo = settings.binding(Command::Undo);
        assert_eq!(settings.assign(Command::Export, undo), Err(Command::Undo));
        assert_eq!(
            settings.binding(Command::Export),
            AppSettings::default().binding(Command::Export)
        );
    }

    #[test]
    fn captured_ctrl_stores_command() {
        let mut settings = AppSettings::default();
        let raw = KeyboardShortcut::new(
            Modifiers {
                ctrl: true,
                command: true,
                ..Modifiers::NONE
            },
            Key::E,
        );
        settings.assign(Command::Export, raw).unwrap();
        let stored = settings.binding(Command::Export);
        assert!(stored.modifiers.command);
        assert!(!stored.modifiers.ctrl);
        assert_eq!(stored.logical_key, Key::E);
    }

    #[test]
    fn broken_lines_keep_the_other_defaults() {
        let settings = AppSettings::parse(
            "\
quality nope
format gif
folder
bind paste command+q
bind export command+z
bind missing command+a
quality 40
rail nope
rail 0
",
        );
        assert_eq!(settings.quality, 40);
        assert_eq!(settings.format, ExportFormat::Jpeg);
        assert!(settings.folder.is_none());
        assert_eq!(settings.rail, DEFAULT_RAIL_WEIGHTS);
        let defaults = AppSettings::default();
        assert_eq!(
            settings.binding(Command::Paste),
            defaults.binding(Command::Paste)
        );
        assert_eq!(
            settings.binding(Command::Export),
            defaults.binding(Command::Export)
        );
    }

    #[test]
    fn missing_file_uses_jpeg_at_ninety() {
        let path = std::env::temp_dir().join(format!(
            "pudduscroll-settings-missing-{}-{}.txt",
            std::process::id(),
            "absent"
        ));
        let _ = crate::disk::remove_file(&path);
        let settings = AppSettings::load_from(&path);
        assert_eq!(settings.quality, 90);
        assert_eq!(settings.format, ExportFormat::Jpeg);
        assert!(settings.folder.is_none());
        assert_eq!(
            settings.binding(Command::AddCanvas).logical_key,
            Key::CloseBracket
        );
    }
}
