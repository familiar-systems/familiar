# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Scope

Covers `apps/site/` only: the public Astro static site (landing page, blog, pricing, roadmap, public campaign pages). Overrides the repo-root CLAUDE.md for anything under this directory.

The site ships to three distinct URL roots:

| Environment | Base URL            |
| ----------- | ------------------- |
| Local dev   | `/`                 |
| PR preview  | `/pr-${PR_NUMBER}/` |
| Prod        | `/`                 |

Vite injects the active base into every build. Anything we hardcode to `/` will 404 on previews.

## Preview-safe URLs

**Always derive internal URLs from the active base. Do not write a leading `/` on a route literal and call it done.**

The mechanism differs by file type:

### `.astro` files (server-rendered)

Use `getRelativeLocaleUrl` from `astro:i18n`. It already prepends the configured base, and it routes through Astro's i18n config so language prefixes stay consistent.

```astro
---
import { getRelativeLocaleUrl } from 'astro:i18n';
const { lang } = Astro.params;
---
<a href={getRelativeLocaleUrl(lang, '/pricing/')}>Pricing</a>
```

If you need to walk away from the current locale (e.g. the language picker), strip both base and locale from `Astro.url.pathname` before re-prefixing. See `src/components/common/LanguagePicker.astro` for the canonical regex.

### React islands (`.jsx`/`.tsx`)

`astro:i18n` is server-only and not available in client bundles. Use `import.meta.env.BASE_URL`, which Vite replaces with a literal at build time:

```jsx
const baseUrl = import.meta.env.BASE_URL;
const href = `${baseUrl}${lang}${path}`.replace(/\/+/g, "/");
```

The slash-collapse matters: `baseUrl` is `"/"` in dev/prod and ends with `/`, so naive concat produces `//en/...`.

The same rule applies to public assets fetched at runtime (Pagefind CSS/JS, images served from `/public`). Prefix every URL with `baseUrl`. `src/components/islands/Search.jsx` shows both patterns (popular-link construction and Pagefind asset loading).

### Quick check before merging

Astro reads its `base` from `SITE_BASE_PATH`, which `mise.toml`'s `[env]` block pins to `/` (see `astro.config.mjs:96`). That pin beats a CLI override, so `mise run build:site` always builds at root. To simulate a preview prefix you must bypass the mise env and build the workspace package directly - this is the one place a raw `pnpm --filter` is correct (a `build` runs no install, so the lockfile is safe). Then grep the output for stray root-relative links:

```bash
SITE_BASE_PATH=/pr-99/ pnpm --filter @familiar-systems/site build
grep -rE 'href="/[^p]' apps/site/dist/ | head     # any hit that isn't /pr-99/... is a bug
```

## Commands

```bash
mise run dev          # the whole stack; Astro listens on 4321
mise run build        # all targets
mise run lint
mise run typecheck
```

For site-only iteration, use the scoped `mise` tasks. They wrap the right `pnpm` filter and inject the env the build reads (`PUBLIC_APP_URL`, `VITE_SITE_URL`) that a bare `pnpm --filter` would skip:

```bash
mise run dev:site     # Astro dev server on :4321
mise run build:site   # site-only production build
```

No site-scoped lint/typecheck task exists; run `mise run lint` / `mise run typecheck`.
