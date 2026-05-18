# Campaign Card Redesign

Replaces the minimal `CampaignCard` on the authenticated hub page with two distinct card shapes that reflect campaign lifecycle state. Graph-paper stubs for incomplete campaigns (no content to show); banner cards for sealed campaigns (full content, gold/plum active/inactive treatment).

## Card States

Four states derived from the `Campaign` type, rendered as two visual shapes.

### State Derivation

```
last_init_error !== null           -> init-failed   (graph paper card)
wizard_completed_at === null       -> draft          (graph paper card)
sealed + in routing table          -> loaded         (sealed banner card, gold)
sealed + not in routing table      -> ready-to-load  (sealed banner card, plum)
```

The routing-table signal is not yet exposed by the API. Until it is, all sealed campaigns render as ready-to-load (plum). The card component accepts a `loaded` boolean prop (defaulting to `false`) so the gold treatment is ready to wire without changing the component API.

### Priority

`last_init_error` takes precedence over `wizard_completed_at`. A campaign that completed the wizard but failed init renders as init-failed, not sealed.

## Shape 1: Graph Paper Card

For draft and init-failed campaigns. These campaigns have no name, tagline, or game system.

### Structure

- Dashed plum border (`border-dashed`, `border-primary/20`)
- CSS graph-paper background: repeating vertical + horizontal lines in `primary/7` on 20px grid
- Centered content: icon, label, hint text
- Same border-radius as sealed cards (`rounded-2xl`)
- Same min-height as sealed cards so the grid stays even
- Hover: lifts (`-translate-y-0.5`), border darkens, plum shadow

### Draft State

- Icon: `Pencil` (lucide-react), in plum icon container (`bg-primary/8`, `rounded-xl`)
- Label: "Draft" (uppercase eyebrow, plum)
- Hint: "Campaign still on the drafting table." (italic, display font, muted)
- Click action: navigate to `/c/$campaignId` (returns to wizard)

### Init Failed State

- Icon: `XCircle` (lucide-react), in amber icon container (`bg-amber-500/12`)
- Label: "Init failed" (uppercase eyebrow, amber `#92400e`)
- Hint: "Something went wrong. Click to retry." (italic, display font, muted)
- Click action: navigate to `/c/$campaignId` (where retry logic lives)

## Shape 2: Sealed Banner Card

For campaigns that have completed the wizard. Ported from the WorldsAwait wireframe variant.

### Structure

Top-to-bottom:

1. **Banner** (80px): gradient background + crosshatch texture + optional gold glow
2. **Body**: status indicator row, display title, italic tagline, footer
3. **Footer**: relative timestamp (from `updated_at`) on the left, game system on the right, separated by a `border-t`

### Loaded State (Gold)

Campaign shard is in the routing table: actors running, WebSocket ready.

- **Banner gradient**: warm gold. `radial-gradient(ellipse at 50% 120%, rgb(184 149 48 / .40), transparent 70%), linear-gradient(160deg, gold/22%, bronze/22%)`
- **Banner glow**: `radial-gradient(circle at 80% 0%, rgb(184 149 48 / .30), transparent 55%)`, `mix-blend-mode: screen`
- **Banner texture**: crosshatch pattern (diagonal cross-lines, like engraved bronze). CSS-only: two `repeating-linear-gradient` layers at 45deg and -45deg, `rgb(0 0 0 / .04)` lines at 5px intervals, 0.8 opacity. Uses the same visual vocabulary as the existing `crosshatch.svg` asset but rendered in CSS for color adaptability.
- **Border**: `rgb(184 149 48 / .3)` with `0 0 0 1px rgb(184 149 48 / .10) inset` shadow
- **Hover**: border intensifies to `gold/.5`, shadow grows
- **Status indicator**: gold ember dot with pulse animation ("Loaded")
- **Card shadow**: `var(--shadow-gold-soft)`

### Ready to Load State (Plum)

