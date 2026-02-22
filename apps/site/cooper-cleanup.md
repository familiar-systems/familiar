---

## Verification

1. `pnpm build` in `apps/site` — clean build, no broken imports
2. Grep for `Cooper`, `cooper`, `Gladtek`, `gladtek` in `apps/site/src/` — should only appear in README attribution
3. Grep for deleted component names — no dangling imports
4. `/privacy`, `/terms`, `/license` render correctly in dev
5. Blog post JSON-LD no longer says "Cooper Team"
