import { createFileRoute } from "@tanstack/react-router";
import { EmptyHubCard } from "../../components/EmptyHubCard";

function Hub(): React.ReactElement {
  // No campaigns endpoint yet. When it lands, derive this from the response
  // and bring the "Your worlds await" header back for the populated state.
  const hasCampaigns = false;

  return hasCampaigns ? (
    <section className="mx-auto w-full max-w-6xl px-8 pt-24 pb-32">
      <header className="mb-16 text-center">
        <span className="text-muted-foreground mb-4 block text-xs font-medium tracking-[0.28em] uppercase motion-safe:duration-700 motion-safe:animate-in motion-safe:fade-in-0 motion-safe:slide-in-from-bottom-4">
          Welcome back
        </span>
        <h1 className="font-display text-5xl leading-none font-medium tracking-tight [animation-delay:100ms] motion-safe:duration-700 motion-safe:animate-in motion-safe:fade-in-0 motion-safe:slide-in-from-bottom-4 md:text-7xl lg:text-8xl">
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
