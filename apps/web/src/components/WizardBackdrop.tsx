// Backdrop for the new-campaign wizard. Two layers:
//
//   1. Soft two-tone glow (gold top-right, plum bottom-left), blurred so
//      it reads as atmosphere rather than a feature. Borrowed from the
//      wireframe's `.ob-glow` (tmp/NewCampaignOnboarding/onboarding.css).
//
//   2. Actual graph paper. Minor lines every 24px (faint), major lines
//      every 120px (every 5th, twice as opaque) so the eye gets scale
//      anchors. Drawn via repeating-linear-gradient so the pattern scales
//      without an SVG file. Tinted with the foreground token via
//      `currentColor` so it tracks light/dark themes.
//
// The wireframe's `.ob-grid` referenced an SVG that's actually a diagonal
// weave; that asset ships as `apps/web/public/crosshatch.svg` (formerly
// the misleadingly-named `grid-pattern.svg`) and the hub's EpicBackdrop
// uses it for texture. This component takes "graph paper" literally
// instead.

const MINOR_STEP = "24px";
const MAJOR_STEP = "120px";

export function WizardBackdrop(): React.ReactElement {
  return (
    <>
      {/* Soft two-tone glow. Same recipe as the wireframe's .ob-glow. */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0"
        style={{
          background:
            "radial-gradient(circle at 82% 8%, rgb(184 149 48 / 0.14), transparent 50%), radial-gradient(circle at 12% 92%, rgb(90 74 106 / 0.16), transparent 55%)",
          filter: "blur(90px)",
        }}
      />
      {/* Minor grid: 24px squares, faint. */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 text-foreground opacity-[0.08] dark:opacity-[0.06]"
        style={{
          backgroundImage: `
            repeating-linear-gradient(to right, currentColor 0 1px, transparent 1px ${MINOR_STEP}),
            repeating-linear-gradient(to bottom, currentColor 0 1px, transparent 1px ${MINOR_STEP})
          `,
        }}
      />
      {/* Major grid: every 5th line, layered on top so the intersections
          remain crisp. Higher opacity gives the eye scale anchors without
          shouting. */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 text-foreground opacity-[0.16] dark:opacity-[0.12]"
        style={{
          backgroundImage: `
            repeating-linear-gradient(to right, currentColor 0 1px, transparent 1px ${MAJOR_STEP}),
            repeating-linear-gradient(to bottom, currentColor 0 1px, transparent 1px ${MAJOR_STEP})
          `,
        }}
      />
    </>
  );
}
