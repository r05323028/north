# North — Product UI Design System
>
> **Source of truth:** current prototype HTML/CSS/JS as implemented (8 screens + index launcher). Documentation extraction, not redesign. Extracted 2026-09 · v0.1.0 prototype.
> **Scope:** foundations + core component depth. Neutral monochrome admin workflow, light/dark/system theme, Traditional Chinese UI with English domain labels.

---

## 1. Design Principles & Product Tone

### Principles (observed, not aspirational)

1. **Restrained density over marketing gloss.** Admin workflow, not storytelling. 13–14 px body, 20–22 px h1, 6/8 px radii, hairline borders — information-dense but scannable.
2. **Monochrome is the accent.** No chromatic primary. `.btn-primary` / active states = near-black (light) / near-white (dark). Color reserved for semantic status only.
3. **Surface = structure.** Depth via `--border` + `--elev-*` whisper shadows (1–3 px), not background tint shifts. `--bg` and `--surface` are identical in both themes.
4. **Workflow transparency.** Every state shows its source: REQ-XXX, revision n, evaluation ID, server-sorted hints, version-conflict banners, audit rows. Optimistic UI avoided — “重新整理即為最新”.
5. **One decisive action per head.** Page-head + toolbar + context cards. No competing primary CTAs in same viewport (exception: Review packet 3-way decision group — intentional triad).
6. **Progressive disclosure.** Details/summary for role matrix, collapsed thinking traces, tab panels (對話/概覽/活動), modal overlays for creation/review.

### Tone

- **Voice:** concise, procedural, Traditional Chinese (zh-TW) for UI chrome; English for domain entities (Requirement, Assessment, Repository, REQ-XXX, Draft/Ready/Accepted, Owner/Admin/Manager/Requester).
- **Copy stance:** instructive not promotional. Helpers explain constraints (“僅需標題與描述”, “至少 12 字元”), not benefits.
- **Motion stance:** fast and purposeful (140–180 ms), never theatrical. Respects `prefers-reduced-motion`.
- **Anti-tone:** no playful illustration, no gradient, no emoji as icons — all icons are 1.5–1.8 px stroke line SVGs.

### Inferred / Unresolved

| Area | Status |
| --- | --- |
| Brand accent colour choice | **Inferred:** `#09090b` is effectively “North black” — no logo colour guideline exists; logo-mark is same token. No secondary brand hue. |
| Illustration / empty-state photography | None in prototype; placeholders are minimal SVG + muted text. No brand illustration system defined. |
| Tone scale beyond zh-TW | Preference selector offers EN/ja, but only zh-TW strings exist. No translation source. |
| Sidebar “0.1.0” badge | Hardcoded version string; release versioning not formalised. |

---

## 2. Color Tokens — Light / Dark / Semantic

All screens share identical `:root` and `[data-theme="dark"]` blocks. Single source of truth below — do not duplicate per-screen.

### 2.1 Core palette

| Token | Light | Dark | Role |
| --- | --- | --- | --- |
| `--bg` | `#ffffff` | `#09090b` | Page canvas |
| `--surface` | `#ffffff` | `#18181b` | Cards, sidebar, dialogs, inputs |
| `--muted-bg` | `#f8fafc` | `#1f1f23` | Toolbar/view-toggle, column bodies, search bg, switches off |
| `--muted-bg-hover` | `#f1f5f9` | `#27272a` | Sidebar active row, hover |
| `--fg` | `#0f172a` (slate-900) | `#fafafa` (zinc-50) | Primary text, headings, active tab |
| `--muted` | `#64748b` (slate-500) | `#a1a1aa` (zinc-400) | Secondary text, descriptions, help |
| `--subtle` | `#94a3b8` (slate-400) | `#71717a` (zinc-500) | Icons, placeholders, section labels |
| `--border` | `#e2e8f0` (slate-200) | `#27272a` (zinc-800) | Default hairline |
| `--border-strong` | `#cbd5e1` (slate-300) | `#3f3f46` (zinc-700) | Hover/emphasis border |
| `--accent` | `#09090b` | `#fafafa` | Primary CTA fill, active nav dot, selected tab |
| `--accent-on` | `#ffffff` | `#09090b` | Text on accent |
| `--accent-hover` | `#27272a` | `#e4e4e7` | CTA hover (lightens in dark via inversion) |
| `--accent-active` | `#3f3f46` | `#d4d4d8` | CTA pressed |

Aliases: `--surface-warm: var(--surface)`, `--fg-2: var(--fg)`, `--meta: var(--muted)`, `--border-soft: var(--border)` — present for schema parity; not used independently.

### 2.2 Semantic

| Token | Light | Dark | Usage |
| --- | --- | --- | --- |
| `--success` | `#16a34a` | `#22c55e` | 已連線 dot, running/completed badge alternative, enabled pill |
| `--warn` | `#d97706` | `#f59e0b` | Discussing/sse-banner, block-left on section 5, exec-running |
| `--danger` | `#dc2626` | `#ef4444` | Rejected, errors, required asterisk, stale/409 banner |

Fixed tints (inline, not tokenised): `#fffbeb` (sse-banner bg), `#fde68a` ( SSE / Discussing border), `#92400e` (Discussing text), `#f0fdf4/#bbf7d0/#166534` (Accepted), `#fef2f2/#fecaca/#991b1b` (Rejected). **Inferred gap:** these status tints lack dark-theme overrides — prototype reuses light tints in dark via inline styles (potential contrast debt).

### 2.3 Theme behavior

```html
<!-- Applied before paint -->
<html data-theme="light|dark"> <!-- color-scheme synced -->
```

- Key: `north-theme` in localStorage = `"light"|"dark"|"system"` (default system).
- Light/dark invert accent model: accent flips to `--fg` equivalent so primary button stays highest contrast on canvas.
- Follow-system: `matchMedia('(prefers-color-scheme: dark)').addEventListener('change', …)`.
- Toggle surface: `system-settings.html` offers 3 cards (跟隨系統/淺色/深色) via `[data-theme-value]` radiogroup — see §6.
- `color-scheme: dark|light` set on `<html>` so native controls adapt.

