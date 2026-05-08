import { createFileRoute, redirect } from "@tanstack/react-router";
import { register } from "@teamhanko/hanko-elements";
import { useEffect } from "react";
import { z } from "zod";
import { LoginCookieNotice } from "../components/LoginCookieNotice";
import { ThemeToggle } from "../components/ThemeToggle";
import { hanko, hankoApiUrl } from "../lib/hanko";
import { assetPath, siteLink, spaRoute } from "../lib/paths";
import "../styles/hanko.css";

// `redirect` is the post-login destination - _authed.tsx passes
// location.href when it bounces an unauthed user here, so we can return
// them to the page they were trying to reach. Validated through Zod
// because TanStack search-param schemas are part of the project's
// "Zod at every system boundary" rule.
const loginSearchSchema = z.object({
  redirect: z.string().optional(),
});

const HARBOR_LIGHT_URL = `url('${assetPath("/harbor-for-light.svg")}')`;
const HARBOR_DARK_URL = `url('${assetPath("/harbor-for-dark.svg")}')`;
const RAVEN_URL = `url('${assetPath("/raven-icon.svg")}')`;
const GRID_PATTERN_URL = `url('${assetPath("/grid-pattern.svg")}')`;

function Login(): React.ReactElement {
  useEffect(() => {
    register(hankoApiUrl).catch((error: unknown) => {
      console.error("hanko register failed", error);
    });
    const unsub = hanko.onSessionCreated(() => {
      window.location.assign(spaRoute(""));
    });
    return () => {
      unsub();
    };
  }, []);

  return (
    <div className="relative min-h-screen overflow-hidden bg-background text-foreground">
      {/* Harbor woodcut backdrop. Same mask-image technique as the marketing
        hero: SVG drives the shape, --color-bronze drives the fill, opacity
        differs between themes to keep contrast comparable. */}
      {/* Theme-paired harbor masks. Opacity-toggle (not display-toggle) so
        each layer states its presence in both modes for the linter — the
        off-theme layer composites at opacity 0, negligible cost on a
        static page. */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 bg-bronze opacity-[0.16] dark:opacity-0"
        style={{
          maskImage: HARBOR_LIGHT_URL,
          maskRepeat: "no-repeat",
          maskPosition: "center",
          maskSize: "cover",
          WebkitMaskImage: HARBOR_LIGHT_URL,
          WebkitMaskRepeat: "no-repeat",
          WebkitMaskPosition: "center",
          WebkitMaskSize: "cover",
        }}
      />
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 bg-bronze opacity-0 dark:opacity-[0.22]"
        style={{
          maskImage: HARBOR_DARK_URL,
          maskRepeat: "no-repeat",
          maskPosition: "center",
          maskSize: "cover",
          WebkitMaskImage: HARBOR_DARK_URL,
          WebkitMaskRepeat: "no-repeat",
          WebkitMaskPosition: "center",
          WebkitMaskSize: "cover",
        }}
      />

      {/* Ambient glow orbs. motion-safe gates the pulse for vestibular users. */}
      <div aria-hidden="true" className="pointer-events-none absolute inset-0 opacity-30">
        <div className="absolute top-[12%] left-[18%] size-120 rounded-full bg-primary/30 blur-[140px] motion-safe:animate-pulse" />
        <div
          className="absolute right-[12%] bottom-[10%] size-105 rounded-full bg-gold/25 blur-[120px] motion-safe:animate-pulse"
          style={{ animationDelay: "3s" }}
        />
      </div>

      {/* Cross-hatch texture overlay, very faint. */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 opacity-[0.04] dark:opacity-[0.06]"
        style={{ backgroundImage: GRID_PATTERN_URL }}
      />

      {/* Centered content column. Bottom padding clears the glass cookie
        banner pinned to the viewport bottom. */}
      <div className="relative z-10 flex min-h-screen flex-col items-center justify-center px-6 pt-16 pb-32">
        <a
          href={siteLink("/")}
          aria-label="familiar.systems home"
          className="mb-10 inline-flex items-center gap-3 transition-opacity hover:opacity-80"
        >
          {/* Raven as mask-image so the fill comes from CSS rather than the
            SVG's baked #000000 (which is invisible in dark mode). The plum
            drop-shadow only fires in dark mode and ties the brand glyph to
            the ambient primary-color orbs in the background. */}
          <span
            aria-hidden="true"
            className="block size-10 bg-foreground drop-shadow-none transition-[filter] duration-300 dark:bg-primary dark:drop-shadow-[0_0_10px_var(--color-primary)]"
            style={{
              maskImage: RAVEN_URL,
              maskRepeat: "no-repeat",
              maskPosition: "center",
              maskSize: "contain",
              WebkitMaskImage: RAVEN_URL,
              WebkitMaskRepeat: "no-repeat",
              WebkitMaskPosition: "center",
              WebkitMaskSize: "contain",
            }}
          />
          <span className="font-display text-3xl font-medium tracking-tight text-foreground">
            familiar.systems
          </span>
        </a>

        <div className="w-full max-w-md rounded-2xl border border-foreground/10 bg-background/70 p-8 shadow-2xl shadow-primary/10 backdrop-blur-md">
          <hanko-auth />
        </div>
      </div>

      {/* Glass cookie banner, fixed to the viewport bottom. Lighter
        background than the card so the harbor (densest detail at the
        bottom) reads through the glass. */}
      <div className="fixed inset-x-0 bottom-0 z-10 border-t border-foreground/10 bg-background/50 backdrop-blur-md">
        <LoginCookieNotice />
      </div>

      {/* Theme toggle, last in source order so it stacks on top of the
        full-viewport content column at the same z-index and clicks aren't
        absorbed by the empty area of the centered flex container. */}
      <ThemeToggle className="absolute top-6 right-6 z-10" />
    </div>
  );
}

export const Route = createFileRoute("/login")({
  validateSearch: loginSearchSchema,
  beforeLoad: ({ context }) => {
    // If the user is already authenticated, /login is a dead end -
    // bounce them to the hub so they don't land on the Hanko form
    // unnecessarily and so /me's lazy-provisioning upsert isn't
    // re-triggered for what's effectively a no-op visit.
    if (context.auth.kind === "authed") {
      throw redirect({ to: "/" });
    }
  },
  component: Login,
});
