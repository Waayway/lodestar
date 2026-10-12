# Lodestar style guide

How the dashboard looks and why, and the components it is built from. The code is the
source of truth for values: tokens in `web/src/styles/tokens.css`, their Tailwind mapping in
`web/src/styles/base.css`, components in `web/src/components/`. This guide explains them; keep
the two in step. The approved mockup is `docs/plans/2026-10-12-web-restyle-mockup.html`
(stylesheets 1, 3 and 5).

## Character

The "Nautical chart": the dashboard reads like a navigator's chart, a calm, precise sheet
you steer a fleet by. Paper and ink in daylight, a dimmed chart table at night. One accent
(the chart's red-brown) marks what to act on; everything else is ink, rules and soundings.
State colours are the only saturated colours on the page, and they always come with an
icon and words, never colour alone.

## Colour tokens

Defined as CSS custom properties on `:root`, mapped into Tailwind `@theme` so components
use utilities (`bg-panel`, `text-muted`, `border-line`, `bg-ok-soft`, `text-unstable`, ...),
never raw hex.

| Token | Light (chart) | Dark (night watch) | Role |
| --- | --- | --- | --- |
| `--bg` | `#f3eee1` | `#0e1626` | Page (chart paper) |
| `--panel` | `#fbf8f0` | `#142035` | Cards, table box, dialogs, sheet |
| `--chip` | `#ebe4d2` | `#1c2a45` | Hash chips, OS tiles, audit icons, hover fill |
| `--fg` | `#17233d` | `#ebe3cd` | Text (ink) |
| `--muted` | `#5d6577` | `#a69e88` | Secondary text, labels, headings |
| `--line` | `#cfc5ad` | `#2d3b58` | Borders, heading rules |
| `--line-soft` | `#e3dcc8` | `#1f2c46` | Dotted row separators |
| `--accent` | `#a8324a` | `#e07a8c` | The one accent: promote, section icons, focus ring |
| `--accent-soft` | `rgba(168,50,74,.1)` | `rgba(224,122,140,.13)` | Accent wash (tile, demo banner) |
| `--accent-line` | `#a8324a` | `#e07a8c` | Promote card border |
| `--mark-bg` / `--mark-fg` | `#17233d` / `#f3eee1` | `#ebe3cd` / `#0e1626` | Logo disc and its star |
| `--ok` / `--ok-soft` | `#1f6e5b` / `rgba(31,110,91,.12)` | `#63c9a8` / `rgba(99,201,168,.13)` | Applied, online, up to date |
| `--warn` / `--warn-soft` | `#8a5a00` / `rgba(176,123,20,.16)` | `#e2b25a` / `rgba(226,178,90,.14)` | Skipped, behind, dirty, agent not answering |
| `--bad` / `--bad-soft` | `#a8261d` / `rgba(168,38,29,.11)` | `#f07468` / `rgba(240,116,104,.14)` | Failed, needs attention, disconnected |
| `--info` / `--info-soft` | `#1f4f8f` / `rgba(31,79,143,.11)` | `#86aee8` / `rgba(134,174,232,.14)` | Building, connecting, queued |
| `--nix` | `#2f5aa8` | `#93b0ef` | The NixOS mark |
| `--stable` / `--stable-line` | `#17233d` / `#17233d` | `#ebe3cd` / `#a69e88` | Promote target channel pill |
| `--unstable` / `--unstable-line` | `#7a5a1e` / `#b08d3c` | `#e2b25a` / `rgba(226,178,90,.6)` | Promote source channel pill |
| `--btn-bg` / `--btn-fg` / `--btn-line` | `#17233d` / `#fbf8f0` / `#17233d` | `#ebe3cd` / `#0e1626` / `#ebe3cd` | Solid button |
| `--log-bg` / `--log-fg` / `--log-cmd` | `#17233d` / `#e6dfcc` / `#e7c27a` | `#09101d` / `#d8d0ba` / `#e2b25a` | Apply log |
| `--grid` | `rgba(23,35,61,.06)` | `rgba(235,227,205,.045)` | Chart grid lines |
| `--overlay` | `rgba(23,35,61,.45)` | `rgba(5,9,18,.6)` | Behind dialogs and the sheet |

`--mark-fg`, `--grid` and `--overlay` are named versions of values the mockup wrote inline.

**Contrast** (WCAG, text on its background), checked for both themes:

| Pair | Light | Dark |
| --- | --- | --- |
| `fg` on `panel` / `bg` | 14.7 / 13.5 | 12.7 / 14.1 |
| `muted` on `panel` / `bg` / `chip` | 5.5 / 5.0 / 4.6 | 6.1 / 6.8 / 5.4 |
| `ok` / `warn` / `bad` / `info` on `panel` | 5.8 / 5.6 / 6.7 / 7.7 | 8.1 / 8.4 / 5.8 / 7.2 |
| `ok` / `warn` / `bad` / `info` on their `-soft` (over `panel`) | 4.9 / 4.7 / 5.6 / 6.5 | 6.3 / 6.4 / 4.7 / 5.5 |
| `accent` on `panel` / `accent-soft` | 6.1 / 5.3 | 5.7 / 4.7 |
| `btn-fg` on `btn-bg` | 14.7 | 14.1 |
| `log-fg` / `log-cmd` on `log-bg` | 11.8 / 9.2 | 12.4 / 9.7 |

All text pairs are at least 4.5:1. Do not put state colours on `--log-bg` in the light theme
(`ok` on it is 2.6:1); the log is plain `log-fg` text.

## Typography

Bundled with `@fontsource` (no font CDN): IBM Plex Sans 400/500/600, IBM Plex Serif
500/600/600 italic, IBM Plex Mono 400/500. Tailwind: `font-sans`, `font-display`, `font-mono`.

| Use | Face | Size / weight | Notes |
| --- | --- | --- | --- |
| Wordmark `lodestar` | Serif italic | 24 px / 600 | `tracking-display` (.02em); the page's only `h1` |
| Section heading | Serif | 13 px / 600 | Uppercase, `tracking-label` (.14em), `text-muted`, rule after |
| Table header | Serif | 12 px / 500 | Uppercase, `tracking-label`, `text-muted` |
| Stat number | Serif | 24 px / 600 | Tabular figures |
| Body | Sans | 14 px / 400, line-height 1.45 | |
| Machine name, card titles | Sans | 14 px / 600 | |
| Labels, pills, buttons (sm) | Sans | 12.5 px | Buttons 600 |
| Buttons | Sans | 13 px / 600 | |
| Sub lines | Sans | 12 px | `text-muted` |
| Hashes, channel names, paths | Mono | 12–12.5 px | Slashed zero |
| Apply log | Mono | 12 px, line-height ~1.6 | |

## Spacing, radii and the chart grid

- Page: max width 1240 px, padding 28 px top, `clamp(16px, 4vw, 48px)` sides, 48 px bottom.
  Sections 22 px apart; tiles and cards 12 px apart.
- Cards and tiles: 16 px padding (tiles 14×16), children 10 px apart.
- Table cells 12×14 px, header cells 10×14 px.
- Radii: `--r` 4 px (`rounded-md`: cards, tiles, table box, dialogs), `--r-sm` 3 px
  (`rounded-sm`: buttons, badges, hash chips, OS tiles), full round for pills, the toggle
  and the logo disc.
- The chart grid: 1 px `--grid` lines every 32 px, on `<html>` so it covers the whole canvas.
  Panels sit on it opaque, so it never runs under text.

## State system

Every state has a colour, an icon and words. The words come from `lib/format.ts`; the
colour and icon come from one table, `lib/tones.ts` (`applyStyle`, `onlineStyle`,
`hostStyle`, `feedStyle`, `behindTone`, `auditIcon`, and the class maps `toneText`,
`toneSoft`, `toneFill`, `toneStripe`, `toneChip`). Components never keep their own
state-to-colour maps. The tones are `ok`, `info`, `warn`, `bad`, `accent` and `neutral`
(muted text on a chip).

| State | Colour | Icon (Lodestar name) | Copy |
| --- | --- | --- | --- |
| Applied | `ok` on `ok-soft` | `Applied` (check) | "Applied" |
| Building | `info` on `info-soft` | `Building` (rotate-cw, spins) | "Applying" |
| Skipped | `warn` on `warn-soft` | `Skipped` (pen-line) | "Skipped: local changes in /path", "Skipped: git fetch failed on the machine", ... |
| Failed | `bad` on `bad-soft` | `Failed` (circle-x) | "Build failed (exit 1)", "Failed" |
| Never applied / idle | `muted` on `chip` | `Idle` (circle-dashed) | "Never applied", "Idle" |
| Online | `ok` dot with an `ok-soft` ring | none | "Online" |
| Agent not answering | `warn` | `Unreachable` (wifi-off) | "Agent not answering" |
| Off | `muted` | `Off` (power) | "Off · last seen 2h ago", "Off" |
| Behind | `warn` (`ok` at zero, `muted` unknown) | none | "3 behind", "up to date", "unknown" |
| Dirty | `warn` on `warn-soft`, small pill | none | "dirty"; checkout line: "local changes", "1 ahead", "checkout is not the running system" |
| Queued | `info` | `Queued` (clock) | "Queued: sent when atlas comes online" |

A machine as a whole has one state, `hostTone`: a running, failed or skipped last apply
wins (also on a machine that is off now), then an agent not answering (`warn`), then off,
then ok when 0 behind, else idle. It drives both the summary tiles and the row stripe, so a
machine is never "fine" in one and "needs attention" in the other:

- *updating*: `hostTone` is running;
- *need attention* (`needsAttention`): failed, skipped or agent not answering;
- *up to date*: the rest that are 0 behind, on or off (`fleetCounts`).

The **row stripe** is a 3 px line on the left edge of each machine row in the `hostStyle`
tone: `ok` green, building `info`, skipped and agent not answering `warn`, failed `bad`, off
`neutral` (muted); an idle machine (behind, nothing wrong) has none. One glance down the
left edge shows which machines need a look.

Channel pills are not states: their `stable` / `unstable` colours live in `ChannelPill`.

## Icons

UI icons are Lucide (`lucide-react` through preact/compat), imported only from `@/icons`
under Lodestar names, 16 px (22 px in tiles and the promote card), `aria-hidden` next to text:

`Apply` (play), `Promote` (chevrons-up), `Log` (scroll-text), `Applied` (check),
`Building` (rotate-cw), `Skipped` (pen-line), `Failed` (circle-x), `Idle` (circle-dashed),
`Off` (power), `Unreachable` (wifi-off), `Attention` (triangle-alert), `Machines` (monitor),
`Channel` (git-branch), `Activation` (terminal), `Audit` (history), `Viewer` (user-round),
`Queued` (clock), `Close` (x), `ThemeSystem` / `ThemeLight` / `ThemeDark` (sun-moon / sun /
moon), `Forbidden` (shield-off), `Disconnected` (cloud-off), and the toast icons
`ToastSuccess` / `ToastInfo` / `ToastWarning` / `ToastError`. `Icon` is their type.

Brand marks come from Simple Icons through `<BrandIcon brand="nixos" | "apple">`
(`icons/BrandIcon.tsx`, named imports so only these paths are bundled): NixOS in `--nix`,
Apple in `currentColor`. They carry `role="img"` and an `aria-label` ("NixOS", "macOS")
unless `decorative`. The logo's four-point star is `StarMark`. No emoji.

## Components

Components take typed props (types from `bindings/`); data and actions come only from
`connection/hooks`. `ui/` components accept `className` and are merged with `cn`.

### `ui/` (shadcn primitives in the tokens)

- **Button** `variant: "default" | "outline" | "ghost"`, `size: "default" | "sm" | "icon" |
  "icon-sm"`, `asChild`. Default is the solid ink button for the action; outline sits beside
  it (also used when the action is not needed, e.g. a machine that is up to date); ghost for
  quiet controls. Icon-only buttons need `aria-label`.
- **Badge** `tone: Tone` (`"ok" | "info" | "warn" | "bad" | "accent" | "neutral"`, from
  `lib/tones`). A state on its soft wash. Prefer `StatusBadge` for applies.
- **Card**, **CardHeader**: a panel; the header row puts a title left and a note right.
- **Dialog** (+ `Content`, `Header`, `Title`, `Description`, `Footer`, `Close`, `Trigger`),
  **Sheet** (`side`), **Tooltip** (`TooltipProvider` in `App`), **Table** (`boxClassName`
  for the scrolling box; rows get dotted separators), **Toaster** (follows the resolved theme).

### `brand/`

- **Logo** `{ children? }`: round star mark and the serif italic wordmark (`h1`);
  `children` is the small line under it. **StarMark** `{ className? }`.
- **ThemeToggle**: three icon buttons (system / light / dark) with `aria-pressed`.
- **ThemeProvider**: holds the choice, sets `<html data-theme>`, tracks the system.

### `fleet/`

- **StatTile** `{ icon, value, label, tone: Tone, spin? }` (the page uses accent, ok, info, bad):
  one number of the fleet summary.
- **ChannelCard** `{ channel: ChannelView, hosts: HostView[], role: ChannelRole }`: head
  commit and subject, machine count with how many are behind, "Apply newest to all on …"
  (disabled without a head or machines).
- **PromoteCard** `{ candidate, pair: PromoteChannels, channels }`: between the pair's
  cards, solid accent border; opens **PromoteDialog**, which pins the commit it showed when
  it opened.
- **HostTable** `{ hosts, now, promote }`: sorted by channel then name; empty state when
  there are none. **HostRow** `{ host: HostView, now, channelRole }`: machine (OS tile,
  name, OS), channel, status, running commit with checkout line, behind, last activation,
  last apply (badge, commit, age, duration, queued note, message tooltip), log button and
  "Apply newest"; the state stripe on its left edge (`data-tone` is the `hostTone`).
- **OsIcon** `{ os }`: 34 px chip with the NixOS or Apple mark (monitor otherwise).
- **StatusBadge** `{ apply: ApplyRecord | null, flakePath? }`: the state system above.
- **OnlineState** `{ host, now }`: dot or icon and the online label.
- **ChannelPill** `{ name, role: "stable" | "unstable" | "other" }`: mono pill; the promote
  target is drawn as `stable`, the source as `unstable` (`channelRole`).
- **CommitHash** `{ rev }`: 7 mono characters on a chip, full hash in `title`.
- **BehindCount** `{ behind }`.
- **ApplyLogSheet** `{ now }`: the log opened from a row, in `--log-*` colours, following
  the end unless scrolled up; reads `useApplyLog()`.
- **AuditList** `{ events: AuditEvent[] | null, now }`: newest audit events, one line and a
  quieter line each, dotted separators.
- **LivePill** `{ feed: FeedStatus }`: Live / Connecting / Disconnected · retry in Ns / No access.
  The visible countdown is `aria-hidden`; a `role="status"` line says only the state
  ("Disconnected, retrying"), so screen readers hear changes, not every second.

### `layout/`

- **PageHeader** `{ feed, viewer, hostCount }`: logo left; live pill, viewer and theme
  toggle right. **DemoBanner** above everything in demo mode.
- **Section** `{ title, icon?, label?, children }`: heading with a rule to the right edge.
  **SectionHeading** for a heading inside a card.
- **EmptyState** `{ title, children? }`: dashed box, muted text.
- **ErrorState** `{ title, icon?, tone? = "bad", children? }`: can't show the fleet (no access, server
  unreachable); `role="alert"`.

