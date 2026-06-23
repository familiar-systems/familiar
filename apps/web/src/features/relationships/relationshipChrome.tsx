// Small presentational pieces shared by the create and edit relationship modals:
// the bronze entity chip and the Players/GM visibility toggle. Extracted so both
// modals render an identical chip and toggle (one DOM/aria shape, one place to
// restyle). Bronze, not gold, on the chip: an entity reference is mechanism, not a
// call to action (style guide: gold = ceremony, bronze = mechanism).

import type { Visibility } from "@familiar-systems/types-campaign";
import { Eye, EyeOff, type LucideIcon } from "lucide-react";

const VIS_ACTIVE: Record<Visibility, string> = {
  players: "bg-gold/15 text-foreground",
  gm: "bg-primary/15 text-primary",
};

export function EntityChip({
  name,
  isNew = false,
}: {
  name: string;
  isNew?: boolean;
}): React.ReactElement {
  return (
    <span className="inline-flex items-baseline rounded bg-bronze/10 px-1.5 py-0.5 font-display font-semibold text-foreground shadow-[inset_0_-1px_0] shadow-bronze/35">
      {name}
      {isNew ? (
        <span className="ml-0.5 font-sans text-[9px] tracking-wide text-muted-foreground uppercase">
          new
        </span>
      ) : null}
    </span>
  );
}

// The two-state visibility control as a radiogroup. Players reveals the fact;
// GM only keeps it secret. Independent of when the fact became true (origin), so
// it sits apart from any lifecycle choice.
export function VisibilityToggle({
  value,
  disabled,
  onChange,
}: {
  value: Visibility;
  disabled: boolean;
  onChange: (v: Visibility) => void;
}): React.ReactElement {
  return (
    <div
      role="radiogroup"
      aria-label="Visibility"
      className="inline-flex overflow-hidden rounded-lg border border-foreground/15"
    >
      <VisButton
        current={value}
        value="players"
        label="Players"
        Icon={Eye}
        disabled={disabled}
        onChange={onChange}
      />
      <VisButton
        current={value}
        value="gm"
        label="GM only"
        Icon={EyeOff}
        disabled={disabled}
        onChange={onChange}
      />
    </div>
  );
}

function VisButton({
  current,
  value,
  label,
  Icon,
  disabled,
  onChange,
}: {
  current: Visibility;
  value: Visibility;
  label: string;
  Icon: LucideIcon;
  disabled: boolean;
  onChange: (v: Visibility) => void;
}): React.ReactElement {
  const active = current === value;
  return (
    <button
      type="button"
      role="radio"
      aria-checked={active}
      disabled={disabled}
      onClick={() => onChange(value)}
      className={[
        "inline-flex items-center gap-1.5 border-foreground/12 px-3 py-1.5 font-sans text-[13px] transition-colors [&+&]:border-l disabled:opacity-50",
        active
          ? `${VIS_ACTIVE[value]} font-semibold`
          : "text-muted-foreground hover:bg-gold/6 hover:text-foreground",
      ].join(" ")}
    >
      <Icon className="size-3.5" aria-hidden="true" />
      {label}
    </button>
  );
}
