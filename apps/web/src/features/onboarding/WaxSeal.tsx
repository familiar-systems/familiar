// Bronze wax seal that tracks the Seal lifecycle:
//   - idle: round, raised, beckoning. Hover lifts subtly.
//   - sealing: pressed, gold ring pulses while the request is in flight.
//   - cracked: a hairline split runs across the disc; the gold ring fades.
//
// The face of the seal is the campaign's monogram (script-aware: "TES" for
// "The Embergrove Saga", first character for CJK names, calligraphic first
// word for Arabic / Hebrew). When no monogram can be derived (empty or
// punctuation-only name), it falls back to the raven mark (our brand). See
// `sealMark.ts` for the derivation rules ported from
// `tmp/NewCampaignOnboarding/wax_seal.jsx`.

import { useEffect, useRef } from "react";
import { FONT_BY_SCRIPT, TRACKING_BY_SCRIPT, sealLayout, sealMark } from "./sealMark";

export type SealState = "idle" | "sealing" | "cracked";

interface WaxSealProps {
  state: SealState;
  campaignName: string;
  /** BCP-47 tag; affects stop-word filtering and casing. */
  locale?: string;
  onClick: () => void;
  disabled?: boolean;
  label?: string;
}

const RAVEN_URL = "url('/raven-icon.svg')";

export function WaxSeal({
  state,
  campaignName,
  locale,
  onClick,
  disabled = false,
  label = "Press to seal",
}: WaxSealProps): React.ReactElement {
  const buttonRef = useRef<HTMLButtonElement>(null);
  const mark = sealMark(campaignName, locale);

  // Move focus to the seal whenever it cracks so screen readers announce
  // the new state and a keyboard user lands on the failure surface.
  useEffect(() => {
    if (state === "cracked" && buttonRef.current) {
      buttonRef.current.focus();
    }
  }, [state]);

  const isSealing = state === "sealing";
  const isCracked = state === "cracked";

  return (
    <div className="flex flex-col items-center gap-4">
      <button
        ref={buttonRef}
        type="button"
        data-testid="wax-seal"
        data-state={state}
        data-glyph={mark === null ? "raven" : "monogram"}
        onClick={onClick}
        disabled={disabled || isSealing}
        aria-label={
          isCracked
            ? "The seal cracked. Try again."
            : mark
              ? `${label} (monogram: ${mark.text})`
              : label
        }
        className={[
          "group relative size-28 rounded-full",
          "transition-transform duration-300",
          "disabled:cursor-not-allowed",
          isSealing ? "scale-95" : "hover:scale-[1.03]",
          isCracked ? "scale-100" : "",
        ].join(" ")}
        style={{
          background:
            "radial-gradient(circle at 30% 25%, color-mix(in srgb, var(--color-bronze) 88%, white 12%), var(--color-bronze) 60%, color-mix(in srgb, var(--color-bronze) 75%, black 25%) 100%)",
          boxShadow: isCracked
            ? "0 6px 16px -8px rgb(0 0 0 / 0.45), inset 0 -2px 4px rgb(0 0 0 / 0.3)"
            : "0 12px 28px -10px rgb(0 0 0 / 0.55), inset 0 2px 4px rgb(255 255 255 / 0.18), inset 0 -3px 6px rgb(0 0 0 / 0.35)",
        }}
      >
        {/* Inner ring — gold pulse during sealing. */}
        <span
          aria-hidden="true"
          className={[
            "absolute inset-3 rounded-full ring-1",
            isSealing ? "animate-pulse ring-gold/60" : "ring-gold/30",
            isCracked ? "ring-foreground/10 dark:ring-white/15" : "",
          ].join(" ")}
        />
        {/* Glyph: monogram (preferred) or raven fallback. */}
        {mark ? (
          <SealGlyphMonogram mark={mark} dimmed={isCracked} />
        ) : (
          <SealGlyphRaven dimmed={isCracked} />
        )}
        {/* Crack: thin diagonal that appears only when the seal fails. */}
        {isCracked ? (
          <span
            aria-hidden="true"
            data-testid="wax-seal-crack"
            className="absolute inset-0 rounded-full"
            style={{
              background:
                "linear-gradient(115deg, transparent 47%, color-mix(in srgb, var(--color-bronze), black 55%) 49%, transparent 53%)",
            }}
          />
        ) : null}
      </button>
      <span
        className={[
          "text-xs tracking-[0.18em] uppercase",
          isCracked ? "text-foreground/70" : "text-muted-foreground",
        ].join(" ")}
      >
        {isCracked ? "The seal cracked" : isSealing ? "Sealing..." : label}
      </span>
    </div>
  );
}

interface MonogramProps {
  mark: NonNullable<ReturnType<typeof sealMark>>;
  dimmed: boolean;
}

// Renders the campaign's monogram inside the wax disc. SVG textLength keeps
// long initials inside the ring no matter the script; gradient fill +
// drop-shadows give the embossed look.
function SealGlyphMonogram({ mark, dimmed }: MonogramProps): React.ReactElement {
  const { text, script } = mark;
  const { targetLen, fontSize } = sealLayout(mark);
  const font = FONT_BY_SCRIPT[script];
  const tracking = TRACKING_BY_SCRIPT[script];

  return (
    <span
      aria-hidden="true"
      data-testid="wax-seal-monogram"
      data-script={script}
      className="absolute inset-4 transition-opacity"
      style={{ opacity: dimmed ? 0.5 : 1 }}
    >
      <svg viewBox="0 0 100 100" width="100%" height="100%" preserveAspectRatio="xMidYMid meet">
        <defs>
          <linearGradient id="seal-text-fill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="#f3c8b8" />
            <stop offset="35%" stopColor="#c46757" />
            <stop offset="75%" stopColor="#6b1813" />
            <stop offset="100%" stopColor="#2b0606" />
          </linearGradient>
        </defs>
        <text
          x="50"
          y="58"
          textAnchor="middle"
          dominantBaseline="middle"
          textLength={targetLen}
          lengthAdjust="spacingAndGlyphs"
          style={{
            fontFamily: font,
            fontWeight: 600,
            fontSize: `${fontSize}px`,
            letterSpacing: tracking,
            fill: "url(#seal-text-fill)",
            filter:
              "drop-shadow(0 1px 0 rgba(255,200,180,0.35)) drop-shadow(0 -1px 0 rgba(0,0,0,0.55))",
          }}
        >
          {text}
        </text>
      </svg>
    </span>
  );
}

function SealGlyphRaven({ dimmed }: { dimmed: boolean }): React.ReactElement {
  return (
    <span
      aria-hidden="true"
      data-testid="wax-seal-raven"
      className="absolute inset-6 bg-background/85 transition-opacity"
      style={{
        maskImage: RAVEN_URL,
        maskRepeat: "no-repeat",
        maskPosition: "center",
        maskSize: "contain",
        WebkitMaskImage: RAVEN_URL,
        WebkitMaskRepeat: "no-repeat",
        WebkitMaskPosition: "center",
        WebkitMaskSize: "contain",
        opacity: dimmed ? 0.5 : 1,
      }}
    />
  );
}
