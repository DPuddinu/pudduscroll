# PudduScroll

PudduScroll is a desktop app for composing Instagram carousels. A carousel is a horizontal strip of canvases. Frames sit on that strip, can cross the gaps between canvases, and each one holds a photo. Export writes one image per canvas, clipped where the frames meet the slide edges, so a swipe stays continuous.

You design the layout once as a template, then open it into as many carousels as you need. Photos, crops, and names stay with each carousel. The library is saved on disk and is still there the next time you open the app.

## Library

The home screen is two collections.

**Templates** are empty layouts. Each one has a name, a cover image, a canvas ratio, a canvas count, and a set of frames. Open a template card to start a new carousel from that layout. Edit changes the template itself. Delete removes it. Frames in a template stay empty.

**Carousels** are the pieces you fill and export. A card shows a preview of the first slide. Rename a carousel in place on its card. Duplicate copies the layout, the photos, and the grid. Delete removes that carousel only.

New template asks for a name, a cover (png, jpeg, or webp), and a canvas ratio, then opens the template editor.

## The strip

The editor shows every canvas in a row, with a divider at each boundary. Frames can sit across those dividers. What you see on the strip is what export clips into each slide.

- Scroll to zoom the strip. Hold Alt and scroll to move along it.
- Hold Space and drag to pan.
- Click empty space to clear the selection.
- The right rail holds Canvas, Frame, Photo, Crop, and Layers. Drag the splits between those sections to give one of them more room.

## Canvas

Four ratios: 1:1, 4:5, 3:4, and 9:16. 4:5 is the default for a new template. 3:4 is the taller Instagram portrait. Changing the ratio resizes every canvas and leaves each frame where it sits on the strip.

Add and remove canvases, from 1 up to 20. The count is shown as `03 / 20`. The pixel size under the ratio is the size export will write.

## Frames

Add frame drops a new frame onto the strip, one canvas wide. A new frame lands on the next canvas and starts on top.

Edit frame turns on move and resize for the selected frame.

- Drag the frame to move it, including across canvas boundaries.
- Drag a corner to resize and keep its shape. Drag an edge to change one side.
- The ratio buttons (1:1, 4:5, 3:4, 9:16) set that frame’s shape.
- Remove frame deletes the selected frame. Delete does the same when the frame has no photo.

Grid draws an alignment grid on every canvas, from 2 to 16 squares. Snap, with the grid on, pulls a frame onto that grid when you release a move or a resize.

## Photos

On a carousel, an empty frame shows a plus. Click it and pick a png, jpeg, or webp. The photo fills the frame. A spinner sits on the frame while it loads.

With Edit frame off, drag a photo to slide it inside the frame. With Edit frame on, scrolling over the selected photo zooms it toward the pointer, from 1× up to 8×. The point under the cursor stays put.

Rotate by 90° counterclockwise or clockwise, or turn on Free and drag the handle for any angle. Remove photo clears the picture and leaves the frame. Delete does the same when a photo is selected.

A photo can have a border. The slider and the percent field run from 0 to 100. The swatch opens a color picker. The border is drawn inside the frame and is clipped with the slide.

## Crop

Crop applies to the selected photo on a carousel. A template has nothing to crop.

Presets: Original, 1:1, 4:5, 3:4, 3:2, 16:9, 9:16, and Freehand. A locked preset keeps that shape while you drag. Freehand does not. Original restores the full picture. The photo is never stretched. The panel shows the visible aspect, such as `Visible 0.80`.

Enter keeps the crop. Escape cancels it. Undo cancels an open crop before it steps backward through edits.

## Layers

Every frame is a layer. The list is front to back: the top row draws on top and wins the click. Hover a row to move that frame up, down, to the front, or to the back. Click a row to select it. Each row shows the photo, cropped the way the frame shows it, or the word Frame when the slot is empty.

## Editing

Undo is Ctrl+Z (Cmd+Z on macOS). Redo is Ctrl+Y or Ctrl+Shift+Z. A drag, a zoom, a border change, and a grid change each become one step, so a single gesture does not flood the history.

The window remembers nothing of its own. Your templates and carousels are written to the library as you go, and again when the app closes.

## Export

Export is enabled once a carousel has a photo. The dialog previews one slide at a time. Step through it with the arrows or with Left and Right.

Choose JPEG or PNG. The JPEG slider runs from a small file to quality 100. PNG is the source rendered with no extra compression. The dialog estimates the encoded size of the whole carousel and shows it in megabytes.

Choose folder picks the destination. If a file with the same name is already there, PudduScroll lists it and asks before overwriting.

Each canvas is one file, named from the carousel: `Summer-01.jpg`, `Summer-02.jpg`, and so on. The width is the largest size that still uses each photo’s own pixels, so a slide is not blown up past the crop. With no usable photo, the width is 1080px. Height follows the canvas ratio.

On screen, photos are shown in the display’s color profile, using the profile embedded in the file. Export is rendered from the original file bytes.

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

### Watch

`cargo watch` rebuilds and relaunches the debug app when a source file changes. Each save quits the window and starts a new process. The library on disk is what comes back.

```bash
cargo install cargo-watch
cargo watch -x run
```

### Windows

Use the MSVC toolchain (`stable-x86_64-pc-windows-msvc`) and the Visual Studio Build Tools with the Desktop development with C++ workload. Then `cargo run --release`.

The library is `%LOCALAPPDATA%\pudduscroll`.

### Linux

Debian or Ubuntu:

```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libxkbcommon-dev libwayland-dev libvulkan-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
cargo run --release
```

A Vulkan driver is required (Mesa or the GPU vendor package).

The library is `$XDG_DATA_HOME/pudduscroll`, or `~/.local/share/pudduscroll`.

### macOS

```bash
xcode-select --install
cargo run --release
```

The library is `~/Library/Application Support/pudduscroll`.

## Layout

PudduScroll is a Rust app. Placement, crop, layering, clipping, and export live in `pudduscroll-core` and are covered by `cargo test`. The window is an eframe/egui binary. The display crate reads the monitor profile used for on-screen color.
