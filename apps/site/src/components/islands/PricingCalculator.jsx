import { useMemo, useState } from "react";
import { AnimatePresence, motion } from "motion/react";

const CAPTURE_METHODS = [
  { value: "notes", label: "Type or upload notes" },
  { value: "tabletop", label: "Record at the table" },
  { value: "discord", label: "Record on Discord" },
];

const CADENCES = [
  { value: "weekly", label: "Weekly", sessionsPerMonth: 52 / 12 },
  { value: "biweekly", label: "Biweekly", sessionsPerMonth: 26 / 12 },
  { value: "monthly", label: "Monthly", sessionsPerMonth: 1 },
];

const NOTEBOOK_PRICE = 6;
const AUDIO_BASE_PRICE = 12;
const INCLUDED_AUDIO_HOURS = 8;
const OVERAGE_PRICE_PER_HOUR = 1;

const SLIDER_MIN = 2;
const SLIDER_MAX = 8;

function formatHours(hours) {
  const rounded = Math.round(hours * 10) / 10;
  return Number.isInteger(rounded) ? rounded.toString() : rounded.toFixed(1);
}

function computeEstimate(captureMethod, cadence, sessionHours) {
  if (captureMethod === "notes") {
    return {
      cost: NOTEBOOK_PRICE,
      breakdown: "Notebook plan, all features included.",
    };
  }
  const cadenceData = CADENCES.find((c) => c.value === cadence);
  if (!cadenceData) {
    return { cost: AUDIO_BASE_PRICE, breakdown: null };
  }
  const totalHoursPerMonth = cadenceData.sessionsPerMonth * sessionHours;
  const overageHours = Math.max(0, totalHoursPerMonth - INCLUDED_AUDIO_HOURS);
  const cost = AUDIO_BASE_PRICE + overageHours * OVERAGE_PRICE_PER_HOUR;
  const formatted = formatHours(totalHoursPerMonth);
  const breakdown =
    overageHours <= 0
      ? `Notebook + Audio with ~${formatted} hr/mo recorded (within included 8 hr).`
      : `Notebook + Audio with ~${formatted} hr/mo recorded (8 included + ${Math.round(overageHours)} extra at 1 EUR).`;
  return { cost, breakdown };
}

function ChipGroup({ legend, value, options, onChange }) {
  return (
    <fieldset className="border-0 p-0 m-0">
      <legend className="text-xs font-medium tracking-[0.25em] uppercase text-muted-foreground mb-3">
        {legend}
      </legend>
      <div className="flex flex-wrap gap-2">
        {options.map((option) => {
          const active = option.value === value;
          return (
            <button
              key={option.value}
              type="button"
              aria-pressed={active}
              onClick={() => onChange(option.value)}
              className={`px-5 py-2.5 rounded-full border text-sm font-medium transition-all duration-200 cursor-pointer ${
                active
                  ? "bg-gold/15 border-gold/50 text-foreground shadow-sm shadow-gold/10"
                  : "bg-background/40 border-foreground/15 text-muted-foreground hover:text-foreground hover:border-foreground/30"
              }`}
            >
              {option.label}
            </button>
          );
        })}
      </div>
    </fieldset>
  );
}