> **Implementation:** copy the inline `<script>` block verbatim at top of `<head>` (blocks FOUC).Expose `window.__setTheme(v)` / `window.__getTheme()` and dispatch `themechange` event.

---

## 3. Typography & Hierarchy

### 3.1 Stacks

| Role | Stack |
| --- | --- |
| Display + Body | `"Geist", "Geist Sans", -apple-system, system-ui, "Segoe UI", Arial, sans-serif` |
| Mono | `"Geist Mono", "Fira Code", ui-monospace, "SF Mono", Menlo, Monaco, monospace` |

No serif. Geist carries both display and body (prototype sets `--font-display == --font-body`). Fira/Geist Mono signals metadata, numbers, IDs.

### 3.2 Scale

| Token | Value | Usage |
| --- | --- | --- |
| `--text-xs` | 12px | meta, badge, field-help, pill |
| `--text-sm` | 13px | buttons, inputs, card body, filter selects |
| `--text-base` | 14px | body baseline (14 not 16 — dense admin) |
| `--text-lg` | 16px | lede / screen-card h3 (16) |
| `--text-xl` | 20px | page h1 |
| `--text-2xl` | 22px | hero-scale (detail title 16–20 variant) |

Leading: `--leading-body 1.5` (body), `--leading-tight 1.25` (headings). Tracking: `--tracking-display -0.015em` on logo/h1.

### 3.3 Hierarchy rules (observed)

- **Page head h1:** 20 px / 650–700 weight / `-0.02em` tracking. Always paired with `.sub` 12.5 px muted line (max 62ch).
- **Section h2/h3:** 13 px / 700, often with inline `idx` circle (22 px, mono, fg on surface). Section 6 uses muted description 12 px.
- **Mono eyebrows:** `.nav-section-label`, col-head h3, table th, screen thumb meta — 10.5–11 px / mono / 0.04–0.07 em tracking / uppercase / `--subtle`.
- **Numbers/IDs:** always mono (`.num`), tabular-nums: REQ-XXX, revision, timestamps, counts, commit SHAs.
- **Weights:** 400 muted, 450 side-link, 500–600 buttons/filters, 650–700 headings, 700–800 badge/status.
- **Balance:** h1/h2/h3 use `text-wrap: balance`; body copy left-aligned, no justification.

### 3.4 Example

```html
<div class="page-head">
  <div>
    <h1>需求看板</h1>
    <p class="sub">按狀態分組 · 搜尋與篩選即時查詢 …</p>
  </div>
  <div class="head-actions"><button class="btn btn-primary">+ 新增需求</button></div>
</div>

<span class="meta">修訂版 <span class="num">5</span></span>
```

---

## 4. Spacing, Sizing, Grid, Containers, Responsive

### 4.1 Spacing scale

| Token | Value |
| --- | --- |
| `--space-1` | 4 px |
| `--space-2` | 8 px |
| `--space-3` | 12 px |
| `--space-4` | 16 px |
| `--space-5` | 20 px |
| `--space-6` | 24 px |
| `--space-8` | 32 px |

Applied rhythm: card padding 16 px (stack-4), page-head 16/12 px vertical, toolbar 10 px vertical, board gap 8 px, detail grid gap 16–20 px, modal body gap 10–12 px.

### 4.2 Sizing constants

| Token | Value | Notes |
| --- | --- | --- |
| `--sidebar-w` | 232 px | Fixed sidebar width, not resizable |
| `--container-max` | 1280 px | max-w-7xl |
| `--container-gutter-desktop` | 20 px | container horizontal padding |
| `--container-gutter-phone` | 16 px | <920 px |
| Control heights | 32 px default (buttons/inputs/selects), 28 px small, 36 px toolbar edge | All interactive controls align to 28/32 baseline |
| Touch target floor | 32 × 32 px hamburger; sidebar links ~32 px | Below 44 px WCAG touch min (known density trade-off, see §9) |
| Logo mark | 24 px square, 6 px radius |

### 4.3 Layout primitives

- **App shell:** `.app { display:flex; min-height:100dvh }` — sidebar sticky left, main flexes.
- **Container:** `.container { max-width:1280; margin-inline:auto; padding-inline: gutter }`.
- **Detail split:** `.layout-detail { grid-template-columns: minmax(0,1.75fr) 360px | 1fr 360px }` — 360 px right meta rail. Collapses to single column at `max-width:1100px`.
- **Board:** `.board { grid-template-columns: repeat(5,1fr); gap:8px }` → 3 col @1200, 2 col @920, 1 col @600.
- **Profile:** 2-col fields (`grid 1fr 1fr`) collapse to 1 col at 920.
- **Packet grid:** 6 cards — intrinsic wrapping (prototype uses grid 1fr 1fr implicit via container); responsive stacks.
- **Card internals:** flex column gap 10–12 px default.

### 4.4 Breakpoints (observed)

| Breakpoint | Behavior |
| --- | --- |
| 1200 px | Board 5 → 3 columns |
| 1100 px | Detail split → single column |
| 960 px | Sidebar → off-canvas drawer (transform translateX -100% → 0), overlay 28% scrim + blur, mobile topbar flex |
| 920 px | Container gutter 20→16, board 3→2, skeletons and stacked filters |
| 600 px | Board 2→1 column |

No other breakpoints; no container queries.

### 4.5 Responsive rules

- Sidebar script: toggle class `open` + `aria-expanded` on hamburger; overlay click closes.
- Reduce-motion: drawer transition disabled when `prefers-reduced-motion: reduce`.
- All tables wrapped in `.table-wrap { overflow:auto; -webkit-overflow-scrolling:touch }` — horizontal scroll, not reflow.

---

## 5. Surfaces, Borders, Radii, Shadows, Density

