import type { ReactElement } from "react";
import {
  Menu as AriaMenu,
  MenuItem as AriaMenuItem,
  MenuTrigger,
  Popover,
} from "react-aria-components";
import type {
  MenuItemProps as AriaMenuItemProps,
  MenuProps as AriaMenuProps,
} from "react-aria-components";

import { cn } from "../lib/cn";

export interface MenuProps extends Omit<AriaMenuProps<object>, "className"> {
  className?: string;
}

// Opaque popover on the parchment + capsule-highlighted items. React Aria moves
// focus to follow the mouse, so data-focused covers both keyboard and hover.
// px/py are symmetric and item text aligns to start, so it mirrors under RTL;
// React Aria flips the popover placement automatically.
const popoverClass =
  "min-w-40 rounded-xl border border-foreground/10 bg-background p-1 shadow-2xl shadow-primary/10 outline-none data-[entering]:animate-in data-[entering]:fade-in-0 data-[entering]:zoom-in-95";

const itemClass =
  "flex cursor-default items-center rounded-lg px-3 py-2 text-sm text-foreground outline-none select-none data-[focused]:bg-primary/5 data-[focused]:text-primary";

export function Menu({ className, ...props }: MenuProps): ReactElement {
  return (
    <Popover className={popoverClass}>
      <AriaMenu {...props} className={cn("outline-none", className)} />
    </Popover>
  );
}

export interface MenuItemProps extends Omit<AriaMenuItemProps<object>, "className"> {
  className?: string;
}

export function MenuItem({ className, ...props }: MenuItemProps): ReactElement {
  return <AriaMenuItem {...props} className={cn(itemClass, className)} />;
}

export { MenuTrigger };
