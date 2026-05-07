import type { MeResponse } from "@familiar-systems/types-app";
import { Plus } from "lucide-react";
import { assetPath, spaRoute } from "../lib/paths";
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
      <div className="mx-auto max-w-7xl px-6 h-16 flex items-center justify-between gap-6">
        <a
          href={spaRoute("")}
          aria-label="familiar.systems hub"
          className="inline-flex items-center gap-3 transition-opacity hover:opacity-80"
        >
          <span
            aria-hidden="true"
            className="block h-7 w-7 bg-foreground dark:bg-primary transition-[filter] duration-300 dark:drop-shadow-[0_0_8px_var(--color-primary)]"
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
        </a>

        <div className="flex items-center gap-3">
          <ThemeToggle />
          {hasCampaigns ? (
            <button
              type="button"
              onClick={onNewCampaign}
              className="inline-flex items-center gap-2 rounded-full bg-gold text-white shadow-lg shadow-gold/25 hover:bg-gold/90 transition-colors px-4 py-2 text-sm font-medium"
            >
              <Plus className="w-4 h-4" />
              <span>New campaign</span>
            </button>
          ) : null}
          <UserMenu me={me} />
        </div>
      </div>
    </nav>
  );
}
