/// <reference types="vite/client" />

// `VITE_BASE_PATH` is consumed by vite.config.ts to set Vite's `base`,
// which is then exposed at runtime as `import.meta.env.BASE_URL` (typed
// by vite/client). We do NOT declare VITE_BASE_PATH here - it is a
// build-config input, not a runtime constant. API base paths are
// derived from BASE_URL in src/lib/paths.ts.
interface ImportMetaEnv {
  readonly VITE_HANKO_API_URL: string;
  readonly VITE_SITE_URL: string;
}
interface ImportMeta {
  readonly env: ImportMetaEnv;
}
