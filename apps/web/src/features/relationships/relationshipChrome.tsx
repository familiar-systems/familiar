// Small presentational pieces shared by the create and edit relationship modals:
// the bronze entity chip and the knowledge control (the two-segment "To the players"
// toggle). Extracted so both modals render an identical chip and control (one DOM/aria
// shape, one place to restyle). Bronze, not gold, on the chip: an entity reference
// is mechanism, not a call to action (style guide: gold = ceremony, bronze =
// mechanism).

import type { KnowledgeInput, SessionId, SessionRef } from "@familiar-systems/types-campaign";
import { SegmentedControl, SegmentedItem } from "@familiar-systems/ui";
import { Eye, EyeOff } from "lucide-react";

// The Prior sentinel for the "as of" select: session ids are ULIDs, so a plain word
// can't collide. Only the create modal offers it (a fact can originate before play);
// edits pick from live sessions only.
const PRIOR_VALUE = "prior";

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
        <span className="ms-0.5 font-sans text-[9px] tracking-wide text-muted-foreground uppercase">
          new
        </span>
      ) : null}
    </span>
  );
}

// The session "as of" picker shared by the create modal (origin), the edit modal
// (end / retcon as-of), and the reveal session below. One DOM/aria/styling shape and
// one "(current)" suffix rule, so the three can't drift (the reveal select had lost
// the suffix before this was unified). `prior` adds the create-only "before the
// campaign" option; `value` is null only for a not-yet-chosen pick.
export function SessionSelect({
  id,
  ariaLabel,
  sessions,
  current,
  value,
  onSelect,
  disabled = false,
  prior,
}: {
  id?: string;
  ariaLabel?: string;
  sessions: SessionRef[];
  current: SessionRef | null;
  value: SessionId | null;
  onSelect: (id: SessionId) => void;
  disabled?: boolean;
  prior?: { label: string; selected: boolean; onSelect: () => void };
}): React.ReactElement {
  return (
    <select
      id={id}
      aria-label={ariaLabel}
      value={prior?.selected === true ? PRIOR_VALUE : (value ?? "")}
      disabled={disabled}
      onChange={(e) => {
        const v = e.target.value;
        if (prior !== undefined && v === PRIOR_VALUE) {
          prior.onSelect();
          return;
        }
        const match = sessions.find((s) => s.id === v);
        if (match !== undefined) onSelect(match.id);
      }}
      className="rounded border border-gold/40 bg-background/60 px-2 py-1 font-sans text-xs text-foreground focus:border-gold/60 focus:outline-none disabled:opacity-50"
    >
      {prior !== undefined ? <option value={PRIOR_VALUE}>{prior.label}</option> : null}
      {sessions.map((s) => (
        <option key={s.id} value={s.id}>
          Session {s.ordinal}
          {current !== null && s.id === current.id ? " (current)" : ""}
        </option>
      ))}
    </select>
  );
}

// The knowledge axis as a control producing a `KnowledgeInput`, ported from the
// wireframe's two-segment "To the players" toggle: [Hidden] [Revealed / Public].
// Knowledge is freely mutable - clicking Hidden conceals (even a once-public fact),
// clicking the other segment reveals. `bornSecret` is the fact's secret bit frozen at
// the control's opening value (create passes `false`); it decides only what the right
// segment means: a plain "Public" (no session) for a fact that opened public, or
// "Revealed" with an inline session `<select>` for a secret fact. A secret fact can't
// be revealed without a session, so that segment is disabled when there are none.
export function KnowledgeControl({
  value,
  onChange,
  sessions,
  bornSecret,
  disabled = false,
}: {
  value: KnowledgeInput;
  onChange: (k: KnowledgeInput) => void;
  sessions: SessionRef[];
  bornSecret: boolean;
  disabled?: boolean;
}): React.ReactElement {
  const hidden = value.kind === "hidden";
  const revealSession = value.kind === "revealed" ? value.content : null;
  // Sessions arrive ascending by ordinal, so the last is the current one - the default
  // a freshly-revealed secret fact lands on.
  const current = sessions.at(-1) ?? null;
  // A secret fact needs a session to be revealed; a public fact ("Public") does not.
  const canReveal = !bornSecret || current !== null;

  function reveal(): void {
    if (!bornSecret) {
      onChange({ kind: "public" });
      return;
    }
    const target = revealSession ?? current?.id ?? null;
    if (target !== null) onChange({ kind: "revealed", content: target });
  }

  return (
    <div className="flex flex-col gap-2">
      <SegmentedControl
        aria-label="To the players"
        isDisabled={disabled}
        selectedKeys={hidden ? ["hidden"] : ["shown"]}
        onSelectionChange={(keys) => {
          // Hidden conceals; the other segment reveals (public, or a secret fact
          // back to its reveal session). disallowEmptySelection keeps one active.
          if ([...keys][0] === "hidden") onChange({ kind: "hidden" });
          else reveal();
        }}
      >
        <SegmentedItem
          id="hidden"
          className="data-[selected]:bg-primary/15 data-[selected]:text-primary"
        >
          <EyeOff className="size-3.5" aria-hidden="true" />
          Hidden
        </SegmentedItem>
        <SegmentedItem
          id="shown"
          isDisabled={!canReveal}
          className="data-[selected]:bg-gold/15 data-[selected]:text-foreground"
        >
          <Eye className="size-3.5" aria-hidden="true" />
          {bornSecret ? "Revealed" : "Public"}
        </SegmentedItem>
      </SegmentedControl>

      {bornSecret && !hidden && revealSession !== null ? (
        <div className="flex flex-wrap items-center gap-2 ps-1 font-sans text-[12px] text-muted-foreground">
          <span>Revealed at</span>
          <SessionSelect
            ariaLabel="Reveal session"
            sessions={sessions}
            current={current}
            value={revealSession}
            disabled={disabled}
            onSelect={(id) => onChange({ kind: "revealed", content: id })}
          />
        </div>
      ) : bornSecret && !hidden && !canReveal ? (
        <span className="ps-1 font-sans text-[12px] text-muted-foreground italic">
          no sessions yet
        </span>
      ) : null}
    </div>
  );
}
