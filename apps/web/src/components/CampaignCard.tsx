import type { Campaign, CampaignId } from "@familiar-systems/types-app";
import { Link } from "@tanstack/react-router";
import { Clock, Pencil, XCircle } from "lucide-react";
import { relativeTime } from "../lib/relative-time";

type CardState = "draft" | "init-failed" | "initialized";

function deriveState(campaign: Campaign): CardState {
  if (campaign.last_init_error !== null) return "init-failed";
  if (campaign.wizard_completed_at === null) return "draft";
  return "initialized";
}

interface CampaignCardProps {
  campaign: Campaign;
  loaded?: boolean;
}

export function CampaignCard({ campaign, loaded = false }: CampaignCardProps): React.ReactElement {
  const state = deriveState(campaign);
  if (state === "initialized") {
    return <InitializedCard campaign={campaign} loaded={loaded} />;
  }
  return <GraphPaperCard state={state} campaignId={campaign.id} />;
}

// ---------------------------------------------------------------------------
// Graph Paper Card (draft / init-failed)
// ---------------------------------------------------------------------------

const GRAPH_PAPER_LIGHT =
  "repeating-linear-gradient(0deg, transparent, transparent 19px, rgb(90 74 106 / 0.07) 19px, rgb(90 74 106 / 0.07) 20px), repeating-linear-gradient(90deg, transparent, transparent 19px, rgb(90 74 106 / 0.07) 19px, rgb(90 74 106 / 0.07) 20px)";
const GRAPH_PAPER_DARK =
  "repeating-linear-gradient(0deg, transparent, transparent 19px, rgb(154 134 170 / 0.06) 19px, rgb(154 134 170 / 0.06) 20px), repeating-linear-gradient(90deg, transparent, transparent 19px, rgb(154 134 170 / 0.06) 19px, rgb(154 134 170 / 0.06) 20px)";

interface GraphPaperCardProps {
  state: "draft" | "init-failed";
  campaignId: CampaignId;
}

function GraphPaperCard({ state, campaignId }: GraphPaperCardProps): React.ReactElement {
  const isDraft = state === "draft";
  return (
    <Link
      to="/c/$campaignId"
      params={{ campaignId }}
      data-testid={`campaign-card-${campaignId}`}
      data-state={state}
      className={[
        "group relative flex min-h-[200px] flex-col items-center justify-center overflow-hidden rounded-2xl border border-dashed bg-background/80 p-8 text-center",
        "border-primary/20 dark:border-primary/15",
        "transition-all duration-300",
        "hover:-translate-y-0.5 hover:border-primary/35 hover:shadow-[0_25px_50px_-12px_rgb(90_74_106/0.10)]",
      ].join(" ")}
    >
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 opacity-100 dark:opacity-0"
        style={{ background: GRAPH_PAPER_LIGHT }}
      />
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 opacity-0 dark:opacity-100"
        style={{ background: GRAPH_PAPER_DARK }}
      />

      <div className="relative z-10">
        <div
          className={[
            "mx-auto mb-3 flex size-10 items-center justify-center rounded-xl",
            isDraft
              ? "bg-primary/8 text-primary"
              : "bg-amber-500/12 text-[#92400e] dark:text-amber-400",
          ].join(" ")}
        >
          {isDraft ? <Pencil className="size-5" /> : <XCircle className="size-5" />}
        </div>
        <p
          className={[
            "mb-1.5 text-[11px] font-semibold tracking-[0.2em] uppercase",
            isDraft ? "text-primary" : "text-[#92400e] dark:text-amber-400",
          ].join(" ")}
        >
          {isDraft ? "Draft" : "Init failed"}
        </p>
        <p className="max-w-65 font-display text-[15px] text-muted-foreground italic">
          {isDraft
            ? "An empty sheet lays at the desk. Your mind swims with possibilities."
            : "Something went wrong. Click to retry."}
        </p>
      </div>
    </Link>
  );
}

// ---------------------------------------------------------------------------
// Initialized Banner Card (loaded / ready-to-load)
// ---------------------------------------------------------------------------

const CROSSHATCH_LIGHT =
  "repeating-linear-gradient(45deg, transparent, transparent 4px, rgb(0 0 0 / 0.04) 4px, rgb(0 0 0 / 0.04) 5px), repeating-linear-gradient(-45deg, transparent, transparent 4px, rgb(0 0 0 / 0.04) 4px, rgb(0 0 0 / 0.04) 5px)";
const CROSSHATCH_DARK =
  "repeating-linear-gradient(45deg, transparent, transparent 4px, rgb(255 255 255 / 0.04) 4px, rgb(255 255 255 / 0.04) 5px), repeating-linear-gradient(-45deg, transparent, transparent 4px, rgb(255 255 255 / 0.04) 4px, rgb(255 255 255 / 0.04) 5px)";

const GOLD_BANNER =
  "radial-gradient(ellipse at 50% 120%, color-mix(in srgb, var(--color-gold), transparent 55%), transparent 70%), linear-gradient(160deg, color-mix(in srgb, var(--color-gold), transparent 78%), color-mix(in srgb, var(--color-bronze), transparent 78%))";
