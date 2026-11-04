---
name: PudduScroll
description: Warm dark desktop chrome with a cream print surface and a single apricot accent
colors:
  apricot-ember: "#e8a060"
  charcoal-void: "#101010"
  charcoal-panel: "#181818"
  warm-paper: "#f4f1ec"
  ivory-text: "#f0ece6"
  dust-taupe: "#a49e94"
  ember-select: "#30261e"
  hairline: "#3a3a3a"
  skeleton-bone: "#282624"
  cover-empty: "#242424"
  ink-on-accent: "#000000"
typography:
  title:
    fontFamily: "egui proportional"
    fontSize: "22px"
    fontWeight: 600
  name:
    fontFamily: "egui proportional"
    fontSize: "16px"
    fontWeight: 600
  body:
    fontFamily: "egui proportional"
    fontSize: "14px"
    fontWeight: 400
  meta:
    fontFamily: "egui proportional"
    fontSize: "13px"
    fontWeight: 400
rounded:
  none: "0px"
  dialog: "8px"
  cover: "10px"
  card: "12px"
spacing:
  xs: "6px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  section: "18px"
  menu-h: "28px"
  toolbar-h: "48px"
  rail-w: "320px"
  icon-btn: "30px"
  card-inner-w: "212px"
  card-inner-h: "248px"
  cover-h: "132px"
components:
  button-primary:
    backgroundColor: "{colors.apricot-ember}"
    textColor: "{colors.ink-on-accent}"
    rounded: "{rounded.none}"
    padding: "6px 14px"
  button-secondary:
    backgroundColor: "{colors.charcoal-panel}"
    textColor: "{colors.ivory-text}"
    rounded: "{rounded.none}"
    padding: "6px 14px"
  card-library:
    backgroundColor: "{colors.charcoal-panel}"
    textColor: "{colors.ivory-text}"
    rounded: "{rounded.card}"
    padding: "12px"
    width: "{spacing.card-inner-w}"
  dialog:
    backgroundColor: "{colors.charcoal-panel}"
    textColor: "{colors.ivory-text}"
    rounded: "{rounded.dialog}"
    padding: "16px"
  canvas-surface:
    backgroundColor: "{colors.warm-paper}"
    rounded: "{rounded.none}"
---

# Design System: PudduScroll

## Overview

**Creative North Star: "Apricot on Charcoal"**

PudduScroll’s interface is a warm, classy desktop tool: charcoal chrome around a cream print surface, with apricot as the only highlight. The mood is refined and restrained. Brand lives in precise details — one accent, flat tonal layers, and quiet motion — not in decoration.

The continuous strip is the stage; the chrome stays out of its way. Menus are squared and native-feeling. Library cards are the softest corners in the system. Anti-references are neon cyber UI, purple SaaS gradients, and Instagram-clone chrome.

**Key Characteristics:**
- Single accent (Apricot Ember) on charcoal; cream paper only for the canvas
- Flat tonal depth; shadows reserved for toasts
- Square chrome, gently rounded cards and dialogs
- Type scale locked to 22 / 16 / 14 / 13
- Motion small and fast; interaction updates stay immediate

## Colors

A warm charcoal set with one apricot voice and a cream print surface for the strip.

### Primary
- **Apricot Ember** (`#e8a060`): The only highlight. Primary actions (Create, Choose folder, Update, Undo on toast), plus strokes, empty-frame plus marks, and crop/rotate handles. Text on apricot fills is black.

### Neutral
- **Charcoal Void** (`#101010`): App background and editor field.
- **Charcoal Panel** (`#181818`): Toolbar, cards, dialogs, rail chrome.
- **Warm Paper** (`#f4f1ec`): Canvas / slide surface only — the “print” against the darkroom.
- **Ivory Text** (`#f0ece6`): Primary labels and names (`RichText::strong` for names).
- **Dust Taupe** (`#a49e94`): Secondary lines, empty-state copy, meta.
- **Ember Select** (`#30261e`): Selected / active warm nest on charcoal.
- **Hairline** (`#3a3a3a`): 1px card and dialog strokes.
- **Skeleton Bone** (`#282624`) / **Cover Empty** (`#242424`): Loading shimmer and empty cover fills.

**The One Accent Rule.** Apricot Ember is the only highlight. New colors join the warm set in `crates/app/src/ui/mod.rs`; they do not compete with ACCENT.

**The Paper-Only Rule.** Warm Paper is for canvases. Do not use it as app chrome fill.

## Typography

**Display Font:** egui default proportional (platform UI font)
**Body Font:** same stack
**Label/Mono Font:** same stack (no separate mono role)

**Character:** Quiet, tool-like hierarchy. Weight and size do the work; no display face, no uppercase branding locks.

