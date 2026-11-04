# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Anyone composing Instagram carousels who wants the job open and approachable: creators, small businesses, marketers, and casual posters. The product should not assume a power-user or agency workflow. The situation is preparing a multi-slide post before publishing on Instagram.

## Product Purpose

PudduScroll is a desktop app for composing Instagram carousels. The user designs a layout once as a template, then opens it into as many carousels as needed, fills frames with photos, and exports one image per canvas so a swipe stays continuous. Success is finishing a carousel that looks intentional on Instagram without fighting the tool.

## Positioning

The continuous horizontal strip is the product: canvases sit in a row, frames can cross the gaps between them, and export clips what you see into each slide. That matches how an Instagram carousel is experienced as one composition across slides, not a stack of separate single-slide editors.

## Operating Context

Primary use is a local desktop session (Windows, Linux, macOS) with a library of templates and carousels saved on disk. The editor is a strip of canvases with a right rail for canvas, frame, photo, crop, and layers. Export writes JPEG or PNG files into a chosen folder for upload to Instagram. Web/WASM is a secondary path, not the primary identity.

## Capabilities and Constraints

- Templates hold empty layouts (name, cover, canvas ratio, canvas count, frames); carousels hold photos, crops, and names.
- Canvas ratios: 1:1, 4:5 (default), 3:4, 9:16; 1–20 canvases.
- Frames move and resize across canvas boundaries; grid snap optional; layer order is front to back.
- Photos: png/jpeg/webp; pan, zoom, rotate, optional border; crop presets including freehand.
- Undo/redo; library persists under the OS app-data location.
- Geometry, crop, layering, clipping, and export live in `pudduscroll-core`; the shell is eframe/egui.
- Visual brand is locked in `DESIGN.md` (Apricot on Charcoal). Logo asset still open.
- Platform note: Impeccable’s Platform field has no desktop-native value; recorded as `web` for schema. Product identity is desktop-first; treat UI as a desktop Operate surface, not browser chrome.

## Brand Commitments

Product name: PudduScroll. Visual world is **Apricot on Charcoal**: warm classy desktop chrome, refined and restrained, single apricot accent on charcoal with cream paper only for canvases. Authority: `DESIGN.md`. Anti-references: neon cyber UI, purple SaaS, Instagram-clone chrome. No logo asset yet.

## Evidence on Hand

Product behavior and copy are documented in `README.md` and `AGENTS.md`. No customer testimonials, case studies, or marketing assets on hand; do not invent them.

## Product Principles

1. The strip is the truth: what you see across canvases is what export clips into the swipe.
2. Broad access over niche power: prefer clarity and approachability for as many people as possible.
3. Intuitive by default: the continuous-carousel model should feel obvious without training.
4. Templates multiply work: design the layout once, reuse it for many carousels.
5. Desktop-first craft: look and feel belong to a native desktop tool; web remains a plus.
