// Step 4: review and initialize. Final summary plus the wax seal.

import type { AudioMode } from "@familiar-systems/types-campaign";
import { Trans } from "../../components/Trans";
import { m } from "../../paraglide/messages.js";
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
          {m.stepReviewEyebrow()}
        </p>
        <h2 className="font-display text-3xl leading-tight font-medium tracking-tight md:text-4xl">
          <Trans
            message={m.stepReviewHeading()}
            components={{ gold: (c) => <em className="text-gold italic">{c}</em> }}
          />
        </h2>
        <p className="max-w-xl text-base leading-relaxed text-muted-foreground">
          {m.stepReviewLede()}
        </p>
      </header>

      <dl
        className="grid gap-4 rounded-2xl border border-foreground/10 bg-bronze-muted/20 p-6 md:grid-cols-2"
        data-testid="review-summary"
      >
        <ReviewRow label={m.stepReviewRowName()}>
          <span className="font-display text-lg font-medium tracking-tight">
            {name || <Empty />}
          </span>
        </ReviewRow>
        <ReviewRow label={m.stepReviewRowTagline()}>
          {tagline ? <span className="text-base">{tagline}</span> : <Empty />}
        </ReviewRow>
        <ReviewRow label={m.stepReviewRowSystem()}>
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
        <ReviewRow label={m.stepReviewRowTemplates()}>
          <span className="text-base">
            {templateSlugs.size === 0 ? (
              <Empty />
            ) : (
              m.stepReviewTemplatesSelected({ count: templateSlugs.size })
            )}
          </span>
        </ReviewRow>
        <ReviewRow label={m.stepReviewRowAudio()}>
          <span className="text-base">{describeAudio(audio)}</span>
        </ReviewRow>
        <ReviewRow label={m.stepReviewRowEvals()}>
          <span className="text-base">
            {evalsEnabled === null
              ? m.stepReviewNotSet()
              : evalsEnabled
                ? m.stepReviewEvalsShared()
                : m.stepReviewEvalsNotShared()}
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
            {m.stepReviewErrorTitle()}
          </p>
          <p className="text-sm leading-relaxed text-muted-foreground">{errorMessage}</p>
          <p className="text-xs leading-relaxed text-muted-foreground/80">
            {m.stepReviewErrorDetail()}
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
            {m.stepReviewBack()}
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
  return <span className="text-muted-foreground italic">{m.stepReviewNotSet()}</span>;
}

function describeAudio(mode: AudioMode | null): string {
  switch (mode) {
    case null:
      return m.stepReviewNotSet();
    case "opt-in":
      return m.stepReviewAudioOptIn();
    case "opt-out":
      return m.stepReviewAudioOptOut();
    case "text-only":
      return m.stepReviewAudioTextOnly();
  }
}