### Hierarchy
- **Title** (strong / heading, 22): Section titles such as Templates / Carousels and modal headings.
- **Name** (strong, 16): Card and object names.
- **Body** (regular, 14): Buttons, toast copy, dialog body, general UI.
- **Meta** (regular, 13): Counts, secondary strip labels, muted status.

**The Locked Scale Rule.** Reuse 22 / 16 / 14 / 13. Do not invent a fifth size for ordinary chrome.

## Layout

Desktop Operate layout: menu bar (28px) over toolbar (48px), then editor field with a fixed right rail (320px). Home is a padded charcoal field (16px inset) with two collections — Templates then Carousels — card grids at 12×12 spacing, 18px between sections.

Cards are fixed: inner 212×248, cover height 132, action row 32, margin 12. Related controls share the current margin and `ui.spacing`; gaps between groups are larger than gaps inside a group. Icon buttons stay 30–34px; crop handle hit is 12px (`CROP_HANDLE_HIT`). New chrome joins the toolbar, the editor, or the rail — not a fourth region.

**The Chrome Trinity Rule.** Toolbar, editor, rail. Home cards use `card_frame`; editor chrome sits flat on Charcoal Void and Charcoal Panel.

## Elevation & Depth

Flat by default. Depth is tonal: Charcoal Void → Charcoal Panel → Ember Select. Focus and manipulation use Apricot Ember strokes, not glow. Drop shadows are not part of resting chrome; the toast frame is the exception (`offset [0,4]`, blur 16, black α96) so ephemeral feedback lifts briefly above the tool.

**The Flat-By-Default Rule.** Surfaces rest flat. Shadows appear only for toast feedback, not for cards, panels, or buttons.

## Shapes

Form language is mostly squared chrome with soft cards. Menu bar and menu items use corner radius 0. Dialogs use 8px. Library cards use 12px; cover images use 10px. Strokes are 1px Hairline on cards and dialogs. Empty-frame affordances are circular plus marks with apricot stroke on a near-black disc.

**The Square Chrome Rule.** Menus and primary tool chrome stay square. Rounding belongs to cards, covers, and dialogs — not to the menu bar.

## Components

Refined and restrained: clear action names, quiet secondary chrome, apricot only when the action matters.

### Buttons
- **Shape:** Square chrome (0 radius); padding about 14×6 on menu-styled controls
- **Primary:** Apricot Ember fill, black label (“Create template”, “Choose folder”, “Update”)
- **Secondary / Ghost:** Default egui / panel controls in Ivory Text on charcoal; Cancel and ordinary actions stay unfilled
- **Hover / Focus:** Pointing hand on clickable rows; no bounce or glow

### Cards / Containers
- **Corner Style:** Card 12px; cover 10px
- **Background:** Charcoal Panel; empty cover Cover Empty (`#242424`)
- **Shadow Strategy:** None at rest; cover hover scales to `1.0 + 0.06 * t` over 0.18s
- **Border:** 1px Hairline
- **Internal Padding:** 12px (`CARD_MARGIN`)

### Inputs / Fields
- Native egui fields on charcoal; selectable ratio labels for canvas aspect
- Sliders use trailing fill; stay within the warm set

### Navigation
- Menu bar on Charcoal Void, Ivory Text labels, square corners, 28px tall
- Toolbar on Charcoal Panel, 48px, inner margin 12×6
- Home section headings with adjacent text actions (New template, Import)

### Dialogs / Modals
- Charcoal Panel, 8px radius, 16px padding, 1px Hairline; min width ~420 on confirms
- Primary action apricot + black text; Cancel secondary

### Toasts
- Dialog frame plus the only standing shadow; body 14 Ivory Text; optional Undo as primary apricot button
- Entrance near full size with fade; scale easing stays within the 0.06 hover budget

### Signature: Continuous Strip / Canvas
- Warm Paper canvases on Charcoal Void; frames may cross dividers
- Selection and handles in Apricot Ember; grid lines stay quiet neutrals
- Empty frames show the circular apricot plus

## Do's and Don'ts

### Do:
- **Do** paint with the warm set in `ui/mod.rs` (`BG`, `PANEL`, `CANVAS_BG`, `ACCENT`, `TEXT`, `MUTED`, `SELECT_BG`).
- **Do** put black text on apricot fills.
- **Do** name buttons as actions (“Create template”) and empty states as what’s missing (“No templates yet.”).
- **Do** keep hover/open motion in 0.1–0.25s; keep drag, zoom, nudge, and undo on the same frame.
- **Do** route long names through `short_name` so rows still read with extreme lengths.

### Don't:
- **Don't** introduce neon cyber UI, purple SaaS gradients, or Instagram-clone chrome.
- **Don't** add a second accent color or use Warm Paper as app chrome.
- **Don't** put drop shadows on resting cards, panels, or buttons.
- **Don't** invent type sizes outside 22 / 16 / 14 / 13 for ordinary UI.
- **Don't** round the menu bar or treat the editor as a nested card stack.
