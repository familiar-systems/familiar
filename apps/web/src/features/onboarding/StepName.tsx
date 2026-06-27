// Step 1: name + tagline. The wizard's most legible step: focus on the
// type, the lede, and the input. No system picker, no privacy buttons.

import { TextField } from "@familiar-systems/ui";
import { m } from "../../paraglide/messages.js";

interface StepNameProps {
  name: string;
  tagline: string;
  onChange: (next: { name: string; tagline: string }) => void;
}

export function StepName({ name, tagline, onChange }: StepNameProps): React.ReactElement {
  return (
    <div className="space-y-8 enter-from-below">
      <header className="space-y-3">
        <p className="text-xs font-medium tracking-[0.28em] text-muted-foreground uppercase">
          {m.stepNameEyebrow()}
        </p>
        {/* Headline stays inline English: the gold-emphasized "in name" is
            inline markup Paraglide's plain-string messages can't carry yet;
            localized with a rich-text interpolation helper later. */}
        <h2 className="font-display text-3xl leading-tight font-medium tracking-tight md:text-4xl">
          Every world begins <em className="text-gold italic">in name</em>.
        </h2>
        <p className="max-w-xl text-base leading-relaxed text-muted-foreground">
          {m.stepNameLede()}
        </p>
      </header>

      <div className="space-y-6">
        <TextField
          label={m.stepNameNameLabel()}
          hint={m.stepNameNameHintRequired()}
          autoFocus
          value={name}
          onChange={(value) => onChange({ name: value, tagline })}
          placeholder={m.stepNameNamePlaceholder()}
          inputProps={{
            "data-testid": "wizard-name-input",
            maxLength: 80,
            className: "font-display text-2xl",
          }}
        />

        {/* Tagline label stays inline English: the muted "· optional" suffix is inline
            markup the plain-string i18n can't carry until a rich-text interpolation
            helper lands. Optional, signalled by the absent Required hint. */}
        <TextField
          label="Tagline"
          hint={m.stepNameTaglineHint()}
          value={tagline}
          onChange={(value) => onChange({ name, tagline: value })}
          placeholder={m.stepNameTaglinePlaceholder()}
          inputProps={{ "data-testid": "wizard-tagline-input", maxLength: 140 }}
        />
      </div>
    </div>
  );
}
