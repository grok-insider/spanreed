# Fabrials design system

Fabrials UI 0.7 "Highstorm" is the shared interface of the Fabrials products and the registry at ui.fabrials.com. This document is the brief: what the system looks like, why, and the rules a screen follows. Code is the source of truth for values (`packages/ui/src/tokens.css`); this file explains them. Explicit product decisions, recorded in that product's own `DESIGN.md`, win over it.

## Principles

1. **One interaction light.** Stormlight blue marks what you can act on and where you are: focus, links, checked controls, the active navigation item, the first data series. Nothing else is blue.
2. **Light only appears dithered.** Fabrials are gems that hold stormlight. On screen, brand light is an ordered dither on canvas, never a gradient or a glow.
3. **Quiet, dense and crisp.** Screens read like good instruments: slate surfaces, hairline dividers, small radii, text at working sizes. One panel with dividers beats cards inside cards.
4. **Say what happened.** A stale value is not a fresh one; a configured connection is not a reachable one. Errors say what went wrong and what to do; empty states invite the next action.
5. **Accessible by default.** Every control has a name, a visible focus and a keyboard path. Colour always travels with text or a symbol.
6. **Small pieces, owned by the host.** Shared components present; products own routing, requests, authentication and storage. Compose small components instead of adding a universal one with dozens of flags.
7. **Synthetic data in public.** Stories, docs and previews never show real mail, credentials or account records.

## Colour

Cold slate neutrals carry the screen; the palette is the same in every product. Values below are the Highstorm references; the tokens hold them in oklch.

| Role | Token | Light | Dark |
| --- | --- | --- | --- |
| Page | `--background` | `#eceeeb` | Stormwall `#161a21` |
| Surface (cards, panels) | `--card` | `#f7f8f6` | `#1e232c` |
| Overlay (menus, dialogs) | `--popover` | `#fbfbfa` | `#232932` |
| Sunken (code, wells) | `--muted`, `--fui-code` | `#e1e4e1` | `#252b35`, `#1b2029` |
| Hover | `--accent` | `#dde1de` | `#2b313b` |
| Hairline | `--border` | `#cdd1cf` | `#2d333d` |
| Control boundary | `--fui-control-border` | `#868d96` | `#5b6470` |
| Ink | `--foreground`, `--primary` | Stormwall `#161a21` | Cold bone `#e6e8e4` |
| Secondary ink | `--muted-foreground` | `#545b64` | `#a0a7af` |
| Stormlight ink (text, focus) | `--brand-ink`, `--ring` | `#2459b3` | `#9cc4ff` |
| Stormlight fill | `--brand` | `#2a63c4` | `#8fb8f5` |

- **Primary actions are ink**: ink on paper in light mode, paper on ink in dark. Stormlight fills a button only when one action must stand out (`variant="accent"`).
- **Status has its own inks**: success, warning and danger (`--fui-success-ink`, `--fui-warning-ink`, `--fui-danger-ink`), each with a word or icon beside it.
- **Crem** `#5a5247`, a warm stone, exists only inside the storm's dither ramp.
- Contrast is checked on the real surfaces: body and control text meet WCAG AA in both themes.

### Gems

Each product signs its mark with a gem, set as `data-gem` on the document root and drawn by `DitherGem` or `FabrialsGem`. The gem colours the mark and nothing else: never a status, a button or a link.

| Gem | Signs |
| --- | --- |
| Stormlight | The Fabrials house mark, Radiant |
| Heliodor | fabrials.com, Usage AI |
| Sapphire | Syl |
| Ruby | Spanreed (spanreeds are ruby fabrials) |
| Emerald | X Tracker |
| Zircon | Fabrials UI, Urithiru |
| Smokestone | News, guides |
| Amethyst | ai.fabrials.com |

## Typography

The IBM Plex superfamily, served locally by `fonts.css` in Latin, Latin Extended, Cyrillic and Vietnamese subsets (Sans also Greek), each face limited by `unicode-range` so a page only downloads what it uses.