### 5.1 Surfaces

| Layer | Light | Dark | Used |
| --- | --- | --- | --- |
| Canvas `--bg` | #fff | #09090b | body, app-main |
| Cards `--surface` | #fff | #18181b | .card, .col, sidebar, dialogs |
| Muted fills `--muted-bg` | #f8fafc | #1f1f23 | col-body, view-toggle base, search inputs, pill muted, sidenav hover |
| Pill/hover `--muted-bg-hover` | #f1f5f9 | #27272a | active sidebar row |
| Composer bar / table head | `color-mix(in oklab, var(--bg) 50%, var(--surface))` / `color-mix(... 60%, white)` | — | Subtle 4–5% tint for chrome |

No warm tier — `--surface-warm` alias unused.

### 5.2 Borders

- Default `1px solid var(--border)` on every card/col/table/input. Emphasis hover → `--border-strong`.
- Never 2 px except left-accent strips (e.g., packet section-cards 3 & 5: `border-left:3px solid var(--accent|warn)`).
- Aliases `--border-soft` present but equal to default.

### 5.3 Radii

| Token | Value | Applied |
| --- | --- | --- |
| `--radius-sm` | 6 px | Buttons, inputs, selects, badges, mini-cards, kbd |
| `--radius-md` | 8 px | Cards, modals, table-wrap, avatar-wrap |
| `--radius-lg` | 10 px | (declared, rarely used — packet cards override 8) |
| `--radius-pill` | 9999px | Pills, view-toggle active, tabs |

### 5.4 Shadows / Elevation

| Token | Value | Light | Dark |
| --- | --- | --- | --- |
| `--elev-flat` | none | — | — |
| `--elev-card` | 0 1px 2px rgba(15,23,42,.04) | 0 1px 2px rgba(0,0,0,.25) | Card idle |
| `--elev-raised` | 0 1px 2px rgba(15,23,42,.06), 0 1px 3px rgba(15,23,42,.06) | 0 1px 2px rgba(0,0,0,.4), same 3px | Hover/active: .section-card:hover, view-toggle active, selected theme card, sidenav active |

Never atmospheric blur — hairline + whisper.

### 5.5 Density

- **Tight by default.** 32 px control height, 7–8 px vertical padding inside cards, 12–14 px between sections.
- Preference `"界面密度": 緊湊（推薦）| 舒適` exists in profile (no implementation observed — both options render same).
- Tables use 8–10 px cell padding, 11–13 px font — maximum 7 visible rows before scroll.

---

## 6. Navigation & Information Architecture

### 6.1 App shell

```text
┌─────────────────┬──────────────────────────────────┐
│ sidebar (232)   │ mobile-topbar (52, <960 only)     │
│  logo N North 0.1.0 │ [hamburger]  N North  0.1.0       │
│  Workspace       │ page-head (h1 + sub + actions)  │
│   對話            │ toolbar (search + filters + reset)│
│   需求  [14] ●   │ ────────────────                 │
│   審核            │ content (board / detail / etc)   │
│  Manage           │                                  │
│   執行狀態        │                                  │
│   儲存庫          │                                  │
│  System           │                                  │
│   成員            │                                  │
│   設定            │                                  │
│  ──────────      │                                  │
│  ●已連線 自動更新│                                  │
│  [Alice avatar Owner] → profile                     │
└─────────────────┴──────────────────────────────────┘
+ sidebar-overlay (scrim 28% + blur 4px, z 39)
```

- `aside.app-sidebar` `position:sticky; top:0; height:100dvh; overflow-y:auto` desktop; fixed off-canvas <960.
- `nav.sidebar-nav` is real `<nav aria-label="主導覽">`.

### 6.2 Route map (prototype pages)

| File | Nav label | Route purpose |
| --- | --- | --- |
| `requirement-board.html` | 需求 | Kanban+Table dual view, search/filter, create dialog, packet modal entry |
| `requirement-detail.html` | 對話 | Detail for single requirement — tabs 對話 / 概覽 / 活動, chat + thinking + composer |
| `review-packet.html` | 審核 | Ready queue — filter + table of packets, review modal 6-block packet |
| `runtime.html` | 執行狀態 | Execution/agent status (placeholder + infra in prototype) |
| `repositories.html` | 儲存庫 | Repo list, connect/pin, commit SHA display |
| `members.html` | 成員 | Member table, role matrix, invite/disabled flows |
| `system-settings.html` | 設定 | Theme (light/dark/system), preferences, notification toggles |
| `profile.html` | (user-card) | Profile, security, preferences, notifications, sessions |
| `index.html` | — | Launcher/overview (1.6 KB, links to above — minimal) |

### 6.3 Navigation components

### Side-link

```html
<a href="requirement-board.html" class="side-link active" aria-current="page">
  <svg width="15" height="15" aria-hidden="true">…</svg>
  需求 <span class="count">14</span>
</a>
```

- Tokens: padding 7×8 px, radius 6, 13 px/450 weight, muted, border transparent → active: muted-bg-hover + border + fg/600.
- Hover non-active → muted-bg + fg.
- Sub variant `.sub` (if used): 32 px left indent, 12.5 px.
- Focus: box-shadow `--focus-ring`.
- Counts (`.count`) pill 11 px mono, muted-bg/border.

### Section labels

- `.nav-section-label`: mono 10.5 px / 0.07 em tracking / uppercase / subtle, 14×8×6 padding, first label tighter.

### Mobile topbar

- 52 px high, border-bottom, sticky top 0, z 22, shows hamburger + compact logo + pill version or +新增 quick action.

### User-card

- Sidebar foot anchor → `profile.html`; flex row 26–32 px avatar + name + email meta + role pill. Bordered 8 px radius.

### SSE status

