PudduScroll 0.6.0 adds a 16:9 canvas and lines up the cards on Home.

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

- A 16:9 canvas is in the ratio list.
- Cover photos fill the home card.
- Captions on a row of cards start at the same left edge.
- Move left or right swaps two frames that span more than one canvas.
- A wide frame steps into the empty canvas past its edge.
