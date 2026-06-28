import type { ReactElement, ReactNode } from "react";
import { Checkbox as AriaCheckbox } from "react-aria-components";
import type { CheckboxProps as AriaCheckboxProps } from "react-aria-components";

import { cn } from "../lib/cn";

export interface CheckboxProps extends Omit<AriaCheckboxProps, "className" | "children"> {
  children: ReactNode;
  /** `danger` tints the indicator + label red for destructive options. */
  tone?: "default" | "danger";
  className?: string;
}

// React Aria Checkbox exposes its state as data-* on the root <label>, so the
// indicator styles purely off group-data-[selected] — no render function. The
// check glyph is an inline SVG (no icon dependency in this package). gap-2 is
// symmetric, so the control mirrors cleanly under RTL.
const rootTone: Record<"default" | "danger", string> = {
  default: "text-foreground",
  danger: "text-red-700 dark:text-red-400",
};

const boxBase =
  "flex size-4 shrink-0 items-center justify-center rounded border border-foreground/30 text-primary-foreground transition-colors group-data-[focus-visible]:ring-2";

const boxTone: Record<"default" | "danger", string> = {
  default:
    "group-data-[selected]:border-primary group-data-[selected]:bg-primary group-data-[focus-visible]:ring-primary/40",
  danger:
    "group-data-[selected]:border-red-700 group-data-[selected]:bg-red-700 group-data-[focus-visible]:ring-red-700/40",
};

export function Checkbox({
  children,
  tone = "default",
  className,
  ...props
}: CheckboxProps): ReactElement {
  return (
    <AriaCheckbox
      {...props}
      className={cn(
        "group flex cursor-pointer items-center gap-2 text-sm data-[disabled]:cursor-default data-[disabled]:opacity-50",
        rootTone[tone],
        className,
      )}
    >
      <span className={cn(boxBase, boxTone[tone])} aria-hidden="true">
        <svg
          viewBox="0 0 16 16"
          className="size-3 opacity-0 group-data-[selected]:opacity-100"
          fill="none"
          stroke="currentColor"
          strokeWidth={2.5}
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d="M3.5 8.5l3 3 6-7" />
        </svg>
      </span>
      {children}
    </AriaCheckbox>
  );
}
