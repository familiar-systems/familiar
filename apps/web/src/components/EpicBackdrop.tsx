import { assetPath } from "../lib/paths";

const EPIC_LIGHT_URL = `url('${assetPath("/epic-square-for-light.svg")}')`;
const EPIC_DARK_URL = `url('${assetPath("/epic-square-for-dark.svg")}')`;
const CROSSHATCH_URL = `url('${assetPath("/crosshatch.svg")}')`;

// The post-auth hub backdrop: a 3x3 fluid epic of an adventuring party,
// rendered through a CSS mask so the bronze fill comes from --color-bronze
// rather than the SVG's baked colors. Same recipe as the login harbor
// backdrop. The 1:1 SVG cover-fits the longer viewport axis: clipped
// top/bottom on landscape, left/right on portrait.
export function EpicBackdrop(): React.ReactElement {
  return (
    <>
      {/* Theme-paired masks. Opacity-toggle (rather than display-toggle) so
        each element states its presence in both modes — keeps the lint's
        no-dark-without-light rule satisfied without per-line suppressions.
        Light: bronze (#5c3a1f) at 0.16, warm walnut. Dark: lit bronze
        (#c4956b) at 0.22, slightly brighter so the artwork holds on the
        deep charcoal background. */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 bg-bronze opacity-[0.16] dark:opacity-0"
        style={{
          maskImage: EPIC_LIGHT_URL,
          maskRepeat: "no-repeat",
          maskPosition: "center",
          maskSize: "cover",
          WebkitMaskImage: EPIC_LIGHT_URL,
          WebkitMaskRepeat: "no-repeat",
          WebkitMaskPosition: "center",
          WebkitMaskSize: "cover",
        }}
      />
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 bg-bronze opacity-0 dark:opacity-[0.22]"
        style={{
          maskImage: EPIC_DARK_URL,
          maskRepeat: "no-repeat",
          maskPosition: "center",
          maskSize: "cover",
          WebkitMaskImage: EPIC_DARK_URL,
          WebkitMaskRepeat: "no-repeat",
          WebkitMaskPosition: "center",
          WebkitMaskSize: "cover",
        }}
      />

      {/* Ambient glow orbs. motion-safe gates the pulse for vestibular users. */}
      <div aria-hidden="true" className="pointer-events-none absolute inset-0 opacity-25">
        <div className="absolute top-[14%] left-[16%] size-120 rounded-full bg-primary/30 blur-[140px] motion-safe:animate-pulse" />
        <div
          className="absolute right-[10%] bottom-[12%] size-105 rounded-full bg-gold/25 blur-[120px] motion-safe:animate-pulse"
          style={{ animationDelay: "3s" }}
        />
      </div>

      {/* Cross-hatch texture, very faint. */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 opacity-[0.04] dark:opacity-[0.06]"
        style={{ backgroundImage: CROSSHATCH_URL }}
      />

      {/* Vignette: top + bottom fade so the nav and footer area read
        cleanly over the artwork. The dark variant adds a soft radial scrim
        around the page center to keep the hero type unambiguously legible. */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 opacity-100 dark:opacity-0"
        style={{
          background:
            "linear-gradient(180deg, color-mix(in srgb, var(--background), transparent 40%) 0%, transparent 18%, transparent 70%, color-mix(in srgb, var(--background), transparent 40%) 100%)",
        }}
      />
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 opacity-0 dark:opacity-100"
        style={{
          background:
            "radial-gradient(ellipse at 50% 30%, transparent 25%, color-mix(in srgb, var(--background), transparent 30%) 75%), linear-gradient(180deg, color-mix(in srgb, var(--background), transparent 50%) 0%, transparent 22%, transparent 55%, color-mix(in srgb, var(--background), transparent 20%) 100%)",
        }}
      />
    </>
  );
}