| Role | Face | Use |
| --- | --- | --- |
| Display | Plex Sans Condensed 600 (`--fui-font-display`) | Hero headlines (40 to 80 px, line height 0.98), page titles (24 px in apps; 36 px, 44 px from 1800 px, for documentation articles), marketing section titles (28 px), the wordmark |
| Interface | Plex Sans | Body and controls 14 px, metadata 12 px, subsections 16 px |
| Code | Plex Mono | Code, commands, keys and identifiers. Not for labels |
| Editorial | Plex Serif | Quotations only |

Numbers use tabular figures. Headings wrap with `text-wrap: balance`. Copy is sentence case; no tracked uppercase eyebrows, no single word in a headline set in another colour, no arrows appended to link or button text. Arrows that show a direction (a pager's Previous and Next, a disclosure) are fine. Do not lower the contrast of essential text to make hierarchy.

## Space, density and shape

- Spacing follows a 4 px grid (`--fui-space-*`).
- Controls are 40 px (`--fui-control-height`); small 32, extra small 28, large 44. `data-density="compact"` tightens controls to 32 px and collection rows, as a composition choice, not a stored preference. Narrow screens and coarse pointers keep 44 px targets.
- Corners are crisp: 3 px for keys and badges, 4 px for small parts and tooltips, 5 px for controls, 6 px for containers, menus, popovers and the command palette, 8 px for dialogs, sheets, toasts and the sign-in card (`--fui-radius-xs` to `--fui-radius-xl`). Tailwind's `rounded-2xl` and larger are not part of the system. Badges, counts and chips are tags with those 3 to 4 px corners, and a badge's status dot is square, like one pixel of the dither; `StatusDot` stays a round light. Pills are only for a passive label that must stand out.
- Separate with space and hairlines before borders; shadows belong to overlays, and a card carries at most the small shadow. Do not box every label and control.
- Tables scroll inside their labelled region; the page never scrolls sideways.

## Dithering is the brand

Brand light is an 8×8 Bayer dither painted on a canvas (`@fabrials/ui/dither`). Each dot takes one colour of a short ramp that runs from the surface behind the canvas to the brightest light, so the dither fades into whatever it sits on. In the light theme the ramp runs toward dark slate with Stormlight at its core.

Three pieces use it, and every product uses them in these places:

| Piece | Where | Rules |
| --- | --- | --- |
| `DitherScene` | Landing hero (`variant="hero"`, the front enters from the right) and sign-in (`variant="full"`, or inside the `AuthLayout` aside) | Text sits on the calm side or on a solid surface, never on dense dither |
| `DitherBand` | Above a page or section heading that carries the brand (a public stats page, a product overview) | At most one per view |
| `DitherGem` | Product marks in navigation, index rows, empty states; large beside a hero | Sized 16, 20, 28 or 40 px; the gem is the product's |

- The ramp's first stop is `"background"`: `DitherCanvas` reads it from `--fui-dither-bg` (set it to `var(--card)` when the canvas sits on a card), else `--background`.
- `STORM_RAMP`, `gemRamp`, `stormField`, `stormBandField` and `gemField` are the official ramps and fields. A product may add its own field (X Tracker's telemetry, Radiant's night sky) on the same engine.
- A canvas paints once and repaints only on resize, theme change or new data. It never loops.
- Dots are 2 to 4 CSS pixels. Dithering stays off controls and body text.

## Motion

Motion answers what a person did: opening, expanding, confirming. Durations are 100, 150 and 240 ms (`--fui-duration-fast`, `--fui-duration`, `--fui-duration-slow`) with a quick-out ease, and all drop to 0 under reduced motion.

- Continuous motion is reserved for genuine loading indicators, and for a `StatusDot` with `pulse` while something is live or in progress.
- A page may have one orchestrated moment: `DitherScene` and a large `DitherGem` accept `reveal`, so the storm rolls in or the gem fills with light once on first paint. Never again, and not under reduced motion.
- A `TimelineItem` marked `fresh` highlights once when a live item arrives.
- `MoonPhase` and `Starfield` are the sign-in exception inherited from Radiant; they stop under reduced motion.
- No enter animations on navigation people repeat all day.

## Loading states

A loading view is the finished view painted as its own skeleton. Wrap the real components in `Loading` and render them with placeholder data (`placeholderText`, `placeholderList`) while the data loads; turn it off when the data arrives.

- Text becomes a bar per line, as long as the text; icons, images and charts become blocks; coloured controls become neutral solid shapes; borders, tables and cards stay as structure; colour is removed entirely, so a loading view never shows a state it does not have yet.
- Only paint changes and the wrappers are `display: contents`, so nothing moves when loading ends, at any width and with any content. Keep placeholder lengths and row counts close to the real data.
- The content is inert while loading and a status message names what loads (`label`). Keep the control that starts a refresh outside `Loading`.
- The pulse is the one continuous motion and stops under reduced motion.
- Tune a part with `data-skeleton`: `keep` (paint as usual), `hide`, `block`, `fill`.
- Use a Spinner or a button's `loading` for an action in progress, Progress when the amount is known, and the standalone Skeleton block only where no component exists yet.

## Layout and composition

- **Product screens** use `WorkspaceShell` with navigation and header slots when they need a shell; hosts own routing, active state, authentication, theme storage and native window chrome. Mail keeps its resizable panes; desktop keeps native window behaviour.
- **Page patterns** come first: `PageHeader`, `SectionHeader`, `CollectionToolbar`, `Table`, `BulkActions`, `StatePanel`. Bulk actions appear once selection begins. Filters live next to their collection; primary actions next to their task.
- **Nothing is centred on the page.** Headers span the window; content anchors to the same left gutter (`--fui-page-padding`), with a maximum width for reading but no `margin-inline: auto`. On a wide screen the free space stays on the right. Centring is for small things inside a component (an empty state, a toast on a phone), never for a page column. Two frames are the exceptions: documentation uses three columns (a sidebar panel reaching the left edge, the article, the page index) whose outer columns share the extra width, and a sign-in `AuthLayout` without an aside centres its card; with the storm aside the card anchors left.
- **Landings** are left-aligned and asymmetric: the headline and one command or action on the left, the storm on the right. Lists of products or features are indexes (rows with a mark, a name, one line and a link), not grids of identical cards. Numbered markers only for real sequences. Marketing compositions stay in the host and may use tokens and controls; they never become a `WorkspaceShell`.
- **Navigation** that will grow groups its items: fabrials.com's Products menu groups tools by how people use them (install, hosted, developers, open data), each with its gem.

## Content and voice

Write from the user's side: name things by what people recognise, in plain verbs and sentence case. An action keeps its name through the flow ("Revoke key", then "Key revoked"). Errors state what happened and how to fix it, without apologising. Empty states say what to do next. Product interfaces are in English unless the product decides otherwise.

## Components

The catalogue lives in Storybook (`bun run storybook`, port 6041) and at ui.fabrials.com, where every control has a page with a live preview and its import.

- **Controls:** Button (`default`, `accent`, `outline`, `secondary`, `ghost`, `destructive`, `link`; sizes `xs` to `lg` and `icon-*`; `loading`), Input, Textarea, Field, Label, Checkbox, Switch, Select, RadioGroup, NativeRadioGroup, ToggleGroup, ThemeSwitcher, MultiSelect, Combobox, IconButton, FileInput, Toolbar, FieldSet.
- **Overlays:** Dialog, AlertDialog, Sheet, DropdownMenu (with submenus), Tooltip, Popover, StatusPopover, HoverCard, Command (with CommandTrigger), ConfirmDialog (and `useConfirm` for a confirm you can await). They render from `fui-` styles without Tailwind.
- **Collections:** Table, Tabs (vertical rail, scrolling strip), NavTabs, Item (records as rows), Badge, Progress, Skeleton, Card, Accordion (rows for a stack of tools), Disclosure, Breadcrumb, Pagination, DescriptionList, Kbd and KbdGroup.
- **Data:** Stat, StatGroup, Sparkline, Meter, StatusDot, NumberTicker, SeriesChart, ActivityStrip, Timeline, RelativeTime.
- **Docs pieces:** CodePanel, CodeTabs, PackageInstall, Files/Folder/File and RepoInfo. Syntax uses the muted `--fui-syntax-*` inks, never Stormlight.
- **Brand and chrome:** DitherScene, DitherBand, DitherGem, DitherCanvas, ProductLockup, FabrialsGem, Avatar, Snippet, CopyButton, SiteHeader, AuthLayout, MoonPhase, Starfield.
- **Patterns:** WorkspaceShell, AppHeader, Sidebar (menu rows work without a shell), NavSwitcher, ResizablePanelGroup, PageHeader, SectionHeader, CollectionToolbar, BulkActions, StatePanel, SettingsSection, FilterChip, FileThumb, SuggestionCard, TruncatedText, ShimmerText.
- **Loading:** `Loading` with `placeholderText`, `placeholderList` and `useLoading` (see Loading states).
- **`@fabrials/ai-ui`:** provider, quota, history and migration presentation, and the chat pieces (ChatMessage, ChatComposer, CodeBlock with `.fui-markdown`, citations, activity, attachments, VoiceInputButton).

Every pattern covers its normal, loading, empty, error and long-content cases; the loading case is the pattern inside `Loading`. Use named exports; interactive modules carry `"use client"`, tokens and styles need no React. Controls keep Base UI or native semantics; form validation belongs to the host, and `Field` connects label, hint and error.

## Accessibility

Focus is a visible 2 px outline with an offset, in Stormlight. Base UI keeps keyboard traversal, Escape and focus return; do not break them when composing. Check both themes at 390, 768 and 1440 px and at 200 % text size. The visual suite runs axe on its stories in both themes and three widths; it does not replace using the screen with a keyboard.

## Using the system in a product

Import once, in this order, and set the theme and gem on the root element:

```css
@import "@fabrials/ui/tokens.css";
@import "@fabrials/ui/fonts.css";
@import "@fabrials/ui/styles.css";
@import "@fabrials/ai-ui/styles.css"; /* products with AI pieces */
@import "@fabrials/ui/tailwind.css"; /* Tailwind 4 products */
```

```html
<html class="dark" data-gem="ruby">
```

Shared styles use the `fui-` prefix in the `components` cascade layer and work without Tailwind. Host overrides come after them and express a documented product requirement, not a restyle of an equivalent control. A product that retints tokens (Radiant's night sky) does it in its global CSS for both themes and records it in its own `DESIGN.md`. Do not read `localStorage` or browser globals while rendering shared components.

Fabrials products consume a verified copy (`bun run vendor <product> --write`), never edited by hand. Other apps install from the registry.

## The registry and other libraries

ui.fabrials.com publishes a shadcn registry with three kinds of entry:

- **Fabrials components:** the controls above, and the WebMCP and MCP blocks. `init` sets an app up with the palette; `styles` imports the CSS without changing the palette.
- **Shims:** shadcn primitives backed by Fabrials controls, generated for every primitive `@fabrials/ui` covers completely, so code written for shadcn gets Fabrials controls.
- **Components from other libraries:** copied from public shadcn registries at a reviewed snapshot. The repository's license (read at the synced commit) and every npm package a component imports must be MIT, Apache-2.0, ISC, BSD-2-Clause, BSD-3-Clause or 0BSD; anything else, or anything unclear, is refused. Files are republished unchanged with the license at the top, and a component must compile against the shims.

Components from other libraries carry a tier. **Community** components keep their library's look and list their design notes: literal colours, continuous motion, ignored reduced motion, gradients, glow or theme overrides. **Fabrials** components have none. Fabrials products use Fabrials-tier components only; promoting one means adapting it to this document and moving it into the packages, with a story, a keyboard test and a visual reference.

## Extending the system

- Generic presentation belongs in `@fabrials/ui`; provider, quota, migration and chat presentation in `@fabrials/ai-ui`, which depends on it. Network clients and product policy stay in products.
- A new public component or variant needs a story, a keyboard test and a visual reference. Where shadcn has a name for the part, use it: that is what lets the shim cover it.
- Changing a public contract means a version bump and a note in `docs/migration-<version>.md`.
- A product's justified exception goes in that product's `DESIGN.md`, not here.
