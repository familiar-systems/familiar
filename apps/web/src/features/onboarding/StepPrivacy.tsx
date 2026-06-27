// Step 3: privacy. Two required radio groups.
//
// **Copy is ported verbatim from `tmp/NewCampaignOnboarding/onboarding.jsx`
// `StepAudio`** (line 766 onwards). Em-dashes have been replaced with
// commas, periods, or colons per the project style guide; otherwise the
// text is the wireframe's. The opt-in / evals-on bullet lists are
// load-bearing (they carve out specific scope: speech-rec only, never
// generative, never sold). Do not collapse them.
//
// `audio` is a sum type, not a bool pair, because the three states aren't
// independent: "text-only" excludes the very notion of training; "opt-out"
// records but won't train; "opt-in" both records and trains.

import type { AudioMode } from "@familiar-systems/types-campaign";
import { m } from "../../paraglide/messages.js";

interface StepPrivacyProps {
  audio: AudioMode | null;
  evalsEnabled: boolean | null;
  onChange: (next: { audio: AudioMode | null; evalsEnabled: boolean | null }) => void;
}

export function StepPrivacy({
  audio,
  evalsEnabled,
  onChange,
}: StepPrivacyProps): React.ReactElement {
  const setAudio = (v: AudioMode): void => {
    onChange({ audio: v, evalsEnabled });
  };
  const setEvals = (v: boolean): void => {
    onChange({ audio, evalsEnabled: v });
  };

  return (
    <div className="space-y-8 enter-from-below">
      <header className="space-y-3">
        <p className="text-xs font-medium tracking-[0.28em] text-muted-foreground uppercase">
          {m.stepPrivacyEyebrow()}
        </p>
        {/* Headline stays inline English: the gold-emphasized "your data" is
            inline markup Paraglide's plain-string messages can't carry yet;
            localized with a rich-text interpolation helper later. */}
        <h2 className="font-display text-3xl leading-tight font-medium tracking-tight md:text-4xl">
          Two questions about <em className="text-gold italic">your data</em>
        </h2>
        <p className="max-w-xl text-base leading-relaxed text-muted-foreground">
          {m.stepPrivacyLede()}
        </p>
      </header>

      {/* ---- Question 1: Audio ---- */}
      <fieldset className="space-y-3" data-testid="audio-fieldset">
        <FieldHead label={m.stepPrivacyAudioLabel()} hint={m.stepPrivacyRequiredChoice()} />
        <p className="text-sm leading-relaxed text-muted-foreground">{m.stepPrivacyAudioLede()}</p>

        <div className="grid gap-2" role="radiogroup" aria-label={m.stepPrivacyAudioAriaLabel()}>
          <RadioCardBullets
            testid="audio-opt-in"
            selected={audio === "opt-in"}
            title={m.stepPrivacyAudioOptInTitle()}
            // Bullets 2-4 stay inline English: each carries a mid-sentence
            // <strong> Paraglide's plain-string messages can't carry yet;
            // localized with a rich-text interpolation helper later.
            bullets={[
              m.stepPrivacyAudioOptInBullet1(),
              <>
                Used only to improve <strong>speech recognition</strong> models in your languages.
              </>,
              <>
                <strong>Never</strong> used to train generative AI or LLMs.
              </>,
              <>
                <strong>Never</strong> sold, licensed, or shared outside familiar.systems.
              </>,
              m.stepPrivacyAudioOptInBullet5(),
            ]}
            onClick={() => {
              setAudio("opt-in");
            }}
          />
          <RadioCardTagline
            testid="audio-opt-out"
            selected={audio === "opt-out"}
            title={m.stepPrivacyAudioOptOutTitle()}
            tagline={m.stepPrivacyAudioOptOutTagline()}
            onClick={() => {
              setAudio("opt-out");
            }}
          />
          <RadioCardTagline
            testid="audio-text-only"
            selected={audio === "text-only"}
            title={m.stepPrivacyAudioTextOnlyTitle()}
            tagline={m.stepPrivacyAudioTextOnlyTagline()}
            onClick={() => {
              setAudio("text-only");
            }}
          />
        </div>
      </fieldset>

      {/* ---- Question 2: AI evals ---- */}
      <fieldset className="space-y-3" data-testid="evals-fieldset">
        <FieldHead label={m.stepPrivacyEvalsLabel()} hint={m.stepPrivacyRequiredChoice()} />
        {/* Lede stays inline English: the italic "anonymized signal" is a
            mid-sentence span Paraglide's plain-string messages can't carry
            yet; localized with a rich-text interpolation helper later. */}
        <p className="text-sm leading-relaxed text-muted-foreground">
          Independent of audio, your familiar can send back{" "}
          <em className="italic">anonymized signal</em> about what worked and what didn't. Used to
          tune prompts and tooling, especially for less-common languages and systems where the
          defaults stumble.
        </p>

        <div className="grid gap-2" role="radiogroup" aria-label={m.stepPrivacyEvalsAriaLabel()}>
          <RadioCardBullets
            testid="evals-on"
            selected={evalsEnabled === true}
            title={m.stepPrivacyEvalsOnTitle()}
            pill={m.stepPrivacyEvalsOnPill()}
            // Bullets 3-4 stay inline English: each carries a mid-sentence
            // <strong> Paraglide's plain-string messages can't carry yet;
            // localized with a rich-text interpolation helper later.
            bullets={[
              m.stepPrivacyEvalsOnBullet1(),
              m.stepPrivacyEvalsOnBullet2(),
              <>
                Used to improve <strong>prompts and tooling</strong>, not to train models.
              </>,
              <>
                <strong>Never</strong> sold, licensed, or shared outside familiar.systems.
              </>,
              m.stepPrivacyEvalsOnBullet5(),
            ]}
            onClick={() => {
              setEvals(true);
            }}
          />
          <RadioCardTagline
            testid="evals-off"
            selected={evalsEnabled === false}
            title={m.stepPrivacyEvalsOffTitle()}
            tagline={m.stepPrivacyEvalsOffTagline()}
            onClick={() => {
              setEvals(false);
            }}
          />
        </div>
      </fieldset>

      {/* Footnote stays inline English: the two <strong>never</strong> spans
          are mid-sentence markup Paraglide's plain-string messages can't carry
          yet; localized with a rich-text interpolation helper later. */}
      <p className="text-xs leading-relaxed text-muted-foreground/80">
        Either way, your data is <strong>never</strong> used to train generative models, and is
        <strong> never</strong> sold or shared.
      </p>
    </div>
  );
}

