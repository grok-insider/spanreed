# Fabrials design system

Version: 0.3.0. AI Relay and enterprise Open Mail consume the generated 0.3 distributions. Spanreed still vendors 0.1. Radiant, the fabrials.com product routes, and admin.fabrials.com do not import the package yet. ui.fabrials.com shares these tokens and does not replace its WebMCP catalogue with mail or AI components. Distribution is a verified vendor copy, not an npm release.

## Identity

Quiet operational tools, not identical screens. Use IBM Plex Sans locally, neutral surfaces and meaningful color for success, warning and destructive/error states. Preserve third-party provider marks and actual user content. No decorative gradients, perpetual motion or inflated empty space.

The source of truth is `packages/ui/src/tokens.css`. Its semantic color names remain compatible with existing shadcn hosts. Shared component styles use the `fui-` prefix in the `components` cascade layer; hosts import tokens, fonts and styles once. Tailwind is optional. Host overrides belong after shared styles and should express a documented product requirement, not restyle equivalent controls.

## Typography and density

Use the token font stack: Plex for interface text, system monospace for code and identifiers. Use tabular numerals. Body and controls use 14px, supporting metadata 12px, subsection headings 16px and page headings 24px. Do not reduce essential text contrast to suggest hierarchy.

Spacing follows 4px increments. Standard controls are 40px; `data-density="compact"` uses 32px controls and tighter collection rows. Narrow/coarse-pointer layouts retain at least 44px control targets. Compact mode is a composition option, not a new stored user preference. Portaled menus and dialogs keep standard density. Avoid stacking a border around every label/control group.

## Composition

`WorkspaceShell` accepts navigation and header slots. Hosts own route resolution, active state, authentication, theme persistence, platform window chrome and native drag behavior. Shared components never import those implementations.

Use `PageHeader`, `SectionHeader`, `CollectionToolbar`, `Table`, `BulkActions` and `StatePanel` before inventing another page pattern. Show bulk actions only after selection begins. Keep filters close to their collection and primary actions close to the task they perform. Normal connectivity is quiet; stale/offline/error states state what happened and what the user can do.

The shell is not a mandatory universal layout: mail retains its resizable three panes; desktop retains native window behavior. Marketing compositions stay in the host. They may import tokens and generic controls; they do not become a `WorkspaceShell`.

## Components and state

The catalogue lives in Storybook (`bun run storybook`, localhost:6041). Stories use synthetic data, never mail, credentials or account records captured from production.

- Controls: Button, Input, Textarea, Field, Label, Checkbox, Switch and Select.
- Overlays: Dialog, AlertDialog, SheetContent, DropdownMenu and Tooltip.
- Collections: Table, Tabs, Badge, Progress, Skeleton and Card.
- Patterns: WorkspaceShell, PageHeader, SectionHeader, CollectionToolbar, BulkActions and StatePanel.

Use named exports for React server/client compatibility. Shared interactive entries retain `use client`; tokens and styles have no React dependency. Each control keeps Base UI/native semantics and an accessible name. Form validation belongs to the host; `Field` connects the label, hint and error with the control.

Every new pattern needs normal, loading, empty, error and long-content cases where applicable. A stale observation is not a fresh value; a configured connection is not proven reachability. Color always has a textual or symbolic companion.

## Theme, accessibility and motion

Light, dark and system themes share semantic names. Hosts apply `.dark` to the document root and keep their existing preference storage. Do not read localStorage or browser globals during shared-component rendering.

Focus is a visible 2px outline with offset. Preserve keyboard traversal, Escape handling and focus restoration through Base UI. Use AA contrast for meaningful text and controls; verify actual surfaces, not token values in isolation. Check 390/768/1440 widths and 200% text/viewport zoom. Data tables can scroll inside their labeled region; the page must not overflow horizontally.

Motion uses the 140ms feedback token. Continuous motion is reserved for genuine loading indicators and stops under reduced motion. Do not add enter animations to frequently repeated navigation.

## Extending the system

Generic presentation belongs in `@fabrials/ui`; provider, quota, migration and consumption components belong in `@fabrials/ai-ui`. The latter depends on the former. Network clients and product policy remain in their applications. Do not add a universal component controlled by dozens of product flags; compose smaller pieces.

Describe a justified exception in the consuming product's design document. Add a story, keyboard test and visual reference before extending a public variant. Update the package version and migration notes when changing a public contract. Never hand-edit generated vendor copies.
