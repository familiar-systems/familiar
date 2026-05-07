import { Plus } from "lucide-react";
import { EpicBackdrop } from "./components/EpicBackdrop";
import { HubNav } from "./components/HubNav";
import { useAuthedMe } from "./lib/auth";
import { assetPath } from "./lib/paths";

const GRID_PATTERN_URL = `url('${assetPath("/grid-pattern.svg")}')`;

export function Hub(): React.ReactElement {
  const { me, error } = useAuthedMe();

  if (error) return <pre className="p-8">Error: {error}</pre>;
  if (!me) return <div className="p-8 text-muted-foreground">Loading...</div>;

  // No campaigns endpoint yet. When it lands, derive this from the response
  // and bring the "Your worlds await" header back for the populated state.
  const hasCampaigns = false;

  return (
    <main className="relative min-h-screen overflow-hidden bg-background text-foreground">
      <EpicBackdrop />

      <div className="relative z-10 flex min-h-screen flex-col">
        <HubNav me={me} hasCampaigns={hasCampaigns} />

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
      </div>
    </main>
  );
}

// Solid-background card so the epic-square art doesn't bleed through and
// steal legibility. Hover lifts via the style guide's interactive-card
// pattern. The CTA is wired to a no-op until the campaign-create flow
// lands; the warn makes the unwired state visible to anyone running dev.
function EmptyHubCard(): React.ReactElement {
  const onStart = (): void => {
    console.warn("Start your first campaign: campaign-create flow not wired yet");
  };

  return (
    <article
      className={[
        "rounded-2xl bg-background overflow-hidden",
        "border border-foreground/10",
        "shadow-[0_8px_32px_-16px_rgb(28_25_23/0.25)]",
        "dark:shadow-[0_12px_40px_-18px_rgb(0_0_0/0.55)]",
        "transition-all duration-300",
        "hover:-translate-y-1 hover:shadow-2xl hover:shadow-primary/10 hover:border-primary/20",
        "motion-safe:animate-in motion-safe:fade-in-0 motion-safe:slide-in-from-bottom-4 motion-safe:duration-700",
      ].join(" ")}
    >
      <EmptyCardBanner className="h-14" />

      <div className="px-10 md:px-14 py-14 text-center">
        <p className="font-display italic text-lg md:text-xl leading-relaxed text-foreground/90 max-w-lg mx-auto mb-6 text-pretty">
          You sit at the desk, empty but for one paper. The sheet is blank but your mind conjures a
          large, black corvid gazing back at you. Its glowing, purple eyes lock with yours.
          &ldquo;Master Wizard,&rdquo; the raven whispers,
        </p>
        <p className="font-display italic font-medium text-2xl md:text-3xl leading-snug max-w-lg mx-auto mb-9 text-pretty">
          &ldquo;your <span className="text-gold">worlds</span> await.&rdquo;
        </p>
        <button
          type="button"
          onClick={onStart}
          className="inline-flex items-center gap-2 rounded-full bg-gold text-white shadow-lg shadow-gold/25 hover:bg-gold/90 transition-colors px-8 py-4 font-medium"
        >
          <Plus className="w-4 h-4" />
          <span>Start your first campaign.</span>
        </button>
      </div>

      <EmptyCardBanner className="h-14" flip />
    </article>
  );
}

interface EmptyCardBannerProps {
  className: string;
  flip?: boolean;
}

// Two banners frame the card so the top doesn't feel like the only visual
// weight. Bottom is taller than top by design: a heavier base reads as a
// plinth, the card "stands" on it. Same gradient flipped via scaleY so the
// glow lands toward the card body on both edges.
function EmptyCardBanner({ className, flip = false }: EmptyCardBannerProps): React.ReactElement {
  return (
    <div
      aria-hidden="true"
      className={`relative overflow-hidden ${className}`}
      style={{
        background:
          "radial-gradient(ellipse at 50% 120%, rgb(90 74 106 / 0.25), transparent 70%), linear-gradient(160deg, color-mix(in srgb, var(--color-primary), transparent 88%), color-mix(in srgb, var(--color-bronze-muted), transparent 70%))",
        transform: flip ? "scaleY(-1)" : undefined,
      }}
    >
      <div
        aria-hidden="true"
        className="absolute inset-0 opacity-[0.18]"
        style={{ backgroundImage: GRID_PATTERN_URL }}
      />
    </div>
  );
}
