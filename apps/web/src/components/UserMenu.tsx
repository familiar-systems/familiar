import type { MeResponse } from "@familiar-systems/types-app";
import { Link } from "@tanstack/react-router";
import { LogOut, Settings as SettingsIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { hanko } from "../lib/hanko";
import { spaRoute } from "../lib/paths";

interface UserMenuProps {
  me: MeResponse;
}

export function UserMenu({ me }: UserMenuProps): React.ReactElement {
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const firstItemRef = useRef<HTMLAnchorElement>(null);

  useEffect(() => {
    if (!open) return;
    const onMouseDown = (e: MouseEvent): void => {
      if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onMouseDown);
    document.addEventListener("keydown", onKey);
    firstItemRef.current?.focus();
    return () => {
      document.removeEventListener("mousedown", onMouseDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const initial = (me.email[0] ?? "?").toUpperCase();

  const onSignOut = async (e: React.MouseEvent): Promise<void> => {
    e.preventDefault();
    try {
      await hanko.logout();
    } finally {
      window.location.assign(spaRoute("login"));
    }
  };

  return (
    <div ref={wrapRef} className="relative">
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label="Open account menu"
        onClick={() => setOpen((o) => !o)}
        className={[
          "h-9 w-9 rounded-full",
          "border border-foreground/10 bg-background/60 backdrop-blur-sm",
          "font-display text-sm text-primary",
          "hover:bg-background/80 transition-colors",
          "inline-flex items-center justify-center",
        ].join(" ")}
      >
        {initial}
      </button>

      {open ? (
        <div
          role="menu"
          className={[
            "absolute right-0 top-11 w-64 z-20",
            "rounded-2xl border border-foreground/10 bg-background/85 backdrop-blur-md",
            "shadow-2xl shadow-primary/10 p-2",
          ].join(" ")}
        >
          <div className="px-3 py-2">
            <span className="text-muted-foreground block text-xs tracking-[0.2em] uppercase">
              Signed in as
            </span>
            <span
              className="mt-1 block truncate font-display text-sm text-foreground"
              title={me.email}
            >
              {me.email}
            </span>
          </div>
          <div className="my-1 h-px bg-foreground/10" />
          <Link
            ref={firstItemRef}
            role="menuitem"
            to="/settings"
            className="flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors hover:bg-foreground/5 focus:bg-foreground/5 focus:outline-none"
          >
            <SettingsIcon className="size-4 text-primary" />
            <span>Settings</span>
          </Link>
          <a
            role="menuitem"
            href={spaRoute("login")}
            onClick={onSignOut}
            className="flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors hover:bg-foreground/5 focus:bg-foreground/5 focus:outline-none"
          >
            <LogOut className="text-muted-foreground size-4" />
            <span>Sign out</span>
          </a>
        </div>
      ) : null}
    </div>
  );
}
