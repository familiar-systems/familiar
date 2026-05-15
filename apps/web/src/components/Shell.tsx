import type { MeResponse } from "@familiar-systems/types-app";
import { EpicBackdrop } from "./EpicBackdrop";
import { HubNav } from "./HubNav";
import { WizardBackdrop } from "./WizardBackdrop";

export type ShellBackdrop = "default" | "wizard";

interface ShellProps {
  me: MeResponse;
  hasCampaigns: boolean;
  /** Which backdrop to show. Cross-fades between variants on change. */
  backdrop?: ShellBackdrop;
  children: React.ReactNode;
}

// The chrome shared by every authenticated page that lives outside a
// specific campaign: hub, settings, future billing/profile/etc. Provides
// the epic-square backdrop and the hub nav, plus a flex column so pages
// can vertically center an empty state by giving their content section
// `flex-1`.
//
// The campaign route swaps the backdrop to the wizard's graph-paper
// variant via the `backdrop` prop; both backdrops stay mounted with
// opacity transitions so the swap reads as a fade rather than a flicker.
//
// Campaign-internal pages (the editor, agent window) will eventually have
// their own shell with different chrome, so this one is intentionally not
// named anything that implies it covers all of the SPA.
export function Shell({
  me,
  hasCampaigns,
  backdrop = "default",
  children,
}: ShellProps): React.ReactElement {
  const isWizard = backdrop === "wizard";
  return (
    <main className="relative min-h-screen overflow-hidden bg-background text-foreground">
      <div
        aria-hidden="true"
        className={[
          "absolute inset-0 transition-opacity duration-700",
          isWizard ? "opacity-0" : "opacity-100",
        ].join(" ")}
        data-backdrop="default"
      >
        <EpicBackdrop />
      </div>
      <div
        aria-hidden="true"
        className={[
          "absolute inset-0 transition-opacity duration-700",
          isWizard ? "opacity-100" : "opacity-0",
        ].join(" ")}
        data-backdrop="wizard"
      >
        <WizardBackdrop />
      </div>
      <div className="relative z-10 flex min-h-screen flex-col">
        <HubNav me={me} hasCampaigns={hasCampaigns} />
        {children}
      </div>
    </main>
  );
}
