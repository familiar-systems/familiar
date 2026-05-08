import type { MeResponse } from "@familiar-systems/types-app";
import { Link } from "@tanstack/react-router";
import { Plus } from "lucide-react";
import { assetPath } from "../lib/paths";
import { ThemeToggle } from "./ThemeToggle";
import { UserMenu } from "./UserMenu";

const RAVEN_URL = `url('${assetPath("/raven-icon.svg")}')`;

interface HubNavProps {
  me: MeResponse;
  hasCampaigns: boolean;
  onNewCampaign?: () => void;
}

export function HubNav({ me, hasCampaigns, onNewCampaign }: HubNavProps): React.ReactElement {
  return (
    <nav className="sticky top-0 z-30 border-b border-foreground/5 bg-background/40 backdrop-blur-md">
      <div className="mx-auto flex h-16 max-w-7xl items-center justify-between gap-6 px-6">
        <Link
          to="/"
          aria-label="familiar.systems hub"
          className="inline-flex items-center gap-3 transition-opacity hover:opacity-80"
        >
          <span
            aria-hidden="true"
            className="block size-7 bg-foreground transition-[filter] duration-300 dark:bg-primary dark:drop-shadow-[0_0_8px_var(--color-primary)]"
            style={{
              maskImage: RAVEN_URL,
              maskRepeat: "no-repeat",
              maskPosition: "center",
              maskSize: "contain",
              WebkitMaskImage: RAVEN_URL,
              WebkitMaskRepeat: "no-repeat",
              WebkitMaskPosition: "center",
              WebkitMaskSize: "contain",
            }}
          />
          <span className="font-display text-xl font-medium tracking-tight text-foreground">
            familiar.systems
          </span>
        </Link>

        <div className="flex items-center gap-3">
          <ThemeToggle />
          {hasCampaigns ? (
            <button
              type="button"
              onClick={onNewCampaign}
              className="inline-flex items-center gap-2 rounded-full bg-gold px-4 py-2 text-sm font-medium text-white shadow-lg shadow-gold/25 transition-colors hover:bg-gold/90"
            >
              <Plus className="size-4" />
              <span>New campaign</span>
            </button>
          ) : null}
          <UserMenu me={me} />
        </div>
      </div>
    </nav>
  );
}
