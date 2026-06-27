// Step 1: name + tagline. The wizard's most legible step: focus on the
// type, the lede, and the input. No system picker, no privacy buttons.

import { TextField } from "@familiar-systems/ui";
import { Trans } from "../../components/Trans";
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
        <h2 className="font-display text-3xl leading-tight font-medium tracking-tight md:text-4xl">
          <Trans
            message={m.stepNameHeading()}
            components={{ gold: (c) => <em className="text-gold italic">{c}</em> }}
          />
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

        {/* Optional field: no Required hint, and "optional" is carried in the
            label text itself rather than as separate styled markup. */}
        <TextField
          label={m.stepNameTaglineLabel()}
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
