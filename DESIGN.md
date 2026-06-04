# S3 Explorer — Design System

Production-grade visual system for `affaan-m/s3-explorer` (Rust + axum + vanilla JS).

## Design Direction

- **Purpose**: Browse, upload, preview, edit, copy, rename, and delete objects in a private S3 bucket.
- **Audience**: Engineers and operators doing daily content review and integration work. Repeat use. Large file lists. Occasional large files.
- **Tone**: Utilitarian, technical, dense-but-quiet. Like a code editor — confident, restrained, fast. Not a marketing surface.
- **Memorable detail**: Keyboard-first command palette (`Cmd/Ctrl+K`) that exposes every action. Breadcrumb as a clickable shell-like path. Stable grid that does not shift when actions appear. Dark by default (engineer audience) with full light theme.
- **Constraints**: Vanilla JS (no React), Askama server-rendered templates, ~600 lines of UI code. No runtime CSS framework. No new build step. The Rust backend stays untouched in behavior.

## Tokens

All visual values live as CSS custom properties on `:root` and are mirrored in `design-tokens.json` for tooling. Never hardcode colors, spacing, or radii in components — always reference a token.

### Spacing scale (4px base)

| Token | Value | Use |
|-------|-------|-----|
| `--space-1` | 4px | tightest gap, icon margin |
| `--space-2` | 8px | inline gap, button padding-y |
| `--space-3` | 12px | form input padding, list item padding-y |
| `--space-4` | 16px | panel padding, section gap |
| `--space-5` | 24px | page padding, large gap |
| `--space-6` | 32px | hero gap, dialog padding |
| `--space-7` | 48px | rare, page-level breathing room |

### Type scale (1.125 modular)

| Token | Size / Line | Use |
|-------|-------------|-----|
| `--text-xs` | 11px / 1.4 | labels, captions, badges |
| `--text-sm` | 13px / 1.5 | metadata, breadcrumbs |
| `--text-base` | 14px / 1.55 | body, file rows |
| `--text-md` | 16px / 1.5 | h2, dialog title |
| `--text-lg` | 18px / 1.4 | h1, page title |
| `--text-xl` | 22px / 1.3 | brand |

Fonts:
- `--font-sans`: system stack (`-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif`)
- `--font-mono`: monospace stack for keys, sizes, etags

### Color tokens

Each color is defined as **two complementary palettes** (dark + light) under the same semantic name. The active palette is selected by `prefers-color-scheme`. A future manual toggle would only swap the `:root` token set.

| Token | Dark | Light | Use |
|-------|------|-------|-----|
| `--bg` | #0f1115 | #fafbfc | page background |
| `--bg-elev` | #161a22 | #ffffff | panels, cards, topbar |
| `--bg-elev-2` | #1c2230 | #f3f5f8 | nested panel, input bg |
| `--bg-hover` | #232a3a | #eef1f5 | hover row, hover button |
| `--fg` | #e7ecf3 | #0d1117 | primary text |
| `--fg-muted` | #8a93a6 | #5a6473 | secondary text |
| `--fg-subtle` | #5a6473 | #8a93a6 | tertiary, disabled |
| `--border` | #262d3b | #e3e6ec | dividers, panel borders |
| `--border-strong` | #3a4356 | #c8ced8 | focus rings (low-prominence) |
| `--accent` | #4f8cff | #2c6cf5 | primary action |
| `--accent-hover` | #6aa1ff | #1d57d4 | primary action hover |
| `--accent-fg` | #ffffff | #ffffff | text on accent |
| `--accent-soft` | rgba(79,140,255,0.12) | rgba(44,108,245,0.10) | accent tints, badge bg |
| `--ok` | #34c759 | #16a34a | success |
| `--warn` | #f5a623 | #d97706 | warning |
| `--danger` | #ef5b5b | #dc2626 | destructive |
| `--danger-soft` | rgba(239,91,91,0.12) | rgba(220,38,38,0.08) | danger tints |

