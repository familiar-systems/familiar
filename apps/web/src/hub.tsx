import { useRouteContext } from "@tanstack/react-router";
import { EmptyHubCard } from "./components/EmptyHubCard";
import { Shell } from "./components/Shell";

export function Hub(): React.ReactElement {
  // me is refined to MeResponse by requireAuth; auth/loading/error states
  // are handled in App and the router's beforeLoad, not here.
  const { me } = useRouteContext({ from: "/" });

  // No campaigns endpoint yet. When it lands, derive this from the response
  // and bring the "Your worlds await" header back for the populated state.
  const hasCampaigns = false;

  return (
    <Shell me={me} hasCampaigns={hasCampaigns}>
      {hasCampaigns ? (
        <section className="mx-auto w-full max-w-6xl px-8 pt-24 pb-32">
          <header className="mb-16 text-center">
            <span className="block mb-4 text-xs uppercase tracking-[0.28em] font-medium text-muted-foreground motion-safe:animate-in motion-safe:fade-in-0 motion-safe:slide-in-from-bottom-4 motion-safe:duration-700">
              Welcome back
            </span>
            <h1 className="font-display font-medium text-5xl md:text-7xl lg:text-8xl leading-none tracking-tight motion-safe:animate-in motion-safe:fade-in-0 motion-safe:slide-in-from-bottom-4 motion-safe:duration-700 [animation-delay:100ms]">
              Your <em className="italic font-normal text-gold">worlds</em> await.
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
      )}
    </Shell>
  );
}
