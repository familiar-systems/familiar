import { register } from "@teamhanko/hanko-elements";
import { useEffect } from "react";
import { CookieNotice } from "./components/CookieNotice";
import { ThemeToggle } from "./components/ThemeToggle";
import { hanko, hankoApiUrl } from "./lib/hanko";
import { assetPath, siteLink, spaRoute } from "./lib/paths";

const HARBOR_LIGHT_URL = `url('${assetPath("/harbor-for-light.svg")}')`;
const HARBOR_DARK_URL = `url('${assetPath("/harbor-for-dark.svg")}')`;
const RAVEN_URL = `url('${assetPath("/raven-icon.svg")}')`;
const GRID_PATTERN_URL = `url('${assetPath("/grid-pattern.svg")}')`;
import "./styles/hanko.css";

export function Login() {
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
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 bg-bronze opacity-[0.16] dark:hidden"
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
        className="pointer-events-none absolute inset-0 hidden bg-bronze opacity-[0.22] dark:block"
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
        <div className="absolute top-[12%] left-[18%] h-[480px] w-[480px] rounded-full bg-primary/30 blur-[140px] motion-safe:animate-pulse" />
        <div
          className="absolute bottom-[10%] right-[12%] h-[420px] w-[420px] rounded-full bg-gold/25 blur-[120px] motion-safe:animate-pulse"
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
            className="block h-10 w-10 bg-foreground transition-[filter] duration-300 dark:drop-shadow-[0_0_10px_var(--color-primary)]"
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
        <CookieNotice />
      </div>

      {/* Theme toggle, last in source order so it stacks on top of the
        full-viewport content column at the same z-index and clicks aren't
        absorbed by the empty area of the centered flex container. */}
      <ThemeToggle className="absolute right-6 top-6 z-10" />
    </div>
  );
}
