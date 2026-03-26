# chronicle.ventures Landing Page Design

## Context

The project is renaming from "Loreweaver" to "chronicle.ventures." There are 5 existing TTRPG products named Loreweaver; this rename establishes a distinct identity. The domain chronicle.ventures is the brand itself (lowercase, TLD included), a play on words: you *chronicle* your *ventures*.

The landing page serves the same purpose as the current loreweaver.no site: introduce the product, explain the core loop, and convert visitors to newsletter subscribers or GitHub followers. This is a rebrand with refreshed aesthetics, not a structural redesign.

## Brand Identity

**Name:** chronicle.ventures (always lowercase, always with TLD)

**Tagline:** "Your table tells the story. We just write it down."

**Tone:** Inclusive, warm, assistive, humble. "Lord of the Rings with friends." Not masculine or tough. The product is a companion, not a power tool. Speak to GMs who are tired of bookkeeping, not power users looking for optimization.

**Theme:** Epic adventure meets fable. Parchment and gold, oceans and voyages. The visual language of old nautical charts and leather-bound journals.

## Design System

### Typography

| Role | Font | Weights | Source |
|------|------|---------|--------|
| Display (headings, hero, section titles) | EB Garamond | 400, 600, 700, italic | Google Fonts |
| Body (paragraphs, descriptions, UI text) | Lora | 400, 600, italic | Google Fonts |

**Rationale:** EB Garamond is Claude Garamond's original 1592 typeface, digitized. It reads as "ancient and trustworthy" without being imposing. Lora has brushed serif curves that feel warm and inviting for body text. Together they say: "a chronicle that's been here for centuries, but come closer, it's comfortable to read." This replaces Cinzel (too carved/imposing for the inclusive direction) and Crimson Pro (functional but less characterful than Lora).

### Color Palette

The palette carries forward from the current site with no changes. It already matches the "Ancient Cartography" direction.

**Light mode:**

| Token | Hex | Usage |
|-------|-----|-------|
| `--background` | `#f8f4ec` | Page background (warm parchment) |
| `--foreground` | `#1c1917` | Primary text |
| `--primary` | `#c49a2b` | Gold accent, buttons, highlights |
| `--muted` | `#ede8dd` | Card backgrounds, borders |
| `--muted-foreground` | `#57534e` | Secondary text |

**Dark mode:**

| Token | Hex | Usage |
|-------|-----|-------|
| `--background` | `#0f0d0a` | Page background (deep charcoal) |
| `--foreground` | `#e7e5e4` | Primary text |
| `--primary` | `#d4a944` | Gold accent (slightly lighter for contrast) |
| `--muted` | `#1c1917` | Card backgrounds |
| `--muted-foreground` | `#a8a29e` | Secondary text |

### Hero Backsplash

A custom harbor/port illustration rendered as SVG, displayed at ~12% opacity ("embossed" prominence) behind the hero text. The illustration depicts a sailing ship, lighthouse, harbor buildings, dock, birds, and distant mountains.

**Assets:**
- `harbor_black_kept.svg` (1536x1024): dark strokes on transparent, for light mode
- `harbor_light_kept.svg` (1536x1024): light strokes on transparent, for dark mode

These are auto-traced from AI-generated raster art. File sizes are ~267KB and ~254KB respectively; will compress significantly with gzip.

**Implementation:** Use as a CSS `background-image` or an absolutely positioned `<img>` within the hero section. Apply the gold tint via CSS (`opacity: 0.12`, with the stroke color matching `--primary`). A radial gradient overlay behind the text ensures readability.

### Temporary Logo

Use Lucide's `Ship` icon as the temporary brand mark. Rendered at the same sizes the current LoreweaverIcon uses. The ship reinforces the "voyage" theme until a proper logo is designed.

## Page Sections

The page retains the same 7-section structure as the current loreweaver.no site. Changes are copy and typography; layout patterns, card components, and grid structures carry forward.

### Section 1: Hero

**Badge:** "AI-Assisted Campaign Notebook"

**Headline:** Chronicle your adventures

**Subtitle:** Every session recorded. Every NPC remembered. A living campaign wiki that grows from the table, not from homework.

**CTAs:**
- Primary: "Read the Vision" (links to vision blog post)
- Secondary: "View on GitHub"

**Background:** Parchment gradient + harbor SVG at embossed opacity. Soft radial gold glow behind text for readability. Animated fade-in on load (existing `fadeUp` animation).

### Section 2: From Table to Tale

**Previous title:** "After Every Session" / "The core loop takes 15 minutes"

**New title:** From table to tale in 15 minutes

**Subtitle:** You played the game. Let the AI do the bookkeeping.

**5-step vertical timeline (same icons):**

