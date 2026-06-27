import type { MeResponse } from "@familiar-systems/types-app";
import { Menu, MenuItem, MenuTrigger } from "@familiar-systems/ui";
import { useNavigate } from "@tanstack/react-router";
import { LogOut, Settings as SettingsIcon } from "lucide-react";
import { Button as AriaButton, Header, Separator } from "react-aria-components";
import { m } from "../paraglide/messages.js";
import { hanko } from "../lib/hanko";
import { spaRoute } from "../lib/paths";

interface UserMenuProps {
  me: MeResponse;
}

export function UserMenu({ me }: UserMenuProps): React.ReactElement {
  const navigate = useNavigate();
  const initial = (me.email[0] ?? "?").toUpperCase();

  // hanko.logout() can reject (network); redirect regardless so the user always
  // lands on /login. A hard navigation (not the router) clears in-memory state.
  const onSignOut = async (): Promise<void> => {
    try {
      await hanko.logout();
    } finally {
      window.location.assign(spaRoute("login"));
    }
  };

  // The avatar is a bespoke trigger, not a generic Button variant, so it draws on
  // React Aria's Button directly. MenuTrigger + Popover supply the open state,
  // outside-press/Escape dismissal, and focus management.
  return (
    <MenuTrigger>
      <AriaButton
        aria-label={m.userMenuOpenAriaLabel()}
        className="inline-flex size-9 items-center justify-center rounded-full border border-foreground/10 bg-background/60 font-display text-sm text-primary backdrop-blur-sm transition-colors outline-none hover:bg-background/80 data-[focus-visible]:ring-2 data-[focus-visible]:ring-primary/50"
      >
        {initial}
      </AriaButton>
      <Menu className="w-64">
        <Header className="px-3 py-2">
          <span className="block text-xs tracking-[0.2em] text-muted-foreground uppercase">
            {m.userMenuSignedInAs()}
          </span>
          <span
            className="mt-1 block truncate font-display text-sm text-foreground"
            title={me.email}
          >
            {me.email}
          </span>
        </Header>
        <Separator className="my-1 h-px bg-foreground/10" />
        <MenuItem className="gap-3" onAction={() => void navigate({ to: "/settings" })}>
          <SettingsIcon className="size-4 text-primary" />
          <span>{m.userMenuSettings()}</span>
        </MenuItem>
        <MenuItem className="gap-3" onAction={() => void onSignOut()}>
          <LogOut className="size-4 text-muted-foreground" />
          <span>{m.userMenuSignOut()}</span>
        </MenuItem>
      </Menu>
    </MenuTrigger>
  );
}