export default function PricingCalculator() {
  const [captureMethod, setCaptureMethod] = useState("notes");
  const [cadence, setCadence] = useState("biweekly");
  const [sessionHours, setSessionHours] = useState(3.5);

  const { cost, breakdown } = useMemo(
    () => computeEstimate(captureMethod, cadence, sessionHours),
    [captureMethod, cadence, sessionHours],
  );

  const displayCost = Math.round(cost);
  const showCadenceAndHours = captureMethod !== "notes";
  const sliderPct = ((sessionHours - SLIDER_MIN) / (SLIDER_MAX - SLIDER_MIN)) * 100;

  return (
    <div className="relative overflow-hidden rounded-2xl border border-foreground/10 bg-bronze-muted/40 dark:bg-bronze-muted/30 p-8 md:p-12">
      <div
        className="absolute top-0 left-0 right-0 h-px bg-linear-to-r from-transparent via-gold to-transparent"
        aria-hidden="true"
      />
      <div
        className="pointer-events-none absolute -bottom-24 left-1/2 -translate-x-1/2 w-[420px] h-[280px] rounded-full bg-primary/15 blur-3xl"
        aria-hidden="true"
      />

      <div className="relative flex flex-col gap-8">
        <ChipGroup
          legend="How do you capture your sessions?"
          value={captureMethod}
          options={CAPTURE_METHODS}
          onChange={setCaptureMethod}
        />

        <AnimatePresence mode="wait" initial={false}>
          {showCadenceAndHours && (
            <motion.div
              key="cadence-and-hours"
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: "auto" }}
              exit={{ opacity: 0, height: 0 }}
              transition={{ duration: 0.3 }}
              className="flex flex-col gap-8 overflow-hidden"
            >
              <ChipGroup
                legend="How often do you play?"
                value={cadence}
                options={CADENCES}
                onChange={setCadence}
              />

              <div>
                <div className="flex items-baseline justify-between mb-3">
                  <label
                    htmlFor="session-hours"
                    className="text-xs font-medium tracking-[0.25em] uppercase text-muted-foreground"
                  >
                    How long are your sessions?
                  </label>
                  <span className="font-display text-xl text-foreground tabular-nums">
                    {formatHours(sessionHours)}
                    <span className="text-sm text-muted-foreground ml-1">hours</span>
                  </span>
                </div>
                <input
                  id="session-hours"
                  type="range"
                  min={SLIDER_MIN}
                  max={SLIDER_MAX}
                  step={0.5}
                  value={sessionHours}
                  onChange={(e) => setSessionHours(Number.parseFloat(e.target.value))}
                  className="pricing-slider w-full appearance-none cursor-pointer bg-foreground/15 rounded-full"
                  style={{
                    backgroundImage: `linear-gradient(to right, var(--color-gold) 0%, var(--color-gold) ${sliderPct}%, transparent ${sliderPct}%, transparent 100%)`,
                  }}
                />
                <div
                  className="mt-3 flex justify-between text-[0.7rem] tracking-[0.2em] uppercase text-muted-foreground/80"
                  aria-hidden="true"
                >
                  <span>2 hr</span>
                  <span>4 hr</span>
                  <span>6 hr</span>
                  <span>8 hr</span>
                </div>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      <div className="relative mt-10 rounded-xl border border-gold/20 bg-background/50 backdrop-blur-sm p-8 text-center overflow-hidden">
        <div
          className="absolute top-0 left-0 right-0 h-px bg-linear-to-r from-transparent via-gold/60 to-transparent"
          aria-hidden="true"
        />
        <p className="text-xs font-medium tracking-[0.25em] uppercase text-bronze dark:text-gold mb-3">
          Estimated monthly cost
        </p>
        <div className="relative h-20 md:h-24 flex items-center justify-center">
          <AnimatePresence mode="wait" initial={false}>
            <motion.div
              key={displayCost}
              initial={{ opacity: 0, y: 8 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -8 }}
              transition={{ duration: 0.18 }}
              className="font-display text-6xl md:text-7xl font-bold text-bronze dark:text-gold tracking-tight tabular-nums absolute"
            >
              €{displayCost}
            </motion.div>
          </AnimatePresence>
        </div>
        {breakdown && (
          <p className="text-sm text-foreground/75 mt-2 max-w-md mx-auto leading-relaxed">
            {breakdown}
          </p>
        )}
        <div
          className="mt-5 flex items-center justify-center gap-3"
          aria-hidden="true"
        >
          <span className="h-px w-12 bg-linear-to-r from-transparent to-gold/40" />
          <span className="w-1 h-1 rounded-full bg-gold/50" />
          <span className="h-px w-12 bg-linear-to-r from-gold/40 to-transparent" />
        </div>
        <p className="text-xs text-muted-foreground/80 mt-3 italic">
          Estimate based on preliminary pricing.
        </p>
      </div>

      <style>{`
        .pricing-slider {
          height: 6px;
        }
        .pricing-slider::-webkit-slider-runnable-track {
          height: 6px;
          border-radius: 9999px;
          background: transparent;
        }
        .pricing-slider::-moz-range-track {
          height: 6px;
          border-radius: 9999px;
          background: transparent;
        }
        .pricing-slider::-webkit-slider-thumb {
          appearance: none;
          width: 1.25rem;
          height: 1.25rem;
          margin-top: -0.4375rem;
          border-radius: 9999px;
          background: var(--color-gold);
          border: 2px solid var(--background);
          box-shadow: 0 0 0 0 rgba(184, 149, 48, 0), 0 1px 3px rgba(0, 0, 0, 0.18);
          cursor: pointer;
          transition: box-shadow 0.2s ease, transform 0.15s ease;
        }
        .pricing-slider:hover::-webkit-slider-thumb,
        .pricing-slider:focus-visible::-webkit-slider-thumb {
          box-shadow: 0 0 0 6px rgba(184, 149, 48, 0.18), 0 1px 3px rgba(0, 0, 0, 0.18);
        }
        .pricing-slider:active::-webkit-slider-thumb {
          transform: scale(1.05);
        }
        .pricing-slider::-moz-range-thumb {
          width: 1.25rem;
          height: 1.25rem;
          border-radius: 9999px;
          background: var(--color-gold);
          border: 2px solid var(--background);
          box-shadow: 0 1px 3px rgba(0, 0, 0, 0.18);
          cursor: pointer;
          transition: box-shadow 0.2s ease;
        }
        .pricing-slider:hover::-moz-range-thumb,
        .pricing-slider:focus-visible::-moz-range-thumb {
          box-shadow: 0 0 0 6px rgba(184, 149, 48, 0.18), 0 1px 3px rgba(0, 0, 0, 0.18);
        }
        .pricing-slider:focus {
          outline: none;
        }
      `}</style>
    </div>
  );
}
