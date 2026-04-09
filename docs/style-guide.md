# Visual Style Guide

The familiar.systems site uses a warm, parchment-inspired palette with gold accents and serif typography. This guide documents the design system as implemented in `apps/site/`.

## Color Palette

All colors are defined as CSS custom properties in `src/styles/theme.css` and mapped through Tailwind.

### Theme Tokens

| Token                  | Light                  | Dark                    | Usage                        |
| ---------------------- | ---------------------- | ----------------------- | ---------------------------- |
| `--background`         | `#f8f4ec` (parchment)  | `#0f0d0a` (charcoal)    | Page background              |
| `--foreground`         | `#1c1917` (dark brown) | `#e7e5e4` (light stone) | Body text                    |
| `--color-primary`      | `#c49a2b` (gold)       | `#d4a944` (bright gold) | Accents, links, CTAs         |
| `--muted`              | `#ede8dd` (beige)      | `#1c1917` (dark brown)  | Subdued backgrounds          |
| `--muted-foreground`   | `#57534e` (brown)      | `#a8a29e` (stone)       | Secondary text, descriptions |
| `--primary-foreground` | `#1c1917`              | `#0f0d0a`               | Text on primary backgrounds  |

### Opacity Conventions

Colors are used at varying opacities rather than defining new color tokens:

- **Tinted backgrounds**: `bg-primary/5`, `bg-primary/10`, `bg-foreground/[0.02]`
- **Borders**: `border-primary/20`, `border-foreground/5`, `border-foreground/10`
- **Text hierarchy**: `text-foreground`, `text-foreground/80`, `text-muted-foreground`, `text-muted-foreground/60`
- **Shadows**: `shadow-primary/10`, `shadow-primary/25`

### Semantic Colors (Non-token)

Used sparingly for status indicators and specific UI elements:

- **Success/done**: `text-emerald-400`, `border-emerald-400/20`
- **Warning**: `text-amber-500`
- **Checkmark green**: `text-green-500`
- **Accent blue**: `bg-blue-600/10`, `text-blue-700 dark:text-blue-300`
- **Destructive red**: `bg-red-600/10`, `text-red-700 dark:text-red-400`

## Typography

### Font Families

| Role    | Font        | Fallback       | Tailwind class | Usage                              |
| ------- | ----------- | -------------- | -------------- | ---------------------------------- |
| Body    | Crimson Pro | Georgia, serif | `font-sans`    | All body text, default             |
| Display | Cinzel      | Georgia, serif | `font-display` | Headlines, brand name, pull quotes |

Both are serif fonts. The site has no sans-serif font in use.

### Heading Hierarchy

| Level | Size                         | Weight | Extra                             |
| ----- | ---------------------------- | ------ | --------------------------------- |
| H1    | `clamp(2.5rem, 6vw, 3.5rem)` | 800    | Border-bottom, `-0.02em` tracking |
| H2    | `1.75rem`                    | 700    | Border-bottom                     |
| H3    | `1.5rem`                     | 600    |                                   |
| H4    | `1.25rem`                    | 600    |                                   |

In page components, headings are often larger than the prose defaults:

- Hero H1: `text-5xl md:text-7xl lg:text-8xl`
- Section H2: `text-3xl md:text-4xl`
- Card H3: `text-xl`

### Text Patterns

- **Body**: `text-base leading-7`
- **Subtitles**: `text-xl md:text-2xl text-muted-foreground`
- **Small/meta**: `text-sm text-muted-foreground`
- **Tiny labels**: `text-xs uppercase tracking-[0.2em]` or `tracking-widest`
- **Large body**: `text-lg leading-relaxed` or `text-[1.05rem]`

## Spacing and Layout

### Container Widths

| Width  | Class       | Usage                               |
| ------ | ----------- | ----------------------------------- |
| 1280px | `max-w-7xl` | Full-width sections (footer, nav)   |
| 896px  | `max-w-4xl` | Content pages, prose, feature grids |
| 768px  | `max-w-3xl` | Centered CTAs, narrow sections      |
| 672px  | `max-w-2xl` | Subtitle text blocks                |

All containers use `mx-auto px-6`.

### Section Padding

- **Major sections**: `py-24 md:py-32`
- **Standard sections**: `py-16 md:py-24`
- **Compact sections**: `py-12`
- **Page top (below header)**: `pt-32 pb-24`

### Grid Patterns

- **Two-column features**: `grid grid-cols-1 md:grid-cols-2 gap-6`
- **Three-column blog**: `grid grid-cols-1 md:grid-cols-3 gap-8`
- **Bio/about split**: `grid grid-cols-1 md:grid-cols-2 gap-10`

### Common Gaps

- `gap-4` (1rem) - default flex spacing
- `gap-6` (1.5rem) - nav links, step items
- `gap-8` (2rem) - grid items, section internals
- `gap-10` (2.5rem) - feature rows with icons

