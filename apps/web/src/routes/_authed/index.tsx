import type { Campaign } from "@familiar-systems/types-app";
import { createFileRoute } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { CampaignCard } from "../../components/CampaignCard";
import { EmptyHubCard } from "../../components/EmptyHubCard";
import { Trans } from "../../components/Trans";
import { useCreateCampaign } from "../../features/onboarding/useCreateCampaign";
import { client } from "../../lib/api";
import { m } from "../../paraglide/messages.js";

interface ListState {
  campaigns: Campaign[] | null;
  error: string | null;
}

function Hub(): React.ReactElement {
  const [list, setList] = useState<ListState>({ campaigns: null, error: null });

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const { data, response } = await client.GET("/campaigns");
      if (cancelled) return;
      if (!response.ok || !data) {
        setList({ campaigns: null, error: m.hubListFailed({ status: response.status }) });
        return;
      }
      setList({ campaigns: data as Campaign[], error: null });
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  if (list.error !== null) {
    return (
      <section className="mx-auto w-full max-w-3xl px-8 pt-24">
        <p className="text-sm text-muted-foreground">{list.error}</p>
      </section>
    );
  }
  if (list.campaigns === null) {
    return (
      <section className="mx-auto w-full max-w-3xl px-8 pt-24">
        <p className="text-sm text-muted-foreground">{m.appLoading()}</p>
      </section>
    );
  }
  if (list.campaigns.length === 0) {
    return (
      <section className="flex flex-1 items-center justify-center px-8 py-16">
        <div className="w-full max-w-3xl">
          <EmptyHubCard />
        </div>
      </section>
    );
  }

  return <PopulatedHub campaigns={list.campaigns} />;
}

interface PopulatedHubProps {
  campaigns: Campaign[];
}

function PopulatedHub({ campaigns }: PopulatedHubProps): React.ReactElement {
  const { state, create } = useCreateCampaign();
  return (
    <section className="mx-auto w-full max-w-5xl px-8 pt-24 pb-32">
      <header className="mb-16 flex flex-col items-center gap-6 text-center">
        <span className="block text-xs font-medium tracking-[0.28em] text-muted-foreground uppercase enter-from-below">
          {m.hubWelcomeBack()}
        </span>
        <h1 className="font-display text-5xl leading-none font-medium tracking-tight enter-from-below [animation-delay:100ms] md:text-7xl lg:text-8xl">
          <Trans
            message={m.hubHero()}
            components={{ gold: (c) => <em className="font-normal text-gold italic">{c}</em> }}
          />
        </h1>
        <button
          type="button"
          data-testid="new-campaign-cta"
          onClick={() => {
            void create();
          }}
          disabled={state.creating}
          className="inline-flex items-center gap-2 rounded-full bg-gold px-6 py-3 text-sm font-medium text-white shadow-md shadow-gold/25 transition-colors hover:bg-gold/90 disabled:cursor-not-allowed disabled:opacity-60"
        >
          {state.creating ? m.hubCreateInProgress() : m.hubStartNewCampaign()}
        </button>
        {state.error !== null ? (
          <p role="alert" className="text-sm text-foreground/70">
            {state.error}
          </p>
        ) : null}
      </header>
      <div data-testid="campaign-list" className="grid grid-cols-1 gap-5 md:grid-cols-2">
        {campaigns.map((c) => (
          <CampaignCard key={c.id} campaign={c} loaded={c.loaded} />
        ))}
      </div>
    </section>
  );
}

export const Route = createFileRoute("/_authed/")({
  component: Hub,
});
