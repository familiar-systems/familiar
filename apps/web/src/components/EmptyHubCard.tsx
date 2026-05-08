import { Plus } from "lucide-react";
import { assetPath } from "../lib/paths";

const GRID_PATTERN_URL = `url('${assetPath("/grid-pattern.svg")}')`;

// Solid-background card so the epic-square art doesn't bleed through and
// steal legibility. Hover lifts via the style guide's interactive-card
// pattern. The CTA is wired to a no-op until the campaign-create flow
// lands; the warn makes the unwired state visible to anyone running dev.
export function EmptyHubCard(): React.ReactElement {
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
      <Banner className="h-14" />

      <div className="px-10 py-14 text-center md:px-14">
        <p className="mx-auto mb-6 max-w-lg font-display text-lg leading-relaxed text-pretty text-foreground/90 italic md:text-xl">
          You sit at the desk, empty but for one paper. The sheet is blank but your mind conjures a
          large, black corvid gazing back at you. Its glowing, purple eyes lock with yours.
          &ldquo;Master Wizard,&rdquo; the raven whispers,
        </p>
        <p className="mx-auto mb-9 max-w-lg font-display text-2xl leading-snug font-medium text-pretty italic md:text-3xl">
          &ldquo;your <span className="text-gold">worlds</span> await.&rdquo;
        </p>
        <button
          type="button"
          onClick={onStart}
          className="inline-flex items-center gap-2 rounded-full bg-gold px-8 py-4 font-medium text-white shadow-lg shadow-gold/25 transition-colors hover:bg-gold/90"
        >
          <Plus className="size-4" />
          <span>Start your first campaign.</span>
        </button>
      </div>

      <Banner className="h-14" flip />
    </article>
  );
}

interface BannerProps {
  className: string;
  flip?: boolean;
}

// Two banners frame the card so the top doesn't feel like the only visual
// weight. Same gradient flipped via scaleY on the bottom so the glow lands
// toward the card body on both edges.
function Banner({ className, flip = false }: BannerProps): React.ReactElement {
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