## Layout

- Order: header, fleet summary, Channels, Machines, Audit. The apply log opens as a sheet
  from the right.
- Summary tiles: four columns, two below 900 px.
- Channels: `from` card, promote card, `to` card in one row (`1fr auto 1fr`); other
  channels flow in a grid. Cards stack below 900 px.
- The machines table keeps its columns and scrolls sideways inside its bordered box on
  narrow screens; the page itself never scrolls sideways.

## Motion and accessibility

- Motion is the building spinner (1.2 s per turn) and the short fade/slide of dialogs, the
  log sheet and tooltips. Under `prefers-reduced-motion` all of it is off (a rule in
  `styles/base.css` on the spinner and the `data-slot` dialog/sheet/tooltip parts); they
  then appear and disappear at once.
- Real `<button>`s; icon-only buttons carry `aria-label` ("Show log for atlas"); the theme
  toggle uses `aria-pressed`; decorative icons are `aria-hidden`; brand marks are labelled.
- Focus: 2 px `--accent` outline, 2 px offset.
- Text contrast at least 4.5:1 in both themes (table above). State is never colour alone.
- Theme: `prefers-color-scheme` unless the toggle set `data-theme`; the choice is kept in
  `localStorage` (`lodestar.theme`) when the browser allows it and lasts the visit otherwise.
  A small inline script in `web/index.html` applies a stored choice before the CSS paints
  (no flash of the other theme); `ThemeProvider` keeps it in step afterwards.

## Do and don't

- Do use the token utilities (`bg-panel`, `text-muted`, `border-line`); don't write hex or
  Tailwind's palette colours (the palette is cleared, so `bg-zinc-100` does nothing).
- Do keep one solid button per card or row; the rest outline or ghost.
- Do pair every state colour with an icon or words from `lib/format.ts`.
- Do keep copy plain and specific ("Apply newest to all on unstable", "Agent not answering").
- Don't import from `lucide-react` in components; add the icon to `icons/index.ts` under a
  Lodestar name.
- Don't call `fetch` or open a WebSocket outside `connection/`.
- Don't add motion beyond the spinner and the dialog/sheet/tooltip fade (all off under
  reduced motion), and no glows, gradients or emoji.
- Do say "machine" in copy the reader sees ("3 machines", "No machines yet."), not "host".
