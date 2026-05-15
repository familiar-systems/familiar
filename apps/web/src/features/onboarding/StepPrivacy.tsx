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
          Step three
        </p>
        <h2 className="font-display text-3xl leading-tight font-medium tracking-tight md:text-4xl">
          Two questions about <em className="text-gold italic">your data</em>
        </h2>
        <p className="max-w-xl text-base leading-relaxed text-muted-foreground">
          Your familiar can be as quiet or as helpful as you want. We need an explicit choice on
          each of the two questions below. No defaults, no pre-ticked boxes. You can change either
          of these any time from settings.
        </p>
      </header>

      {/* ---- Question 1: Audio ---- */}
      <fieldset className="space-y-3" data-testid="audio-fieldset">
        <FieldHead label="1. Session audio" hint="Required choice" />
        <p className="text-sm leading-relaxed text-muted-foreground">
          Some tables record their sessions and let the familiar transcribe; others keep everything
          in text. Tabletop sessions also trip up off-the-shelf speech models (fantasy names, system
          jargon, overlapping voices, different languages), so we tune our own transcription on real
          audio from people who opt in.
        </p>

        <div className="grid gap-2" role="radiogroup" aria-label="Session audio">
          <RadioCardBullets
            testid="audio-opt-in"
            selected={audio === "opt-in"}
            title="Opt in. Record and help improve transcription."
            bullets={[
              "You upload audio; your familiar transcribes and drafts recaps.",
              <>
                Used only to improve <strong>speech recognition</strong> models in your languages.
              </>,
              <>
                <strong>Never</strong> used to train generative AI or LLMs.
              </>,
              <>
                <strong>Never</strong> sold, licensed, or shared outside familiar.systems.
              </>,
              "Switch off anytime; your audio leaves the training pool.",
            ]}
            onClick={() => {
              setAudio("opt-in");
            }}
          />
          <RadioCardTagline
            testid="audio-opt-out"
            selected={audio === "opt-out"}
            title="Opt out. Record, but don't train on me."
            tagline="You still get full transcription. Your audio is processed, returned as text, and excluded from any future training run."
            onClick={() => {
              setAudio("opt-out");
            }}
          />
          <RadioCardTagline
            testid="audio-text-only"
            selected={audio === "text-only"}
            title="Text only. Never record."
            tagline="You'll paste notes by hand. Audio features stay off; nothing is uploaded or transcribed at any point."
            onClick={() => {
              setAudio("text-only");
            }}
          />
        </div>
      </fieldset>

      {/* ---- Question 2: AI evals ---- */}
      <fieldset className="space-y-3" data-testid="evals-fieldset">
        <FieldHead label="2. AI evals & tooling" hint="Required choice" />
        <p className="text-sm leading-relaxed text-muted-foreground">
          Independent of audio, your familiar can send back{" "}
          <em className="italic">anonymized signal</em> about what worked and what didn't. Used to
          tune prompts and tooling, especially for less-common languages and systems where the
          defaults stumble.
        </p>

        <div className="grid gap-2" role="radiogroup" aria-label="AI evals">
          <RadioCardBullets
            testid="evals-on"
            selected={evalsEnabled === true}
            title="Evals are fine. Help us tune the AI."
            pill="Please help"
            bullets={[
              "Anonymized: no account name, no player names, no campaign title.",
              "Your writing stays yours; never reused as content.",
              <>
                Used to improve <strong>prompts and tooling</strong>, not to train models.
              </>,
              <>
                <strong>Never</strong> sold, licensed, or shared outside familiar.systems.
              </>,
              "Switch off anytime.",
            ]}
            onClick={() => {
              setEvals(true);
            }}
          />
          <RadioCardTagline
            testid="evals-off"
            selected={evalsEnabled === false}
            title="No evals. Keep it to yourself."
            tagline="Your familiar still works exactly the same. We just won't learn from what worked and what didn't in your campaign."
            onClick={() => {
              setEvals(false);
            }}
          />
        </div>
      </fieldset>

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
        <ul className="ml-4 list-disc space-y-1 text-xs leading-snug text-muted-foreground marker:text-muted-foreground/40">
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
