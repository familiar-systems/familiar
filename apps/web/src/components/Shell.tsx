import type { MeResponse } from "@familiar-systems/types-app";
import { EpicBackdrop } from "./EpicBackdrop";
import { HubNav } from "./HubNav";

interface ShellProps {
  me: MeResponse;
  hasCampaigns: boolean;
  children: React.ReactNode;
}

// The chrome shared by every authenticated page that lives outside a
// specific campaign: hub, settings, future billing/profile/etc. Provides
// the epic-square backdrop and the hub nav, plus a flex column so pages
// can vertically center an empty state by giving their content section
// `flex-1`.
//
// Campaign-internal pages (the editor, agent window) will eventually have
// their own shell with different chrome, so this one is intentionally not
// named anything that implies it covers all of the SPA.
export function Shell({ me, hasCampaigns, children }: ShellProps): React.ReactElement {
  return (
    <main className="relative min-h-screen overflow-hidden bg-background text-foreground">
      <EpicBackdrop />
      <div className="relative z-10 flex min-h-screen flex-col">
        <HubNav me={me} hasCampaigns={hasCampaigns} />
        {children}
      </div>
    </main>
  );
}
