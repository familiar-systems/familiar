// Step 1: name + tagline. The wizard's most legible step: focus on the
// type, the lede, and the input. No system picker, no privacy buttons.

import { useEffect, useRef } from "react";
import { m } from "../../paraglide/messages.js";

interface StepNameProps {
  name: string;
  tagline: string;
  onChange: (next: { name: string; tagline: string }) => void;
}

export function StepName({ name, tagline, onChange }: StepNameProps): React.ReactElement {
  const nameRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    nameRef.current?.focus();
  }, []);

  return (
    <div className="space-y-8 enter-from-below">
      <header className="space-y-3">
        <p className="text-xs font-medium tracking-[0.28em] text-muted-foreground uppercase">
          {m.stepNameEyebrow()}
        </p>
        {/* Headline stays inline English: the gold-emphasized "in name" is
            inline markup Paraglide's plain-string messages can't carry yet;
            localized with a rich-text helper (Phase 4). */}
        <h2 className="font-display text-3xl leading-tight font-medium tracking-tight md:text-4xl">
          Every world begins <em className="text-gold italic">in name</em>.
        </h2>
        <p className="max-w-xl text-base leading-relaxed text-muted-foreground">
          {m.stepNameLede()}
        </p>
      </header>

      <div className="space-y-6">
        <Field label={m.stepNameNameLabel()} hint={m.stepNameNameHintRequired()} htmlFor="ob-name">
          <input
            id="ob-name"
            ref={nameRef}
            data-testid="wizard-name-input"
            type="text"
            value={name}
            onChange={(e) => {
              onChange({ name: e.target.value, tagline });
            }}
            placeholder={m.stepNameNamePlaceholder()}
            maxLength={80}
            className="w-full rounded-xl border border-foreground/10 bg-background/60 px-4 py-3 font-display text-2xl text-foreground placeholder:text-muted-foreground/60 focus:border-gold/50 focus:ring-2 focus:ring-gold/20 focus:outline-none"
          />
        </Field>

        <Field
          // Label stays inline English: the muted "· optional" suffix is
          // inline markup Paraglide's plain-string messages can't carry yet;
          // localized with a rich-text helper (Phase 4).
          label={
            <>
              Tagline <span className="font-normal text-muted-foreground">· optional</span>
            </>
          }
          hint={m.stepNameTaglineHint()}
          htmlFor="ob-tagline"
        >
          <input
            id="ob-tagline"
            data-testid="wizard-tagline-input"
            type="text"
            value={tagline}
            onChange={(e) => {
              onChange({ name, tagline: e.target.value });
            }}
            placeholder={m.stepNameTaglinePlaceholder()}
            maxLength={140}
            className="w-full rounded-xl border border-foreground/10 bg-background/60 px-4 py-3 text-base text-foreground placeholder:text-muted-foreground/60 focus:border-gold/50 focus:ring-2 focus:ring-gold/20 focus:outline-none"
          />
        </Field>
      </div>
    </div>
  );
}

interface FieldProps {
  label: React.ReactNode;
  hint?: string;
  htmlFor: string;
  children: React.ReactNode;
}

function Field({ label, hint, htmlFor, children }: FieldProps): React.ReactElement {
  return (
    <div className="space-y-2">
      <div className="flex items-baseline justify-between gap-4">
        <label htmlFor={htmlFor} className="text-sm font-medium text-foreground">
          {label}
        </label>
        {hint ? (
          <span className="text-xs tracking-wider text-muted-foreground uppercase">{hint}</span>
        ) : null}
      </div>
      {children}
    </div>
  );
}