## Components

### Buttons

All buttons: `inline-flex items-center justify-center font-medium transition-colors`.

| Variant   | Classes                                                                              |
| --------- | ------------------------------------------------------------------------------------ |
| Primary   | `bg-primary text-white hover:bg-primary/90 shadow-lg shadow-primary/25`              |
| Secondary | `bg-foreground/5 text-foreground hover:bg-foreground/10 border border-foreground/10` |
| Outline   | `border border-foreground/10 bg-transparent hover:bg-foreground/5`                   |
| Ghost     | `hover:bg-primary/5 text-primary`                                                    |
| Icon      | `p-0 aspect-square h-9 w-9 rounded-full`                                             |

Sizes: `sm` (`text-xs`), `md` (`text-sm`, default), `lg` (`text-base`).

Hero/CTA buttons use `rounded-full px-8 py-4`. Standard buttons use `rounded-lg`.

### Cards

```
block p-6 rounded-2xl
border border-black/5 dark:border-white/10
bg-black/5 dark:bg-white/5
```

Interactive cards add: `hover:border-primary/20 hover:shadow-2xl hover:shadow-primary/10 hover:-translate-y-1 transition-all duration-300`.

Feature cards use `p-8 rounded-2xl` with a `w-12 h-12 rounded-xl bg-primary/10` icon container.

### Badges

```
inline-flex items-center px-3 py-1 rounded-full text-xs font-medium
```

Variants: default, outline, secondary, accent (blue), destructive (red). All use `backdrop-blur-sm`.

### Border Radius Scale

| Value          | Usage                             |
| -------------- | --------------------------------- |
| `rounded-full` | Buttons (pill), badges, avatars   |
| `rounded-3xl`  | Large feature cards, testimonials |
| `rounded-2xl`  | Standard cards, sections          |
| `rounded-xl`   | Icon containers, code blocks      |
| `rounded-lg`   | Standard buttons, inputs          |

## Visual Effects

### Backdrop Blur

- **Header**: `backdrop-blur-md` with `bg-background/80`
- **Cards/badges**: `backdrop-blur-sm`

### Shadows

- Primary glow: `shadow-lg shadow-primary/25` (buttons), `shadow-primary/10` (hover)
- Custom ambient: `shadow-[0_0_40px_rgba(196,154,43,0.04)]`
- Cards on hover: `shadow-2xl shadow-primary/10`

### Animations

**fadeUp** (page entrance):

```css
@keyframes fadeUp {
    from {
        opacity: 0;
        transform: translateY(20px);
    }
    to {
        opacity: 1;
        transform: translateY(0);
    }
}
animation: fadeUp 0.8s ease-out forwards;
```

**Hover patterns**:

- Scale: `group-hover:scale-105` (images), `group-hover:scale-110` (icons)
- Rotate: `group-hover:rotate-6` (feature icons)
- Lift: `hover:-translate-y-1` (cards)
- Border glow: `hover:border-primary/20` or `hover:border-primary/50`
- All with `transition-all duration-300` or `duration-500`

### Hero Ambient Glow

Large blurred circles behind the hero section:

```html
<div class="bg-primary/40 rounded-full blur-[140px] animate-pulse"></div>
<div class="bg-amber-700/30 rounded-full blur-[120px] animate-pulse"></div>
```

## Icons

**Library**: `lucide-react`

| Context         | Size                   |
| --------------- | ---------------------- |
| Inline/nav      | `w-4 h-4`              |
| Navigation      | `w-5 h-5`              |
| Feature cards   | `w-6 h-6`              |
| Step indicators | `w-7 h-7` to `w-8 h-8` |

Icon color is typically `text-primary`. Brand icons (GitHub, Bluesky, LinkedIn) are custom SVG components in `components/icons/BrandIcons.tsx`.

## Dark Mode

- Activated via `.dark` class on the document root
- Persisted to `localStorage` with key `"theme"`
- Tailwind config: `darkMode: "class"`
- Theme toggle in header swaps Sun/Moon icons
- Smooth transition: `transition: background-color 0.3s, color 0.3s` on body

All components use Tailwind dark variants (`dark:border-white/10`, `dark:bg-white/5`) rather than separate dark stylesheets. The CSS custom properties handle the primary color shift automatically.

## Anti-patterns

Things deliberately absent from the site:

- **No emoji** in any content or UI
- **No exclamation marks** in marketing copy (periods create calm confidence)
- **No flowery or magical language** in marketing, despite the fantasy product domain
- **No superlatives** ("best", "most advanced") without specific evidence
- **No urgency language** ("Act now", "Limited time", "Don't miss out")
- **No em dashes**. Use periods, commas, colons, or parentheses instead