### Radii

- `--radius-sm`: 4px (tags, badges, code chips)
- `--radius`: 8px (buttons, inputs, panels)
- `--radius-lg`: 12px (dialogs)
- `--radius-pill`: 999px (avatars, status dots)

### Shadows

- `--shadow-sm`: 0 1px 2px rgba(0,0,0,0.25)  (focus rings, lift on hover)
- `--shadow`: 0 6px 16px rgba(0,0,0,0.35)  (dialogs, dropdowns)
- `--shadow-lg`: 0 16px 40px rgba(0,0,0,0.45)  (modal backdrop shadow)

### Motion

- `--ease`: `cubic-bezier(0.2, 0.8, 0.2, 1)` (default smooth)
- `--duration-fast`: 100ms (hover, focus)
- `--duration-base`: 180ms (transitions)
- `--duration-slow`: 280ms (dialog enter)
- All transitions are killed by `@media (prefers-reduced-motion: reduce)`.

## Component Inventory

| Component | Purpose | Notes |
|-----------|---------|-------|
| `.topbar` | Sticky header with brand, breadcrumb, global actions | backdrop-blur, 1px bottom border |
| `.breadcrumb` | Clickable path segments with `/` separators | `<nav aria-label="breadcrumb">` + `<ol>` |
| `.btn` / `.btn.primary` / `.btn.ghost` / `.btn.danger` / `.btn.small` | Actions | `<button>` for actions, `<a class="btn">` only for navigation |
| `.panel` | Generic content card | elevated background, border, radius |
| `.upload-zone` | Drag-and-drop file target | dashed border when idle, solid accent on `hot` |
| `.progress` | Upload progress bar | labeled, role="progressbar", aria-valuenow |
| `.toast` | Transient feedback | role="status", aria-live="polite" |
| `.dialog` | Modal (uses `<dialog>`) | native modal semantics, focus trap |
| `.file-list` | Table-like list of files/folders | `<ul role="list">`, grid layout, stable columns |
| `.file-row` | One row in the file list | grid columns: `icon name size date actions` |
| `.file-icon` | Type-aware icon (folder/image/text/code/etc.) | inline SVG, `aria-hidden` |
| `.kbd` | Keyboard hint | styled `<kbd>` element |
| `.command-palette` | Floating action launcher | Cmd/Ctrl+K, fuzzy filter |
| `.badge` | File-type / status tag | uppercase, small, accent-soft bg |

## Accessibility Contract

Every component is built to WCAG 2.2 AA. Specifics:

- All actionable elements are `<button>` (not `<div onClick>`).
- Every form control has a connected `<label>`.
- Focus is always visible (`:focus-visible` ring, 2px accent outline, 2px offset).
- Interactive targets ≥ 24×24 CSS pixels (44×44 on touch).
- Color is never the sole indicator (icons + text accompany color).
- Live regions (`role="status"`, `role="alert"`) wrap dynamic content.
- `prefers-reduced-motion`, `prefers-contrast: more`, and `forced-colors` are honored.
- All text meets 4.5:1 contrast; large text and UI components meet 3:1.
- Image previews use descriptive `alt`; decorative images use `alt=""` + `aria-hidden`.
- Tables (future) use proper `<th scope>`; lists use `<ul role="list">` if CSS removes markers.

## Anti-patterns explicitly avoided

- No purple gradients.
- No glass-morphism cards without purpose.
- No animated hero on a tools page.
- No oversized cards on top of cards.
- No "Click here" copy.
- No positive `tabindex`.
- No `aria-label` on a non-interactive element.
- No `placeholder` as a label.
- No `localStorage` for tokens (out of scope, but mentioned because Rust backend uses httpOnly session).
- No emoji as iconography — the favicon and brand mark are SVG. Emojis in copy are allowed (e.g., `Drop a file to upload`).
- No generic Inter-only typographic system without a fallback. System stack is fine.