function FieldHead({ label, hint }: { label: string; hint: string }): React.ReactElement {
  return (
    <div className="flex items-baseline justify-between gap-4">
      <legend className="font-display text-base font-medium tracking-tight">{label}</legend>
      <span className="text-xs tracking-wider text-muted-foreground uppercase">{hint}</span>
    </div>
  );
}

interface BaseRadioProps {
  testid: string;
  selected: boolean;
  title: string;
  onClick: () => void;
}

interface RadioCardBulletsProps extends BaseRadioProps {
  bullets: React.ReactNode[];
  pill?: string;
}

function RadioCardBullets({
  testid,
  selected,
  title,
  bullets,
  pill,
  onClick,
}: RadioCardBulletsProps): React.ReactElement {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      data-testid={testid}
      data-selected={selected}
      onClick={onClick}
      className={[
        "group flex items-start gap-3 rounded-2xl border p-4 text-left transition-all duration-200",
        selected
          ? "border-gold/60 bg-bronze-muted/30 shadow-md shadow-gold/10"
          : "border-foreground/10 bg-background/40 hover:border-primary/30 hover:bg-foreground/[0.02]",
      ].join(" ")}
    >
      <RadioPip selected={selected} />
      <span className="flex-1 space-y-2">
        <span className="flex flex-wrap items-baseline gap-2">
          <span className="text-sm font-medium text-foreground">{title}</span>
          {pill ? (
            <span className="inline-flex items-center rounded-full border border-gold/30 bg-gold/10 px-2 py-0.5 text-[10px] tracking-[0.18em] text-gold uppercase">
              {pill}
            </span>
          ) : null}
        </span>
        <ul className="ms-4 list-disc space-y-1 text-xs leading-snug text-muted-foreground marker:text-muted-foreground/40">
          {bullets.map((bullet, i) => (
            // Bullets are static at compile time, so the array index is stable.
            // eslint-disable-next-line react/no-array-index-key
            <li key={i}>{bullet}</li>
          ))}
        </ul>
      </span>
    </button>
  );
}

interface RadioCardTaglineProps extends BaseRadioProps {
  tagline: string;
}

function RadioCardTagline({
  testid,
  selected,
  title,
  tagline,
  onClick,
}: RadioCardTaglineProps): React.ReactElement {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      data-testid={testid}
      data-selected={selected}
      onClick={onClick}
      className={[
        "group flex items-start gap-3 rounded-2xl border p-4 text-left transition-all duration-200",
        selected
          ? "border-gold/60 bg-bronze-muted/30 shadow-md shadow-gold/10"
          : "border-foreground/10 bg-background/40 hover:border-primary/30 hover:bg-foreground/[0.02]",
      ].join(" ")}
    >
      <RadioPip selected={selected} />
      <span className="flex-1 space-y-1">
        <span className="block text-sm font-medium text-foreground">{title}</span>
        <span className="block text-xs leading-snug text-muted-foreground">{tagline}</span>
      </span>
    </button>
  );
}

function RadioPip({ selected }: { selected: boolean }): React.ReactElement {
  return (
    <span
      aria-hidden="true"
      className={[
        "mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full border transition-colors",
        selected ? "border-gold bg-gold" : "border-foreground/20 bg-background",
      ].join(" ")}
    >
      {selected ? <span className="size-2 rounded-full bg-white" /> : null}
    </span>
  );
}
