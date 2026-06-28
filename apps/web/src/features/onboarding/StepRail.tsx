// Horizontal step indicator. Sits across the top of the wizard so the
// step content gets the full vertical viewport on mobile (where vertical
// space is scarcest) and the content column gets the full horizontal
// width on desktop (where the system picker wants to breathe).
//
// Why not the style guide's vertical Step Timeline? That pattern is for
// the marketing site's "After Every Session" scroll story, where the
// timeline IS the content. A wizard's rail is navigation chrome over
// active content; horizontal is the conventional shape and matches the
// wireframe at tmp/NewCampaignOnboarding/onboarding.jsx:14-28.
//
// Mobile (default): numbered dots with thin connectors. Step labels are
// hidden to keep the rail compact. The active step's headline appears in
// the step body, so the label-on-dot is redundant on small viewports.
// Desktop (md+): labels reappear inline next to each dot.

import { Check } from "lucide-react";
import { m } from "../../paraglide/messages.js";

interface StepRailProps {
  current: number;
  steps: readonly { id: string; label: string }[];
}

export function StepRail({ current, steps }: StepRailProps): React.ReactElement {
  return (
    <ol
      className="flex items-center gap-1 md:gap-3"
      aria-label={m.stepRailAriaLabel()}
      data-testid="step-rail"
    >
      {steps.map((step, i) => {
        const state = i < current ? "done" : i === current ? "active" : "todo";
        const isLast = i === steps.length - 1;
        return (
          <li
            key={step.id}
            data-state={state}
            data-step={step.id}
            className="flex flex-1 items-center gap-2 md:gap-3"
          >
            <span
              aria-hidden="true"
              className={[
                "flex size-9 shrink-0 items-center justify-center rounded-full",
                "border transition-colors duration-300",
                state === "active"
                  ? "border-gold/60 bg-[var(--color-step-bg)] text-gold shadow-md shadow-gold/15"
                  : state === "done"
                    ? "border-foreground/15 bg-[var(--color-step-bg)] text-foreground/80"
                    : "border-foreground/10 bg-background/40 text-muted-foreground/60",
              ].join(" ")}
            >
              {state === "done" ? (
                <Check className="size-4" />
              ) : (
                <span className="font-display text-sm font-semibold">{i + 1}</span>
              )}
            </span>
            <span
              className={[
                "hidden font-display text-sm tracking-tight md:inline",
                state === "todo" ? "text-muted-foreground/70" : "text-foreground",
              ].join(" ")}
            >
              {step.label}
            </span>
            {!isLast ? (
              <span
                aria-hidden="true"
                className={[
                  "h-px flex-1 transition-colors duration-300",
                  state === "done" ? "bg-foreground/25" : "bg-foreground/10",
                ].join(" ")}
              />
            ) : null}
          </li>
        );
      })}
    </ol>
  );
}
