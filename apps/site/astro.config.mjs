import { defineConfig } from "astro/config";
import sitemap from "@astrojs/sitemap";
import react from "@astrojs/react";
import mdx from "@astrojs/mdx";
import tailwindcss from "@tailwindcss/vite";
import mermaid from "astro-mermaid";
import fs from "node:fs";
import path from "node:path";
import matter from "gray-matter";

// Helper to find noindex URLs - excludes them from the sitemap
function getNoIndexUrls() {
  const urls = new Set();
  const contentDir = path.resolve("./src/content");
  const pagesDir = path.resolve("./src/pages");

  function scanDir(dir, callback) {
    if (!fs.existsSync(dir)) return;
    const files = fs.readdirSync(dir);
    for (const file of files) {
      const fullPath = path.join(dir, file);
      const stat = fs.statSync(fullPath);
      if (stat.isDirectory()) {
        scanDir(fullPath, callback);
      } else {
        callback(fullPath);
      }
    }
  }

  scanDir(contentDir, (filePath) => {
    if (filePath.endsWith(".md") || filePath.endsWith(".mdx")) {
      try {
        const fileContent = fs.readFileSync(filePath, "utf-8");
        const { data } = matter(fileContent);
        if (data.noindex) {
          let relative = path.relative(contentDir, filePath);
          let urlPath = relative.replace(/\.(md|mdx)$/, "");
          urlPath = urlPath.replace(/\\/g, "/");
          if (!urlPath.startsWith("/")) urlPath = "/" + urlPath;
          urls.add(urlPath);
          urls.add(urlPath + "/");
        }
      } catch (e) {
        console.warn(`Error parsing frontmatter for ${filePath}`, e);
      }
    }
  });

  scanDir(pagesDir, (filePath) => {
    if (filePath.endsWith(".astro")) {
      const content = fs.readFileSync(filePath, "utf-8");
      if (content.includes("noindex={true}")) {
        let relative = path.relative(pagesDir, filePath);
        let urlPath = relative.replace(/\.astro$/, "");
        urlPath = urlPath.replace(/\\/g, "/");

        if (urlPath.endsWith("/index")) {
          urlPath = urlPath.replace(/\/index$/, "") || "/";
        } else if (urlPath === "index") {
          urlPath = "/";
        }

        if (!urlPath.startsWith("/")) urlPath = "/" + urlPath;
        urls.add(urlPath);
        urls.add(urlPath + "/");
      }
    }
  });

  return Array.from(urls);
}

const noIndexUrls = getNoIndexUrls();

const DEFAULT_LOCALE = "en";

// SITE_URL is the canonical URL baked into sitemap, RSS, and absolute links.
// SITE_BASE_PATH mirrors the marketing apex's path prefix.
// Prod: "https://familiar.systems" + "/". Preview: "https://preview.familiar.systems" + "/pr-${PR_NUMBER}/". Dev: Caddy front door + "/".
// See docs/plans/2026-03-30-deployment-architecture.md §URL routing.
// Both are required — no silent fallbacks. mise.toml provides them for local
// dev tasks; CI passes them as Docker build-args.
const SITE_URL = process.env.SITE_URL;
if (!SITE_URL) {
  throw new Error("SITE_URL must be set at build time.");
}
const SITE_BASE_PATH = process.env.SITE_BASE_PATH;
if (!SITE_BASE_PATH) {
  throw new Error("SITE_BASE_PATH must be set at build time.");
}

// https://astro.build/config
export default defineConfig({
  site: SITE_URL,
  base: SITE_BASE_PATH,
  output: "static",
  image: {
    domains: [],
  },
  integrations: [
    sitemap({
      filter: (page) => {
        const url = new URL(page);
        const pathname = url.pathname;
        return !noIndexUrls.includes(pathname);
      },
    }),
    react(),
    mdx(),
    mermaid(),
    (await import("astro-compress")).default({
      Image: true,
      JavaScript: true,
      HTML: false,
    }),
  ],
  vite: {
    plugins: [tailwindcss()],
    define: {
      "import.meta.env.DEFAULT_LOCALE": JSON.stringify(DEFAULT_LOCALE),
    },
  },
  i18n: {
    defaultLocale: DEFAULT_LOCALE,
    locales: ["en"],
    routing: {
      prefixDefaultLocale: true,
      redirectToDefaultLocale: false,
    },
  },
});
