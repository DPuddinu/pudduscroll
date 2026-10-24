# PudduScroll

Desktop Instagram carousel composer. Photos sit on a horizontal strip of canvases and can cross the boundaries, so a swipe looks seamless.

PudduScroll is a Rust app. The placement, crop, layering, per-canvas clipping, and export math live in `pudduscroll-core` and are covered by `cargo test`. The window is an eframe/egui binary.

## Build and run

Install the stable Rust toolchain from [rustup](https://rustup.rs). From this directory:

```bash
cargo test
```

### Dev

```bash
cargo run
```

The debug binary is `target/debug/pudduscroll` (`pudduscroll.exe` on Windows).

### Prod

```bash
cargo run --release
```

The release binary is `target/release/pudduscroll` (`pudduscroll.exe` on Windows).

Passing image paths on the command line imports them at startup:

```bash
cargo run -- photo-a.jpg photo-b.png
cargo run --release -- photo-a.jpg photo-b.png
```

### Watch

`cargo watch` rebuilds and relaunches the debug app when a source file changes. Each save quits the window and starts a new process, so photos and layout start over.

```bash
cargo install cargo-watch
cargo watch -x run
```

### Windows

Use the MSVC toolchain (`stable-x86_64-pc-windows-msvc`) and the Visual Studio Build Tools with the Desktop development with C++ workload. Then `cargo run --release`.

### Linux

Debian or Ubuntu:

```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libxkbcommon-dev libwayland-dev libvulkan-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
cargo run --release
```

A Vulkan driver is required (Mesa or the GPU vendor package).

### macOS

```bash
xcode-select --install
cargo run --release
```

## Using it

- Canvas ratios: 1:1, 4:5, 3:4, and 9:16. 3:4 is the modern Instagram portrait. Changing the ratio resizes every canvas and keeps each photo’s place on the strip.
- Up to 20 canvases. The count is shown in the toolbar.
- Import png, jpeg, and webp, including several files at once.
- Drag a photo to move it. Drag a corner or an edge to resize. Scale stays uniform, including across canvas boundaries.
- Crop the selected photo: Original, 1:1, 4:5, 3:4, 3:2, 16:9, 9:16, or freehand. The photo is not stretched.
- The layer list selects a photo and moves it up, down, to the top, or to the bottom. Higher layers draw on top.
- The phone frame previews one exported slide at a time.
- Export writes one image per canvas into a folder you pick. The default width is 1080px. Choose JPEG or PNG. The JPEG slider runs from a small file to quality 100, and PNG is native quality. The size shown is the encoded carousel in megabytes. Files are named `pudduscroll-01.jpg` or `pudduscroll-01.png`.
- Click a photo to select it. Delete removes it. Canvas dividers are only shown while editing.
