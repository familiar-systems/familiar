# @familiar-systems/ui

Accessible component primitives: thin wrappers over [React Aria Components](https://react-spectrum.adobe.com/react-aria/), styled with `@familiar-systems/design` tokens. RTL and locale-aware behavior come from React Aria; dark mode and responsiveness ride the shared tokens.

## Data-injection contract

Components are **pure presentational**: data arrives via props, effects arrive via injected service interfaces (e.g. a `Localization` service, a data/repository service). Nothing here reaches into live stores, the CRDT doc, or the network directly.

Why it matters: it makes components demoable on **facade data** (the marketing site renders the real components inside islands fed by fake services) and deterministically testable. A component that reaches for ambient globals can be neither.

## Stories + tests

Stories are colocated (`*.stories.tsx`) and run as real-browser interaction tests through `apps/web`'s Storybook + Vitest browser tier:

```bash
mise run web:stories   # browser-mode component/interaction tests
mise run storybook     # browse the components visually
```

Colocating here (rather than in `apps/web`) keeps a future dedicated Storybook target a drop-in: it would point at the same stories and `apps/web` drops the glob.