- `.sse-inline`: 6 px dot + mono 11 px “已連線 · 自動更新” (pulse animation, respects reduced-motion). Also banner variant `.sse-banner` (#fffbeb) in board.

### 6.4 Page-head pattern (shared)

- Wrapper `.page-head { 16/12 pad; border-bottom; flex wrap; gap 10; justify-between }`.
- Left: h1 (20 px) + .sub (12.5 px muted, max 62 ch). Right: `.head-actions` (gap 8, wrap).
- Used on every page except detail (which uses `.detail-head` — same but tighter).

---

## 7. Core Components & States

### 7.1 Buttons

Base `.btn { inline-flex; center; gap 6; pad 7×12; radius 6; 13 px/500; transition 140ms ease; white-space:nowrap }`

| Variant | Fill | Text | Border | Hover | Active | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `.btn-primary` | --accent | --accent-on | --accent | --accent-hover | --accent-active | Sole primary CTA per viewport. In decision group takes flex:1. |
| `.btn-secondary` | --surface | --fg | --border | muted-bg + border-strong | — | Cancel, secondary |
| `.btn-outline` | --surface | --fg | --border | border-strong + muted-bg | — | Reject in packet |
| `.btn-ghost` | transparent | --muted | transparent | muted-bg + fg | — | Icon/toggle, tables |
| `.btn-danger` | --danger | white | --danger | — | — | Destructive (demote/disable) |
| `.btn-sm` | 6×10 pad, 12.5 px | — | — | — | — | Tables, footers |
| `.icon-btn` | — | — | — | — | — | Close X (14 px svg), hover muted-bg |

States: `:focus-visible box-shadow var(--focus-ring)`; `:active translateY(.5px)`; `:disabled opacity .5 pointer:none`; `:hover` per variant. No `:focus` ring on click-only (focus-visible polyfill native).

### Examples

```html
<button class="btn btn-primary" style="height:32px" onclick="openModal()">+ 新增需求</button>
<button class="btn btn-ghost btn-sm" style="height:28px">知道了</button>
<button class="icon-btn" aria-label="關閉對話框"><svg width="14" height="14">…</svg></button>
```

**Constraint:** exactly one `.btn-primary` per head. Review modal is exception — 3 primary-weight buttons in a split decision group (Accept=primary, Reject=outline, Request Changes=secondary).

### 7.2 Links

- Global `a { color:inherit; text-decoration:none }` — no underline by default.
- Logo and cards use text links with focus-ring. Table row titles are title-links via `role="button" tabindex=0` pattern.
- No distinct visited color.

### 7.3 Inputs / Selects / Textareas

### Text input

```html
<label for="fTitle">標題 <span class="req">*</span></label>
<input id="fTitle" placeholder="輸入需求標題…" />
<span class="field-help">具體、可審核的標題…</span>
```

- CSS: `height:32px; padding:7×10; border:1px var(--border); radius:6; bg --surface; fg 13 px; placeholder --subtle`.
- Hover → `--border-strong`; Focus → `border-color var(--accent); box-shadow var(--focus-ring)`.
- In profile/detail overview edits, focus handling is inline `onfocus/onblur` style JS (same token).
- Help text: `.help` / `.field-help` 11–12 px muted, 1.4 leading.

### Select

```html
<select aria-label="按角色篩選"><option>全部角色</option></select>
```

- 32 px height, same border/radius/bg/font as input, right 28 px chevron gap via **CSS-drawn double triangle** (`linear-gradient 45/135deg` at calc(100%-14/9) 50%, 5 px size) — no asset. `appearance:none`.
- Min-width 120 px. Hover/focus same as input.

### Textarea

- `min-height:72–84px; padding:10–14; resize:vertical` (74 textarea in overview uses 76 px min, comment composer 72 px, packet rc 84 px).
- Composer variant: wrapped in `.composer { border 1px, radius md, :focus-within ring }`; inner textarea border 0, bar with muted-bg mix.

### Field grouping

- `.field { display:flex; flex-direction:column; gap:6px }`; labels 12–13 px/600, required asterisk `span.req { color:var(--danger) }` or inline red star; help/below 11 px.
- Two-col fields use `grid 1fr 1fr; gap:12/10px; at 920 collapse to 1fr` class `.two-col`.

States: `:invalid` uses inline `.field-error { role:alert }` red text; no formal error border variant observed.

### 7.4 Badges / Pills / Status

**Pill** `.pill`

- Inline-flex, gap 5, 3×8 pad, 9999 radius, mono 11 px / 0.02 em / border var(--border) / bg surface / muted. Variants via inline overrides:
  - `background:var(--accent); color:var(--accent-on); border-color:var(--accent)` — “目前登入”, Owner, revision nav active count.
  - `background:color-mix(var(--success) 10%, white)` + success border — enabled.
  - `color-mix(var(--warn) …)` — warning.

**Badge** `.badge`

- Similar but 3×7 pad, 6 px radius, 11 px/600. Dot child `.badge-dot 6px`.

**Status lozenge** `.status`

- Uppercase mono 10.5 px / 0.04 em / 600 / 2.5×6 pad / 4 px radius / before-dot 5 px:
  - `.s-draft` #f8fafc / #64748b / #e2e8f0
  - `.s-discussing` #fffbeb / #92400e / #fde68a
  - `.s-ready` var(--accent) / var(--accent-on) / --accent + before-dot accent-on
  - `.s-accepted` #f0fdf4 / #166534 / #bbf7d0
  - `.s-rejected` #fef2f2 / #991b1b / #fecaca

**Exec badge** `.exec-badge`

- Pill-shaped uppercase mono 10 px/700: `exec-running` warn tint, `exec-completed` success tint (color-mix 10% bg / 22% border).

**Kbd** `.kbd`

- Mono 11 px, 2×6 pad, muted-bg, 1 px border + 2 px bottom edge, 5 px radius — used sparingly (prototype notes).

### 7.5 Tables / Lists

**Table-mini** (members, repositories, packet list, role matrix)

```html
<div class="table-wrap">
  <table class="table-mini" aria-label="成員清單">
    <thead><tr><th>成員</th><th>角色</th>…</tr></thead>
    <tbody id="membersTbody">…</tbody>
  </table>
</div>
```

- `.table-wrap { overflow:auto; -webkit-overflow-scrolling:touch; border 1px var(--border); radius 8 }`.
- `table.table-mini { width:100%; border-collapse:collapse; font-size:13px }`.
- `th { mono 11 px/0.04 em uppercase/muted/600; 8×10 pad; border-bottom; bg color-mix(var(--bg)60%,white) }` (dark: mix differs but same rule).
- `td { 8×10 pad; border-bottom }`.
- Footer bar `.table-foot` flex between meta left/right with muted text 11 px.

**List view wrapper** (board list)

- `<table class="ds" id="listTable">` — similar but prototype reuses table-mini rules; header stays sticky visually via border.

**Filtering/search scaffold** — see §7.11.

### 7.6 Cards

| Variant | CSS | Usage |
| --- | --- | --- |
| `.card` | bg surface; border; 8 radius; 16 pad | Default panel (members, profile, security, packet preview) |
| `.card-muted` | bg muted-bg; same border/pad | Inactive state, not prominent in current screens |
| `.card--flush` | pad 0; overflow hidden | Tab panels (detail overview wraps to allow internal padding) |
| `.section-card` | bg surface; border 8 radius; 16 pad; flex col gap 10; hover border+shadow | Review packet 6 blocks, profile sub-panels |
| `.screen-card` | bg surface; border md radius; overflow hidden; flex col; thumb 168 px + body | System-settings index/overview |
| `.req-card` | bg surface; border 6 radius; 10×11 pad; cursor pointer; gap 7; text-left | Kanban card |
| `col` | bg surface; border 6 radius; min-height 320; head/body/foot split | Kanban column |

Generic grid: `gap 8–16`; card hover always `border-color color-mix(...fg 8-10%) + box-shadow var(--elev-raised)`. No scale transform.

### 7.7 Dialogs / Modals

**Dialog overlay** (create requirement)

```html
<div id="createModal" class="dialog-overlay" role="dialog" aria-modal="true" aria-labelledby="dialogTitle">
  <div class="dialog">
    <div class="dialog-head"><h2 id="dialogTitle">新增需求</h2><button class="icon-btn" aria-label="關閉…">×</button></div>
    <div class="dialog-body"><div class="field">…</div></div>
    <div class="dialog-foot"><button class="btn btn-ghost">取消</button><button class="btn btn-primary">建立並開啟 →</button></div>
  </div>
</div>
```

- `dialog-overlay` full-viewport scrim (board uses `rgba(15,23,42,.28) + blur 4px`; review modal uses `.modal` with similar). Body `overflow:hidden` when open (JS).
- `dialog` centered, bordered, white surface, 8 radius, max ~560 px width (inferred), stacked head/body/foot with borders.

**Packet review modal** (`.modal` → `.card.modal-card` → `.modal-head/.modal-body`)

- Wider: 720–840 px inferred, scrollable body, sticky decision row. Contains stale-banner, rcPanel, packet-grid, decision-area, audit.
- Close via X button or scrim; focus trapped (basic JS — not full inert).

**Image / avatar picker** uses native `<input type=file hidden>` + label.

States: open toggles `display:flex|block` and `aria-hidden`; focus moves to first input/primary button; Esc dismiss (JS). No backdrop click on packet? Actually overlay click bound on sidebar only; dialogs require explicit close.

### 7.8 Tabs

```html
<div class="tabs" role="tablist">
  <button role="tab" aria-selected="true" class="active">對話</button>
  <button role="tab">概覽</button>
  <button role="tab">活動</button>
</div>
<div id="panel-chat" class="tab-panel active">…</div>
```

- Pill container: `inline-flex; bg surface; border; radius pill; pad 3; gap 2`.
- Buttons: `7×14 pad; pill radius; 12 px mono 600; muted; transparent`; active → `bg var(--fg); color var(--surface)` (accent inversion).
- `focus-visible: var(--focus-ring)`; transition 140 ms.
- Panels: `display:none / block` toggled by `.active`; JS updates `aria-selected`.

### 7.9 Filters / Search toolbar

```html
<div class="toolbar" role="search" aria-label="搜尋與篩選">
  <label class="search"><svg>…</svg><input type="search" placeholder="搜尋…"></label>
  <div class="filter-row">
    <select aria-label="狀態篩選"><option>全部狀態</option></select>
    <select aria-label="排序"><option>最近更新</option></select>
    <button class="btn btn-ghost">重置</button>
  </div>
  <span class="meta">由伺服器排序 · 自動更新</span>
</div>
```

- `.toolbar { flex wrap; gap 8; 10 px vertical; border-bottom }`.
- `.search { flex 1; min-width 200; max-width 380; position:relative }`; icon 14 px absolute left 10; input 32 px high with 32 px left pad.
- `.filter-row { gap 6; wrap; center }`; selects 32 px.
- Hint text `.endpoint-hint / .meta 11 px` on right auto margin.
- All labels hide accessible name via `position:absolute; left:-9999px` when using selects.

### 7.10 Toasts

- No dedicated `<div role=status>` container in some screens; prototype uses `window.showToast(msg)` fallback: creates transient `position:fixed; bottom 16; right 16; 12.5 px` pill (observed via inline call in settings and profile: `showToast('偏好已儲存')`, `showToast('已切換為淺色主題')`).
- Toast style inferred: surface + border + shadow raised, 8 radius, 12 px text — matches shadcn toast pattern (not explicitly styled in CSS — JS-injected). Treat as **inferred** — needs formal spec.

### 7.11 Review actions

Packet decision area — triad + request-changes form:

```html
<button class="btn btn-primary" id="btnAccept">✓ 接受 Accept</button>
<button class="btn btn-outline" id="btnReject">✕ 拒絕</button>
<button class="btn btn-secondary" id="btnRC">↩ 要求修改</button>
<!-- expands -->
<div id="rcPanel" class="card" style="display:none"><textarea id="rcFeedback" placeholder="請說明需要補強之處…"></textarea><button class="btn btn-primary" id="btnRCSubmit">送出 Request Changes</button></div>
```

- Three outcomes: Accepted (終態), → Discussing (Request Changes), Rejected (可 Reopen). Decision preview cards below mirror same styling with tinted backgrounds.
- `staleBanner` / `.conflict` shown on 409 — red/orange mixed text, “重新讀取” button.
- Only Manager/Admin/Owner roles can decide; Requester gets 403 note (centered meta: “僅 … · Requester 403 · 所有決策原子寫入”).
- Request Changes requires non-empty feedback textarea; empty disables submit (inferred, not enforced via disabled attr in prototype).

### 7.12 Loading / Thinking / Empty / Skeleton

### Skeleton

- `.skeleton { height:12; radius 6; bg linear-gradient 90deg muted-bg 25%, #eef2f7 37%, muted-bg 63%; bg-size 400%; animation shimmer 1.2s infinite }` — used in board/list placeholders.

### Thinking (agent-thinking)

```html
<div class="agent-thinking is-thinking" data-thinking>
  <button class="thinking-head" aria-expanded="true">
    <span class="dot-pulse"></span> 思考中 <span class="meta">正在整理…</span><span class="meta">4s</span><svg class="chevron">…</svg>
  </button>
  <div class="thinking-body" role="list">
    <div class="tool-row done">…已檢視儲存庫…<span class="tool-time">1.1s</span></div>
    <div class="tool-row active">…正在整理開放問題…<span class="tool-status">處理中</span></div>
    <div class="tool-row pending">…待更新文件結構…</div>
  </div>
  <div class="thinking-summary">已整理 <strong>2</strong> 項脈絡…</div>
</div>
```

- Container: border, 8 radius, muted-bg head + surface body; variants `is-thinking` (dot pulses), `is-done collapsed` (success dot, collapsed body).
- `dot-pulse` 6 px circle + `animation thinkingPulse 1.1s` when thinking; `chevron` rotates -90 when collapsed.
- `tool-row`: flex gap 9; icon 20×20 (border 6 radius, variants done=fg/surface, active=warn tint, pending 0.65 opacity); name 550/fg, detail mono 11/muted truncate, time/status pill.
- Summary bar: muted-bg + top border, 12 px.

### Empty states

- Board empty, packet empty, member empty: centered 36 px icon box (muted-bg + border 8 radius), 13 px title + muted description + action buttons.

### SSE Banner

- `.sse-banner { flex gap10; 9×12 pad; #fffbeb bg; #fde68a border; 6 radius; 12.5 px/#92400e }` dot inherits success tint; ghost “知道了” button dismisses.

---

## 8. Interaction & Motion

### 8.1 Tokens

- `--motion-fast 140ms`, `--motion-base 180ms`, `--ease-standard cubic-bezier(0.2,0,0,1)`.
- All interactive elements use `transition: all var(--motion-fast) var(--ease-standard)`; sidebar drawer uses motion-base.

### 8.2 Hover / Active

- Buttons: bg/border shift per variant (no fg lightening — contrast maintained).
- Cards: border → fg 8-10% mix + shadow raised.
- Links/side-links: bg muted-bg → muted-bg-hover.
- Arrow in screen-cards: inverts fg/surface on card hover.
- `req-card:active` and `btn:active` apply `translateY(.5px)`.

### 8.3 Focus

- Global: `:focus-visible { outline:none; box-shadow: var(--focus-ring) }` — 2 px canvas halo + 2 px accent ring.
- Dark theme swaps ring to `--fg` (light accent ring invalid on dark canvas).
- Applied to buttons, inputs, selects, tabs, side-links, hamburger.

### 8.4 Reduced motion

```css
@media (prefers-reduced-motion: reduce) { .app-sidebar { transition:none } }
@media (prefers-reduced-motion: reduce) { *,*::before,*::after { animation:none !important; transition:none !important } }
@media (prefers-reduced-motion: no-preference) { .sse-dot { animation:pulse 1.8s infinite } .dot-pulse { animation:thinkingPulse 1.1s } }
```

Prototype declares per-component; system-settings declares universal override. **Canonical:** respect universal disable for all transitions/animations when user prefers reduced motion.

### 8.5 Other micro-interaction

- View toggle active button has subtle drop shadow (`0 1px 2px rgba(15,23,42,.08)`) to lift above muted track.
- Stale-conflict banner appears inline without overlay; rcPanel slides via `display:none→block` (no height animation).

---

## 9. Accessibility & Keyboard / Focus Requirements

### 9.1 Implemented (keep)

- `lang="zh-TW"` on every `<html>`.
- Landmarks: `<aside aria-label="主導覽">`, `<nav aria-label="主導覽">`, `<main>`, headings in order (h1 page, h2 section, h3 card).
- Skip-adjacent: focus rings visible (4 px double ring) with AA contrast against both themes.
- Buttons have discernible text + svg `aria-hidden`; icon-only buttons have `aria-label` (“開啟導覽”, “關閉對話框”).
- Tables have `<thead><th>` and `aria-label`; search inputs have `aria-label` + visible placeholder; selects have `aria-label`.
- Live regions: `role="status" aria-live="polite"` on SSE inline, staleBanner, sseBanner.
- Dialog: `role="dialog" aria-modal="true" aria-labelledby="dialogTitle"` — present on create + packet.
- Tabs: `role="tablist/tab" aria-selected`.
- Reduced-motion respected.

### 9.2 Gaps / Debt

| Issue | Location | Fix |
| --- | --- | --- |
| Touch target 32 px < 44 px min (WCAG 2.5.5) | Buttons, select arrows, table row actions | Raise to 44 px in next iteration or add 6 px hit-pad via ::before |
| Dialog focus trap not inert | Packet/create modals | Add `inert` on main when modal open; return focus on close |
| Table row as button `tabindex=0 role=button` | Packet table rows | Ensure Enter + Space both activate; Space doesn’t scroll |
| Contrast of inline tints in dark | Status tints (#fffbeb on dark surface) | Add dark overrides or replace with color-mix tints using semantic tokens |
| Image alt on user-card | i.pravatar demo images | Real product must use name-based alt; not decorative |
| Kbd/disabled opacity .5 may fail AA on muted-bg | Disabled buttons | Verify 3:1 minimum for disabled is allowed, but avoid conveying info by opacity alone |

### 9.3 Keyboard map (must preserve)

- `Tab/Shift-Tab` traverses head → toolbar → board/list → dialog.
- `Enter/Space` activates buttons and packet rows; `Esc` closes dialogs and sidebar drawer.
- Arrow keys within theme selector (`ArrowRight/Left` moves between 灰系 cards) — replicated for radiogroup.
- Hamburger has `aria-controls="app-sidebar" aria-expanded` toggling.

---

## 10. Content & Localization Conventions

### 10.1 Language matrix

- **UI chrome = zh-TW:** 需求、審核、執行狀態、儲存庫、成員、邀請成員、對話、概覽、活動、偏好、已連線、重新整理即為最新… All visible strings are Traditional Chinese unless noted.
- **Domain entities = English:** REQ-XXX, Draft / Discussing / Ready / Accepted / Rejected, Owner/Admin/Manager/Requester, REQ-101, asm_9f2c… hash, north-theme. Mixed inline: “✓ 接受 Accept” (dual label intentional).
- **Mono fields:** IDs, SHAs, timestamps, counts, emails — always English/number formatting.

### 10.2 Formatting

- Dates: `YYYY-MM-DD` (meta cells) or `YYYY-MM-DD HH:mm` (audit). Preference selector declares format switch, but fixed rendering currently.
- Numbers: tabular-nums via `font-variant-numeric:tabular-nums` on `.num`.
- Email: lower-case; displayed at 11 px meta under name.
- Hashes: truncated `asm_9f2c…e1a0` in headings, full 40-char mono break-all in cards.
- Status labels: English capitalised first letter, paired with Chinese context (“審核需求 · Ready”, “進行中”).
- Help colons: “·” middle-dot as separator — consistent pattern `A · B · C`.

### 10.3 Empty / Hint / Error copy tone

- Empty: “無符合需求 / 調整搜尋或篩選” + clear filter CTA — neutral, actionable.
- Hints: “由伺服器排序 · 自動更新 · 重新整理即為最新” — explains system truth, not client optimism.
- Error: “409 Stale · 需求已過期 … 已是版本 8” — includes version numbers, suggests retry (“重新讀取”).

### 10.4 Inferred constraints

- No pluralization needed (Chinese). English plural “1 需求 vs 3 需求” — Chinese stays same.
- No content-width prose beyond 62 ch subline — body cards are short paragraphs 1–2 lines.

---

## 11. Screen-to-Pattern Mapping

| Screen | Primary layout | Key patterns + components used |
| --- | --- | --- |
| **需求看板 (requirement-board)** | Board grid 5→1 + list table dual view | App shell, page-head, toolbar (search + 3 selects + reset), view-toggle (看板/清單), sse-banner, col/col-head/col-body/col-foot, req-card (status + exec-badge), table-wrap ds, pagination foot, create dialog-overlay, packet modal (6-block grid), stale-conflict banner, audit trail, toast |
| **需求細節 / 對話 (requirement-detail)** | Detail split 1.75fr+360px → stack | Detail-head, tabs (對話/概覽/活動) + tab-panel, msg/bubble (req/agent/system), agent-thinking (is-thinking/is-done), composer (textarea+bar), overview editable form (field grid, two-col), activity timeline, metadata pills |
| **審核 (review-packet)** | Table + modal | Page-head + count pill, toolbar (search + select filters), table-mini (packet rows with status/409), empty state, packet modal — **6-block section-card grid** (Goal/Scope/AC/Assumptions/Blockers/Repos) + decision triad + audit; rcPanel form |
| **執行狀態 (runtime)** | Card list/status dashboard | Page-head, card-muted execution rows, exec-badge running/completed, session/device table (inferred skeleton) |
| **儲存庫 (repositories)** | Table + config | Table-mini repo rows (name/enabled/dot + SHA mini), connect form, filter row, card sections |
| **成員 (members)** | Card + table + details | Page-head with count pill + invite primary, details/summary role-matrix (table-mini), toolbar (search + role/status selects), table-mini with avatar/name/role pill/status pill/action ghost, pending/disabled row styles, invite dialog |
| **設定 (system-settings)** | Overview grid + cards | Screen-grid (mini-kanban cards), sidenav, theme selector radiogroup (3 cards with data-theme-value, checkmark, border/boxShadow sync on themechange), table-mini for spec, legend-dot |
| **個人設定 (profile)** | Stacked cards + grid | Profile card (avatar-wrap + edit label + file input + two-col fields), security card (password fields + email + session list with revoke), prefs card (lang/date/view/density selects + switch), notify card (switch rows), danger/session patterns |
| **Launcher (index)** | Minimal hub | 1.6 KB link list to others — no unique tokens |

**Shared shell across all:** top theme script + container + sidebar + mobile-topbar + sidebar-overlay + footer SSE — identical CSS per file (no shared stylesheet extraction in prototype; tokens duplicated).

---

## 12. Implementation Guidance & Anti-Patterns

### 12.1 How to build new screens in North style

1. **Copy the head contract:** paste the `:root` + `[data-theme="dark"]` block + theme `<script>` before first paint. Do not add new color tokens — use color-mix for tints.
2. **Use the shell:** `div.app > aside.app-sidebar + div.app-main > div.mobile-topbar + main.container`. New pages add to `sidebar-nav` under correct section label (Workspace/Manage/System).
3. **Start with .page-head:** h1 20 px + sub 12.5 px muted + head-actions (one primary at most). Below it, if filtering needed: use `.toolbar` pattern verbatim (search 200–380 px + filter-row).
4. **Prefer .card 8/16 + .table-wrap + .table-mini for lists.** Kanban only for requirement-board — don’t reintroduce columns elsewhere.
5. **Forms:** use `.field` stacks; grid two-col where labels pair; textarea min 72–84 px; help at 11 px. Primary action bottom-right (“儲存變更”, “建立並開啟 →”).
6. **Fonts:** body 14 px, inputs/selects 13 px, meta 11–12 px mono — never introduce 16 px body in admin flows.
7. **Focus:** never remove `box-shadow: var(--focus-ring)` on `:focus-visible`.
8. **Language:** chrome zh-TW, entities English, numbers mono. Keep “X · Y · Z” separators.

### 12.2 Tokens as code

```css
:root{
  --bg:#fff; --surface:#fff; --muted-bg:#f8fafc; --muted-bg-hover:#f1f5f9;
  --fg:#0f172a; --muted:#64748b; --subtle:#94a3b8;
  --border:#e2e8f0; --border-strong:#cbd5e1;
  --accent:#09090b; --accent-on:#fff; --accent-hover:#27272a; --accent-active:#3f3f46;
  --success:#16a34a; --warn:#d97706; --danger:#dc2626;
  --radius-sm:6px; --radius-md:8px; --radius-pill:9999px;
  --elev-card:0 1px 2px rgba(15,23,42,.04); --elev-raised:0 1px 2px rgba(15,23,42,.06),0 1px 3px rgba(15,23,42,.06);
  --focus-ring:0 0 0 2px var(--bg),0 0 0 4px var(--accent);
  --motion-fast:140ms; --motion-base:180ms; --ease-standard:cubic-bezier(0.2,0,0,1);
  --container-max:1280px; --container-gutter-desktop:20px; --container-gutter-phone:16px; --sidebar-w:232px;
  --font-body:"Geist","Geist Sans",-apple-system,system-ui,"Segoe UI",Arial,sans-serif;
  --font-mono:"Geist Mono","Fira Code",ui-monospace,"SF Mono",Menlo,Monaco,monospace;
  --text-xs:12px; --text-sm:13px; --text-base:14px; --text-lg:16px; --text-xl:20px;
  --leading-body:1.5; --leading-tight:1.25;
}
```

Import dark block from any existing screen verbatim (see §2).

### 12.3 Anti-patterns (do not)

| Don’t | Why | Do instead |
| --- | --- | --- |
| Introduce a chromatic primary (blue/purple) or gradient wash | Breaks monochrome identity; prototype uses black as only CTA | Use `--accent` + semantic tokens only |
| Add new border styles (2 px everywhere, dashed) | Prototype is uniform hairline + 3 px left accent only | Keep 1 px; left-accent only for packet blocks 3/5 |
| Use Inter/Roboto/Fraunces for headings | Prototype is Geist unified; serif would clash | Geist everywhere, mono for metadata |
| Increase body to 16 px or radii to 12+ pill cards | Would break dense admin rhythm | 14 px base, 6/8 radii |
| Duplicate primary CTA in same head | Governance failure — one action per viewport | One `btn-primary`; secondary as ghost/secondary |
| Hotlink images or add hand-drawn SVG people | Product is data-dense, no illustration system | Use initials avatar or 24 px geometric mark |
| Ignore dark theme token inversion | Status tints/low contrast debt | Add dark tint overrides via color-mix with semantic tokens |
| Inline hex outside `:root` for new tints | Breaks theme switching | `color-mix(in oklab, var(--warn) 10%, white)` pattern |
| Skip `aria-label` on search/select | Breaks screen-reader flow | Hidden label via offscreen span or aria-label (as prototype does) |
| Add `white-space:nowrap` to titles | Prototype allows balance/wrap; nowrap creates overflow at 360 px | Use `text-wrap:balance`, allow wrap, truncate numbers only |

### 12.4 Open questions / Known gaps to formalize

- Extract duplicated CSS into single stylesheet (currently pasted per-file, ~350 lines — high drift risk).
- Formalize toast container spec (position, duration, queue).
- Define dark overrides for `#fffbeb/#fde68a` family.
- Specify real session/revoke and runtime execution data contracts (currently static demo rows).
- Decide density “舒適” implementation diff (padding/line-height delta).
- Add empty-state illustration guidance or keep icon-box minimal.

---

## 13. Quick Reference — Class Inventory (observed)

`app, app-sidebar, sidebar-head, logo, logo-mark, sidebar-nav, nav-section-label, side-link, count, sidebar-foot, sse-inline, sse-dot, sse-banner, user-card, mobile-topbar, hamburger, sidebar-overlay, page-head, sub, head-actions, toolbar, search, filter-row, view-toggle, btn, btn-primary/secondary/ghost/outline/danger, btn-sm, icon-btn, meta, num, pill, badge, badge-dot, kbd, card, card-muted, card--flush, section-card, sep, board-view, list-view, board, col, col-head, col-body, col-foot, req-card, req-top, status (s-draft/discussing/ready/accepted/rejected), exec-badge, layout-detail, detail-head, tabs, tab-panel, msg, bubble, bubble-req/agent/system, agent-thinking, thinking-head, dot-pulse, chevron, thinking-body, tool-row, tool-icon, tool-name/detail/time/status, thinking-summary, composer, composer-bar, field, help, two-col, table-wrap, table-mini, dialog-overlay, dialog, dialog-head/body/foot, modal, modal-head/body, modal-card, conflict, audit, audit-row, packet-grid, decision-area, screen-grid, screen-card, screen-thumb, sidenav, skeleton, empty.`

> **Density rule of thumb:** if unsure, choose the smaller padding/height, the muted color, and the hairline border. North whispers — it doesn’t shout.
