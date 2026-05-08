import { createFileRoute } from "@tanstack/react-router";
import { EmptyHubCard } from "../../components/EmptyHubCard";

function Hub(): React.ReactElement {
  // No campaigns endpoint yet. When it lands, derive this from the response
  // and bring the "Your worlds await" header back for the populated state.
  const hasCampaigns = false;

  return hasCampaigns ? (
    <section className="mx-auto w-full max-w-6xl px-8 pt-24 pb-32">
      <header className="mb-16 text-center">
        <span className="mb-4 block text-xs font-medium tracking-[0.28em] text-muted-foreground uppercase enter-from-below">
          Welcome back
        </span>
        <h1 className="font-display text-5xl leading-none font-medium tracking-tight enter-from-below [animation-delay:100ms] md:text-7xl lg:text-8xl">
          Your <em className="font-normal text-gold italic">worlds</em> await.
        </h1>
      </header>
      {/* TODO: campaign grid */}
    </section>
  ) : (
    <section className="flex flex-1 items-center justify-center px-8 py-16">
      <div className="w-full max-w-3xl">
        <EmptyHubCard />
      </div>
    </section>
  );
}

export const Route = createFileRoute("/_authed/")({
  component: Hub,
});