1. **Upload your recording or notes** (Upload icon)
   "Audio, video, or written notes from the session. Players can add their characters' perspectives too."

2. **Your session gets processed** (Sparkles icon)
   "Audio is transcribed, events are identified, NPCs, locations, and plot threads are extracted, and a session write-up is drafted."

3. **You get a list of proposals** (ScrollText icon)
   "I found 3 new NPCs. Kael frequents the Rusty Anchor. Tormund appears to be dead. Here's a draft of what happened."

4. **Review and approve** (CheckCircle icon)
   "Accept what looks right, tweak what's close, skip what you don't care about. The AI never changes your world without your say-so."

5. **Done. Your wiki just grew.** (BookOpen icon)
   "15 minutes instead of hours. Everything linked, searchable, and ready for next session's prep."

**Note:** Step 2 was rephrased from "Loreweaver processes everything" to "Your session gets processed" to avoid the awkward "chronicle.ventures processes everything" construction. The brand name works as a noun-phrase (the site, the product) but not as a sentence subject.

### Section 3: Your Campaign Wiki

Same 4-card grid, same icons:

- **Sessions** (ScrollText): Chronological write-ups of what happened
- **Things** (Users): NPCs, locations, items, factions
- **Connections** (Link2): Relationships between entities, searchable graph
- **Player Visibility** (Eye): Control what players can see, progressive reveal

Copy is substantively identical. No "Loreweaver" references to replace (this section already avoids the brand name).

### Section 4: What Makes It Different

Same 4 feature blocks, same icons:

- **MapPin** - "Meets you where you are" (start anywhere, no blank-wiki problem)
- **Clock** - "Tolerant of neglect" (unreviewed suggestions expire, no guilt)
- **MessageSquare** - "AI proposes, you decide" (approval-based, never edits without consent)
- **Swords** - "Captures the mess" (retcons, contradictions, timeline conflicts preserved)

Copy is substantively identical. These are product truths, not brand-specific messaging.

### Section 5: Open Source & Self-Hostable

Same structure: dual icon (GitFork + ShieldCheck), headline, description, GitHub CTA.

Updated to reference chronicle.ventures where the current copy says "Loreweaver."

### Section 6: Latest Posts

Same 3-post grid structure. Content depends on what blog posts exist at launch. The component itself is unchanged.

### Section 7: Final CTA

**Previous headline:** "Stop taking notes. Start playing."

**New headline:** Your table tells the story. We just write it down.

**Subtitle:** chronicle.ventures is in early development. Sign up to follow along.

**Components:** Newsletter signup form (Keila integration) + GitHub link.

## What Does Not Change

- **Astro static site architecture** (SSG, same integrations)
- **Tailwind CSS** (same utility-first approach, same config structure)
- **Color palette** (parchment/gold tokens are unchanged)
- **Component library** (Button, Card, Badge, Section, Container, Grid, etc.)
- **Page structure** (7 sections in same order)
- **Dark mode strategy** (CSS custom properties + `.dark` class)
- **i18n setup** (English only, same properties file approach)
- **Legal pages** (privacy, terms, license, sub-processors)
- **Analytics, cookie consent, newsletter** integrations
- **Responsive breakpoints** (md/lg, same max-widths)

## What Changes

| File/Area | Change |
|-----------|--------|
| Google Fonts import | Cinzel + Crimson Pro replaced with EB Garamond + Lora |
| `theme.css` | Font family variables updated |
| `tailwind.config.mjs` | Font family references updated |
| `en.properties` | All copy strings updated for rebrand |
| `site.config.ts` | Site name, URLs, metadata |
| `HomePage.astro` | Hero headline, core loop title, final CTA, step 2 copy |
| `Header.astro` | Logo component swap (LoreweaverIcon to Ship icon) |
| `Footer.astro` | Brand name references |
| `Layout.astro` | Title, meta tags, OG tags |
| Hero section | Add harbor SVG backsplash with opacity/glow treatment |
| `public/` | Add harbor SVG files, remove old logo PNGs |
| `astro.config.mjs` | Update site URL to chronicle.ventures |
| Legal pages | Update entity name references |

## Verification

1. `pnpm --filter apps/site dev` and visually confirm:
   - Hero renders with EB Garamond headlines, Lora body, harbor backsplash
   - All 7 sections display with updated copy
   - Ship icon appears in header
   - Dark mode toggle swaps SVG variant and maintains readability
   - Responsive layout works at mobile, tablet, desktop breakpoints
2. `pnpm --filter apps/site build` succeeds with no errors
3. Check all internal links (vision blog post, GitHub, legal pages)
4. Lighthouse audit: performance score should not regress (SVG files are lightweight)
