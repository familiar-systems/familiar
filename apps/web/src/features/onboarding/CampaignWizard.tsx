// Orchestrating component for the new-campaign wizard.
//
// State is entirely client-side until the user seals the campaign.
// On seal we fire one `PATCH /campaign/{id}` with `wizard_complete: true`.

import type {
  AudioMode,
  CatalogResponse,
  PatchCampaignRequest,
  SystemEntry,
} from "@familiar-systems/types-campaign";
import { useEffect, useMemo, useState } from "react";
import { m } from "../../paraglide/messages.js";
import { campaignClient } from "../../lib/campaigns-api";
import { StepName } from "./StepName";
import { StepPrivacy } from "./StepPrivacy";
import { StepRail } from "./StepRail";
import { StepReview } from "./StepReview";
import { BYO_COLOR, BYO_DEFAULT_NAME, StepSystem, type SystemPick } from "./StepSystem";
import type { SealState } from "./WaxSeal";

interface CampaignWizardProps {
  campaignId: string;
  /** Locale used both for catalog fetch and as the initial content_locale. */
  locale: string;
  onDone: () => void;
}

export function CampaignWizard({
  campaignId,
  locale,
  onDone,
}: CampaignWizardProps): React.ReactElement {
  // Step labels resolve per render so a locale change re-localizes the rail;
  // ids are DOM keys / data-step values, not user-facing.
  const STEPS = [
    { id: "name", label: m.wizardStepNameLabel() },
    { id: "system", label: m.wizardStepSystemLabel() },
    { id: "privacy", label: m.wizardStepPrivacyLabel() },
    { id: "review", label: m.wizardStepReviewLabel() },
  ];

  const [step, setStep] = useState(0);
  const [name, setName] = useState("");
  const [tagline, setTagline] = useState("");
  const [catalog, setCatalog] = useState<CatalogResponse | null>(null);
  const [pick, setPick] = useState<SystemPick>({ kind: "none" });
  const [templateSlugs, setTemplateSlugs] = useState<Set<string>>(new Set());
  const [audio, setAudio] = useState<AudioMode | null>(null);
  const [evalsEnabled, setEvalsEnabled] = useState<boolean | null>(null);
  const [sealState, setSealState] = useState<SealState>("idle");
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  // Catalog fetch runs once on mount. The wizard cannot leave step 1 with a
  // null catalog (step 2's UI handles the loading case) so we don't gate
  // the whole wizard on it; users who type a name fast will see "fetching
  // the catalog" briefly when they reach step 2.
  useEffect(() => {
    let cancelled = false;
    campaignClient
      .GET("/catalog/systems", { params: { query: { locale } } })
      .then(({ data, error }) => {
        if (cancelled) return;
        if (error || !data) {
          setErrorMessage(m.wizardCatalogUnavailable());
          return;
        }
        setCatalog(data);
      });
    return () => {
      cancelled = true;
    };
  }, [locale]);

  const canAdvance = (() => {
    switch (step) {
      case 0:
        return name.trim().length > 0;
      case 1:
        return pick.kind !== "none" && templateSlugs.size > 0;
      case 2:
        return audio !== null && evalsEnabled !== null;
      default:
        return false;
    }
  })();

  const onCatalogPick = (next: SystemEntry): void => {
    setPick({ kind: "catalog", entry: next });
    // Default to the full bundle on first pick. If the user comes back and
    // picks a different system, replace selections wholesale rather than
    // keeping orphaned slugs from the previous bundle.
    setTemplateSlugs(new Set(next.bundle.map((t) => t.slug)));
  };

  const onByoPick = (): void => {
    if (catalog === null) return;
    setPick((prev) => ({ kind: "byo", name: prev.kind === "byo" ? prev.name : "" }));
    setTemplateSlugs(new Set(catalog.byo.bundle.map((t) => t.slug)));
  };

  const onByoNameChange = (next: string): void => {
    setPick((prev) => (prev.kind === "byo" ? { kind: "byo", name: next } : prev));
  };

  const systemDisplay = useMemo<{ name: string; color: string } | null>(() => {
    switch (pick.kind) {
      case "none":
        return null;
      case "catalog":
        return { name: pick.entry.name, color: pick.entry.color };
      case "byo": {
        const trimmed = pick.name.trim();
        return {
          name: trimmed === "" ? BYO_DEFAULT_NAME : trimmed,
          color: BYO_COLOR,
        };
      }
    }
  }, [pick]);

  const onInitializeCampaign = async (): Promise<void> => {
    if (systemDisplay === null || audio === null || evalsEnabled === null) {
      // Should be unreachable since canAdvance gates the initialization step,
      // but guard so we never POST a partial payload.
      setErrorMessage(m.wizardPayloadIncomplete());
      return;
    }
    setSealState("sealing");
    setErrorMessage(null);
    const body: PatchCampaignRequest = {
      game_system: systemDisplay.name,
      content_locale: locale,
      name: name.trim(),
      tagline: tagline.trim() === "" ? null : tagline.trim(),
      template_slugs: Array.from(templateSlugs),
      audio,
      evals_enabled: evalsEnabled,
      wizard_complete: true,
    };
    const { error } = await campaignClient.PATCH("/campaign/{id}", {
      params: { path: { id: campaignId } },
      body,
    });
    if (error) {
      setErrorMessage(error.error ?? m.wizardGenericError());
      setSealState("cracked");
      return;
    }
    onDone();
  };

  return (
    <div
      className="mx-auto w-full max-w-3xl space-y-8 rounded-2xl border border-foreground/10 bg-background/85 p-6 shadow-[0_24px_64px_-24px_rgb(28_25_23/0.35)] backdrop-blur enter-from-below md:max-w-4xl md:space-y-10 md:p-10"
      data-testid="campaign-wizard"
    >
      <StepRail current={step} steps={STEPS} />

      <div className="space-y-10">
        {step === 0 ? (
          <StepName
            name={name}
            tagline={tagline}
            onChange={(next) => {
              setName(next.name);
              setTagline(next.tagline);
            }}
          />
        ) : null}
        {step === 1 ? (
          <StepSystem
            catalog={catalog}
            pick={pick}
            selectedTemplateSlugs={templateSlugs}
            onCatalogPick={onCatalogPick}
            onByoPick={onByoPick}
            onByoNameChange={onByoNameChange}
            onTemplatesChange={setTemplateSlugs}
          />
        ) : null}
        {step === 2 ? (
          <StepPrivacy
            audio={audio}
            evalsEnabled={evalsEnabled}
            onChange={(next) => {
              setAudio(next.audio);
              setEvalsEnabled(next.evalsEnabled);
            }}
          />
        ) : null}
        {step === 3 ? (
          <StepReview
            name={name}
            tagline={tagline}
            systemDisplay={systemDisplay}
            templateSlugs={templateSlugs}
            audio={audio}
            evalsEnabled={evalsEnabled}
            sealState={sealState}
            errorMessage={errorMessage}
            locale={locale}
            onInitializeCampaign={() => {
              void onInitializeCampaign();
            }}
            onBack={() => {
              setSealState("idle");
              setErrorMessage(null);
              setStep(2);
            }}
          />
        ) : null}

        {step < 3 ? <Footer step={step} canAdvance={canAdvance} setStep={setStep} /> : null}
      </div>
    </div>
  );
}

interface FooterProps {
  step: number;
  canAdvance: boolean;
  setStep: (n: number) => void;
}

function Footer({ step, canAdvance, setStep }: FooterProps): React.ReactElement {
  return (
    <footer className="flex items-center justify-between border-t border-foreground/5 pt-6">
      <button
        type="button"
        data-testid="wizard-back"
        onClick={() => {
          setStep(Math.max(0, step - 1));
        }}
        disabled={step === 0}
        className="rounded-full px-4 py-2 text-sm text-muted-foreground transition-colors hover:bg-foreground/5 disabled:cursor-not-allowed disabled:opacity-40"
      >
        {m.wizardBack()}
      </button>
      <button
        type="button"
        data-testid="wizard-next"
        onClick={() => {
          setStep(step + 1);
        }}
        disabled={!canAdvance}
        className="inline-flex items-center gap-2 rounded-full bg-gold px-6 py-2.5 text-sm font-medium text-white shadow-md shadow-gold/25 transition-colors hover:bg-gold/90 disabled:cursor-not-allowed disabled:opacity-50 disabled:shadow-none"
      >
        {m.wizardContinue()}
      </button>
    </footer>
  );
}
