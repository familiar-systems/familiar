import type { ReactElement } from "react";
import { ToggleButton, ToggleButtonGroup } from "react-aria-components";
import type { ToggleButtonGroupProps, ToggleButtonProps } from "react-aria-components";

import { cn } from "../lib/cn";

export interface SegmentedControlProps extends Omit<
  ToggleButtonGroupProps,
  "className" | "selectionMode"
> {
  className?: string;
}

// A single-select segmented control (the knowledge / factuality toggles): a
// crisp bordered box whose segments butt together with hairline dividers, not a
// pill. selectionMode is pinned to single + disallowEmptySelection so there is
// always exactly one active segment — it models a sum type, not a set of flags.
// Segments are symmetric and the divider is a logical inline-start border, so it
// mirrors cleanly under RTL; React Aria provides roving arrow-key focus.
const groupClass = "inline-flex w-fit overflow-hidden rounded-lg border border-foreground/15";

const itemClass =
  "inline-flex items-center gap-1.5 border-foreground/15 px-3 py-1.5 text-[13px] text-muted-foreground outline-none transition-colors [&:not(:first-child)]:border-s data-[hovered]:bg-foreground/5 data-[hovered]:text-foreground data-[selected]:bg-foreground/8 data-[selected]:font-semibold data-[selected]:text-foreground data-[focus-visible]:bg-foreground/5 data-[disabled]:opacity-40";

export function SegmentedControl({ className, ...props }: SegmentedControlProps): ReactElement {
  return (
    <ToggleButtonGroup
      {...props}
      selectionMode="single"
      disallowEmptySelection
      className={cn(groupClass, className)}
    />
  );
}

export interface SegmentedItemProps extends Omit<ToggleButtonProps, "className"> {
  className?: string;
}

export function SegmentedItem({ className, ...props }: SegmentedItemProps): ReactElement {
  return <ToggleButton {...props} className={cn(itemClass, className)} />;
}
