PudduScroll 0.4.0 lets you rotate frames and resize the side panels.

## Downloads

| File | System |
| --- | --- |
| `pudduscroll-windows-x86_64.zip` | Windows 10 or later, 64-bit. Unzip and run `pudduscroll.exe`. |
| `pudduscroll-macos-aarch64.zip` | macOS on Apple silicon. Unzip and open `PudduScroll.app`. |
| `pudduscroll-macos-x86_64.zip` | macOS on Intel. Unzip and open `PudduScroll.app`. |
| `pudduscroll-linux-x86_64.tar.gz` | 64-bit Linux. Built on Ubuntu 24.04. |

These builds are unsigned. Windows SmartScreen and macOS Gatekeeper ask for confirmation. On a Mac, right-click `PudduScroll.app` and choose Open.

Linux needs a Vulkan driver and these packages:

```bash
sudo apt-get install -y libxkbcommon0 libwayland-client0 libvulkan1 libxcb-render0 libxcb-shape0 libxcb-xfixes0
tar -xzf pudduscroll-linux-x86_64.tar.gz
chmod +x pudduscroll
./pudduscroll
```

Templates and carousels are saved on this machine:

- Windows: `%LOCALAPPDATA%\pudduscroll`
- macOS: `~/Library/Application Support/pudduscroll`
- Linux: `$XDG_DATA_HOME/pudduscroll`, or `~/.local/share/pudduscroll`

## New

- Rotate a frame with the handle on the canvas or the rotate buttons in the toolbar.
- On Windows, the window uses a custom title bar with File and Help menus.
- Drag the split between Tools and Layers to change how much space each uses; your choice is remembered.
- The canvas count in the toolbar is shown in a tighter format.
