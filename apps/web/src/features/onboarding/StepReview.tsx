// Step 4: review and initialize. Final summary plus the wax seal.

import type { AudioMode } from "@familiar-systems/types-campaign";
import { type SealState, WaxSeal } from "./WaxSeal";

interface StepReviewProps {
  name: string;
  tagline: string;
  /**
   * Resolved system display: the locale-resolved catalog name, the trimmed
   * BYO custom name, or the BYO `default_name` fallback. `null` when the
   * user hasn't picked yet (should be unreachable here since canAdvance
   * gates step 1, but typed for safety).
   */
  systemDisplay: { name: string; color: string } | null;
  templateSlugs: ReadonlySet<string>;
  audio: AudioMode | null;
  evalsEnabled: boolean | null;
  sealState: SealState;
  errorMessage: string | null;
  /** BCP-47; routed to WaxSeal so script-aware monogram derivation honors locale. */
  locale: string;
  onInitializeCampaign: () => void;
  onBack: () => void;
}

export function StepReview({
  name,
  tagline,
  systemDisplay,
  templateSlugs,
  audio,
  evalsEnabled,
  sealState,
  errorMessage,
  locale,
  onInitializeCampaign,
  onBack,
}: StepReviewProps): React.ReactElement {
  const cracked = sealState === "cracked";

  return (
    <div className="space-y-8 enter-from-below">
      <header className="space-y-3">
        <p className="text-xs font-medium tracking-[0.28em] text-muted-foreground uppercase">
          Step four
        </p>
        <h2 className="font-display text-3xl leading-tight font-medium tracking-tight md:text-4xl">
          Review, then <em className="text-gold italic">seal</em>.
        </h2>
        <p className="max-w-xl text-base leading-relaxed text-muted-foreground">
          Pressing the seal commits the campaign. You can rename, swap templates, and edit settings
          inside the campaign once it exists.
        </p>
      </header>

      <dl
        className="grid gap-4 rounded-2xl border border-foreground/10 bg-bronze-muted/20 p-6 md:grid-cols-2"
        data-testid="review-summary"
      >
        <ReviewRow label="Name">
          <span className="font-display text-lg font-medium tracking-tight">
            {name || <Empty />}
          </span>
        </ReviewRow>
        <ReviewRow label="Tagline">
          {tagline ? <span className="text-base">{tagline}</span> : <Empty />}
        </ReviewRow>
        <ReviewRow label="System">
          {systemDisplay ? (
            <span className="inline-flex items-center gap-2">
              <span
                aria-hidden="true"
                className="size-3 rounded-full"
                style={{ background: systemDisplay.color }}
              />
              <span>{systemDisplay.name}</span>
            </span>
          ) : (
            <Empty />
          )}
        </ReviewRow>
        <ReviewRow label="Templates">
          <span className="text-base">
            {templateSlugs.size === 0 ? <Empty /> : `${templateSlugs.size} selected`}
          </span>
        </ReviewRow>
        <ReviewRow label="Audio">
          <span className="text-base">{describeAudio(audio)}</span>
        </ReviewRow>
        <ReviewRow label="AI evals">
          <span className="text-base">
            {evalsEnabled === null
              ? "(not set)"
              : evalsEnabled
                ? "Anonymized signal shared."
                : "No signal sent."}
          </span>
        </ReviewRow>
      </dl>

      {errorMessage ? (
        <div
          role="alert"
          data-testid="seal-error"
          className="space-y-2 rounded-2xl border border-foreground/10 bg-bronze-muted/40 p-5"
        >
          <p className="font-display text-base font-medium tracking-tight text-foreground">
            The seal cracked.
          </p>
          <p className="text-sm leading-relaxed text-muted-foreground">{errorMessage}</p>
          <p className="text-xs leading-relaxed text-muted-foreground/80">
            This is a known thin-slice failure: the platform minted your campaign id, but the
            initialization pipeline is not wired up yet. Your campaign exists in a draft state.
            Refresh the page and you can try again, or return to the hub.
          </p>
        </div>
      ) : null}

      <div className="flex flex-col items-center gap-6 pt-4">
        <WaxSeal
          state={sealState}
          campaignName={name}
          locale={locale}
          onClick={onInitializeCampaign}
          disabled={cracked}
        />
        <div className="flex items-center gap-2">
          <button
            type="button"
            data-testid="seal-back"
            onClick={onBack}
            className="rounded-full border border-foreground/10 bg-background/60 px-4 py-2 text-sm text-foreground transition-colors hover:bg-foreground/5"
          >
            Back
          </button>
        </div>
      </div>
    </div>
  );
}

interface ReviewRowProps {
  label: string;
  children: React.ReactNode;
}

function ReviewRow({ label, children }: ReviewRowProps): React.ReactElement {
  return (
    <div className="space-y-1">
      <dt className="text-xs tracking-[0.18em] text-muted-foreground uppercase">{label}</dt>
      <dd className="text-foreground">{children}</dd>
    </div>
  );
}

function Empty(): React.ReactElement {
  return <span className="text-muted-foreground italic">(not set)</span>;
}

function describeAudio(mode: AudioMode | null): string {
  switch (mode) {
    case null:
      return "(not set)";
    case "opt-in":
      return "Recording. Audio improves transcription.";
    case "opt-out":
      return "Recording. Audio excluded from training.";
    case "text-only":
      return "Text only. No recordings.";
  }
}