const PLUM_BANNER =
  "radial-gradient(ellipse at 50% 120%, color-mix(in srgb, var(--color-primary), transparent 80%), transparent 70%), linear-gradient(160deg, color-mix(in srgb, var(--color-primary), transparent 88%), color-mix(in srgb, var(--color-bronze-muted), transparent 70%))";
const GOLD_GLOW =
  "radial-gradient(circle at 80% 0%, color-mix(in srgb, var(--color-gold), transparent 70%), transparent 55%)";

interface InitializedCardProps {
  campaign: Campaign;
  loaded: boolean;
}

function InitializedCard({ campaign, loaded }: InitializedCardProps): React.ReactElement {
  const display = campaign.name ?? "Untitled campaign";
  const hasTagline = campaign.tagline !== null && campaign.tagline !== "";

  return (
    <Link
      to="/c/$campaignId"
      params={{ campaignId: campaign.id }}
      data-testid={`campaign-card-${campaign.id}`}
      data-state={loaded ? "loaded" : "ready"}
      className={[
        "group block overflow-hidden rounded-2xl bg-background",
        "transition-all duration-300",
        "hover:-translate-y-0.5",
        loaded
          ? [
              "border border-[rgb(184_149_48/0.3)] dark:border-[rgb(212_169_68/0.35)]",
              "shadow-[0_0_0_1px_rgb(184_149_48/0.10)_inset,0_10px_15px_-3px_rgb(184_149_48/0.10)]",
              "dark:shadow-[0_0_0_1px_rgb(212_169_68/0.15)_inset,0_12px_50px_-18px_rgb(212_169_68/0.25)]",
              "hover:border-[rgb(184_149_48/0.5)] dark:hover:border-[rgb(212_169_68/0.5)]",
              "hover:shadow-[0_0_0_1px_rgb(184_149_48/0.15)_inset,0_12px_24px_-6px_rgb(184_149_48/0.18)]",
            ].join(" ")
          : [
              "border border-[rgb(90_74_106/0.15)] dark:border-[rgb(154_134_170/0.15)]",
              "shadow-[0_8px_32px_-16px_rgb(28_25_23/0.25)]",
              "dark:shadow-[0_12px_40px_-18px_rgb(0_0_0/0.55)]",
              "hover:border-[rgb(90_74_106/0.35)] dark:hover:border-[rgb(154_134_170/0.3)]",
              "hover:shadow-[0_25px_50px_-12px_rgb(90_74_106/0.10)] dark:hover:shadow-[0_25px_50px_-12px_rgb(0_0_0/0.3)]",
            ].join(" "),
      ].join(" ")}
    >
      {/* Banner */}
      <div
        className="relative h-20 overflow-hidden"
        style={{ background: loaded ? GOLD_BANNER : PLUM_BANNER }}
      >
        <div
          aria-hidden="true"
          className={[
            "absolute inset-0",
            loaded ? "opacity-80 dark:opacity-0" : "opacity-50 dark:opacity-0",
          ].join(" ")}
          style={{ background: CROSSHATCH_LIGHT }}
        />
        <div
          aria-hidden="true"
          className={[
            "absolute inset-0",
            loaded ? "opacity-0 dark:opacity-80" : "opacity-0 dark:opacity-50",
          ].join(" ")}
          style={{ background: CROSSHATCH_DARK }}
        />
        {loaded ? (
          <div
            aria-hidden="true"
            className="absolute inset-0"
            style={{
              background: GOLD_GLOW,
              mixBlendMode: "screen",
            }}
          />
        ) : null}
      </div>

      {/* Body */}
      <div className="px-5.5 py-4.5">
        <div className="mb-2 flex items-center gap-1.5">
          <span
            className={[
              "block size-[7px] flex-none rounded-full",
              loaded ? "bg-gold ember-dot" : "bg-line",
            ].join(" ")}
          />
          <span
            className={[
              "text-[11px] font-medium tracking-[0.04em]",
              loaded ? "text-gold" : "text-muted-foreground",
            ].join(" ")}
          >
            {loaded ? "Loaded" : "Ready to Load"}
          </span>
        </div>

        <h3 className="mb-1 font-display text-2xl leading-[1.15] font-medium tracking-tight">
          {display}
        </h3>

        {hasTagline ? (
          <p
            className="mb-3.5 font-display text-sm leading-[1.45] text-foreground/75 italic"
            style={{ textWrap: "pretty" }}
          >
            {campaign.tagline}
          </p>
        ) : null}

        <div className="flex items-center justify-between border-t border-black/6 pt-3 text-xs text-muted-foreground dark:border-white/8">
          <span className="inline-flex items-center gap-1.5">
            <Clock className="size-3.25" />
            {relativeTime(campaign.updated_at)}
          </span>
          <span className="font-display text-primary italic">
            {campaign.game_system ?? "System not yet chosen"}
          </span>
        </div>
      </div>
    </Link>
  );
}