Campaign is sealed and healthy but not currently checked out. Will cold-start on access.

- **Banner gradient**: cool plum. `radial-gradient(ellipse at 50% 120%, rgb(90 74 106 / .20), transparent 70%), linear-gradient(160deg, primary/12%, bronze-muted/30%)`
- **Banner glow**: none
- **Banner texture**: same crosshatch pattern as loaded, but at reduced opacity (0.5) so it recedes behind the muted gradient
- **Border**: `rgb(90 74 106 / .15)`
- **Hover**: border intensifies to `primary/.35`, plum shadow
- **Status indicator**: muted static dot, `var(--color-line)` ("Ready to Load")
- **Card shadow**: default `0 8px 32px -16px rgb(28 25 23 / .25)`

### Shared Sealed Card Details

- **Status indicator**: dot (7px circle) + text label. Flexed to the left in the head row.
- **Title**: `font-display text-2xl font-medium tracking-tight` (Cormorant Garamond). From `campaign.name`, falls back to "Untitled campaign".
- **Tagline**: `font-display text-sm italic` in `foreground/75`. From `campaign.tagline`, hidden if null/empty.
- **Footer timestamp**: relative time derived from `campaign.updated_at`. Clock icon (lucide `Clock`, 13px) + text. Use a small utility function with `Intl.RelativeTimeFormat` (no external dependency).
- **Footer game system**: `font-display italic text-primary`. From `campaign.game_system`, falls back to "System not yet chosen".
- **Banner activity slot**: reserved for future activity indicators (where the wireframe's flame icon was). Not rendered in this implementation. The slot's position (absolute, right-14px, vertically centered in banner) is documented for the future PR that adds presence indicators.

## Grid Layout

- 2 columns on desktop (`md:grid-cols-2`), 1 column on mobile
- `gap-5` (20px)
- Graph paper cards and sealed cards intermix in the same grid, sorted by `updated_at` descending
- Max width: `max-w-5xl` (same container as current hub, `max-w-6xl` can be narrowed since 2-col is wider per card than 3-col)

## Data Mapping

| Card field | Source | Fallback |
|---|---|---|
| State | Derived (see above) | -- |
| Title | `campaign.name` | "Untitled campaign" (sealed only) |
| Tagline | `campaign.tagline` | Hidden if null/empty |
| Game system | `campaign.game_system` | "System not yet chosen" |
| Timestamp | Relative from `campaign.updated_at` | -- |
| Loaded | Future: routing table status | `false` (all sealed cards render as ready-to-load until wired) |

## Component Structure

```
CampaignCard (props: { campaign: Campaign })
  -> derives state from campaign fields
  -> if draft or init-failed: renders GraphPaperCard
  -> if sealed: renders SealedCard

GraphPaperCard (props: { state: "draft" | "init-failed", campaignId: CampaignId })
  -> dashed border, graph paper bg, icon + label + hint
  -> Link to /c/$campaignId

SealedCard (props: { campaign: Campaign, loaded: boolean })
  -> banner + body + footer
  -> gold or plum treatment based on `loaded`
  -> Link to /c/$campaignId
```

`CampaignCard` remains the public API. `GraphPaperCard` and `SealedCard` are internal to the component file (not exported). The parent hub page does not need to know about card shapes.

## What Stays Unchanged

- Hub page layout, header ("Welcome back" / "Your worlds await."), "Start a new campaign" button
- `HubNav`, `Shell`, `EpicBackdrop`, `WizardBackdrop`
- `EmptyHubCard` (zero-campaign empty state)
- Campaign creation flow, wizard, all routing
- `PopulatedHub` component structure (just renders a grid of `CampaignCard`s)

## What Changes

- `CampaignCard.tsx`: rewritten from a single flat card to a dispatcher over `GraphPaperCard` / `SealedCard`
- `_authed/index.tsx`: grid changes from 3-col to 2-col, container width may narrow
- New CSS: graph-paper background (CSS grid lines for draft cards), crosshatch texture (CSS diagonal lines for sealed banners), banner gradients, ember dot animation, status indicators. All CSS-only, no new SVG assets. Tailwind utility classes where possible; inline styles for gradients and repeating patterns.

## Dark Mode

The banner gradients use `color-mix()` with CSS custom properties (`--color-gold`, `--color-primary`, `--color-bronze`, `--color-bronze-muted`) that already shift under `.dark` in `theme.css`. No gradient overrides needed. Borders, shadows, and a few accent colors need explicit `dark:` Tailwind variants.

### Sealed Card (both states)

| Property | Light | Dark |
|---|---|---|
| Card background | `bg-background` (adapts) | (same) |
| Default border | `border-black/8` | `dark:border-white/8` |
| Card shadow | `shadow-[0_8px_32px_-16px_rgb(28_25_23/0.25)]` | `dark:shadow-[0_12px_40px_-18px_rgb(0_0_0/0.55)]` |
| Footer separator | `border-black/6` | `dark:border-white/8` |
| Crosshatch lines (loaded) | `rgb(0 0 0 / .04)` at 0.8 opacity | `rgb(255 255 255 / .04)` at 0.8 opacity |
| Crosshatch lines (ready) | `rgb(0 0 0 / .04)` at 0.5 opacity | `rgb(255 255 255 / .04)` at 0.5 opacity |

### Loaded State (Gold) Dark Overrides

| Property | Light | Dark |
|---|---|---|
| Border | `rgb(184 149 48 / .3)` | `rgb(212 169 68 / .35)` (uses dark gold `#d4a944`) |
| Inset shadow | `1px rgb(184 149 48 / .10)` | `1px rgb(212 169 68 / .15)` |
| Card shadow | `shadow-gold-soft` | `0 12px 50px -18px rgb(212 169 68 / .25)` |
| Ember dot glow | pulses with light gold | pulses with dark gold |

### Ready to Load State (Plum) Dark Overrides

| Property | Light | Dark |
|---|---|---|
| Border | `rgb(90 74 106 / .15)` | `rgb(154 134 170 / .15)` (uses dark primary `#9a86aa`) |
| Hover border | `rgb(90 74 106 / .35)` | `rgb(154 134 170 / .3)` |
| Hover shadow | `shadow-primary` | `0 25px 50px -12px rgb(0 0 0 / .3)` |

### Graph Paper Card Dark Overrides

| Property | Light | Dark |
|---|---|---|
| Dashed border | `primary/20` | `dark:border-primary/15` |
| Grid lines | `rgb(90 74 106 / .07)` | `rgb(154 134 170 / .06)` (adapts via token) |
| Icon container bg | `primary/8` | (adapts via token) |

### Init Failed Amber

Amber is a semantic color, not themed. In light mode: text `#92400e`, bg `rgb(251 191 36 / .12)`. In dark mode: text `amber-400` (`#fbbf24`), bg `rgb(251 191 36 / .08)`. The dark variant brightens the text so it reads on the dark background.

## Design Tokens Used

All from the existing style guide and `theme.css`. No new tokens introduced.

- Gold: `--color-gold` (border, gradient, ember dot, status text)
- Primary/plum: `--color-primary` (border, gradient, graph paper lines)
- Bronze: `--color-bronze`, `--color-bronze-muted` (banner gradient components)
- Line: `--color-line` (inactive status dot)
- Shadows: `--shadow-gold-soft`, `--shadow-primary`
- Typography: `--font-display` (title, tagline, game system), `--font-sans` (status, timestamp)

## Future Work (Not in This PR)

- **Routing-table status in API**: expose whether a campaign is loaded. Wire to `SealedCard`'s `loaded` prop.
- **Activity indicators**: presence information ("your friends are active") in the banner's reserved slot.
- **Role eyebrow**: once the API exposes user role per campaign (GM/Player/Owner), add it to the sealed card head row.
