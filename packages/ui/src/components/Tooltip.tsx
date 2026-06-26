import type { ReactElement, ReactNode } from "react";
import { Tooltip as AriaTooltip, TooltipTrigger } from "react-aria-components";

import { cn } from "../lib/cn";

export interface TooltipProps {
  content: ReactNode;
  /** The trigger: a focusable React Aria element (e.g. our Button). */
  children: ReactElement;
  className?: string;
  /** Hover warmup in ms; focus shows it immediately regardless. */
  delay?: number;
}

// Inverted surface (dark-on-light in light theme, and vice versa) for contrast
// against the parchment. React Aria handles placement, hover/focus, dismissal,
// and RTL mirroring of the offset.
export function Tooltip({ content, children, className, delay = 600 }: TooltipProps): ReactElement {
  return (
    <TooltipTrigger delay={delay}>
      {children}
      <AriaTooltip
        offset={6}
        className={cn(
          "rounded-md bg-foreground px-2.5 py-1.5 text-xs text-background shadow-lg",
          className,
        )}
      >
        {content}
      </AriaTooltip>
    </TooltipTrigger>
  );
}
