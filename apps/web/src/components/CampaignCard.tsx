// Hub list item. Mirrors the EmptyHubCard's lifted-card aesthetic but is
// dense enough to fit several across the grid. Surfaces the
// initialization-failed state via a bronze-bordered chip when
// `last_init_error` is set.

import type { Campaign } from "@familiar-systems/types-app";
import { Link } from "@tanstack/react-router";

interface CampaignCardProps {
  campaign: Campaign;
}

export function CampaignCard({ campaign }: CampaignCardProps): React.ReactElement {
  const failed = campaign.last_init_error !== null;
  const display = campaign.name ?? "Untitled campaign";

  return (
    <Link
      to="/c/$campaignId"
      params={{ campaignId: campaign.id }}
      data-testid={`campaign-card-${campaign.id}`}
      data-state={failed ? "failed" : campaign.wizard_completed_at !== null ? "sealed" : "draft"}
      className={[
        "group relative block space-y-3 rounded-2xl border bg-background p-6",
        "border-foreground/10 shadow-[0_8px_24px_-16px_rgb(28_25_23/0.25)]",
        "transition-all duration-200",
        "hover:-translate-y-0.5 hover:border-primary/30 hover:shadow-lg hover:shadow-primary/10",
      ].join(" ")}
    >
      <div className="flex items-baseline justify-between gap-3">
        <h3 className="font-display text-xl font-medium tracking-tight">{display}</h3>
        {failed ? (
          <span
            data-testid="failed-init-badge"
            className="inline-flex items-center rounded-full border border-foreground/15 bg-bronze-muted/60 px-2.5 py-0.5 text-[10px] tracking-wider text-foreground/80 uppercase"
          >
            Init failed
          </span>
        ) : campaign.wizard_completed_at === null ? (
          <span className="inline-flex items-center rounded-full border border-foreground/10 bg-background/60 px-2.5 py-0.5 text-[10px] tracking-wider text-muted-foreground uppercase">
            Draft
          </span>
        ) : null}
      </div>
      {campaign.tagline !== null && campaign.tagline !== "" ? (
        <p className="text-sm leading-snug text-muted-foreground">{campaign.tagline}</p>
      ) : null}
      <p className="text-xs tracking-wide text-muted-foreground/70 uppercase">
        {campaign.game_system ?? "System not yet chosen"}
      </p>
    </Link>
  );
}
