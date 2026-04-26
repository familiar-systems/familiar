/// <reference path="../.astro/types.d.ts" />

interface ImportMetaEnv {
  readonly DEFAULT_LOCALE: string;
  readonly PUBLIC_APP_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
