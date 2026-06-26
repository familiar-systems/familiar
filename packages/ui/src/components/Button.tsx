import { Button as AriaButton } from "react-aria-components";
import type { ButtonProps as AriaButtonProps } from "react-aria-components";

import { cn } from "../lib/cn";

export type ButtonVariant = "primary" | "secondary" | "outline" | "ghost" | "icon";
export type ButtonSize = "sm" | "md" | "lg";

export interface ButtonProps extends Omit<AriaButtonProps, "className"> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  className?: string;
}

// Color treatment per the design system's Buttons card: primary is the gold
// pill (ceremony), secondary/outline/ghost are the standard rounded-lg set,
// icon is a round affordance. Padding is symmetric (px/py) so no RTL mirroring
// is needed here.
const base =
  "inline-flex items-center justify-center font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/50 disabled:pointer-events-none disabled:opacity-50";

const variantClass: Record<ButtonVariant, string> = {
  primary: "rounded-full bg-gold text-white shadow-lg shadow-gold/25 hover:bg-gold/90",
  secondary:
    "rounded-lg border border-foreground/10 bg-foreground/5 text-foreground hover:bg-foreground/10",
  outline:
    "rounded-lg border border-foreground/10 bg-transparent text-foreground hover:bg-foreground/5",
  ghost: "rounded-lg text-primary hover:bg-primary/5",
  icon: "rounded-full border border-foreground/10 bg-foreground/5 text-foreground hover:bg-foreground/10",
};

const paddedSize: Record<ButtonSize, string> = {
  sm: "px-3 py-1.5 text-xs",
  md: "px-4 py-2 text-sm",
  lg: "px-8 py-4 text-base",
};

const iconSize: Record<ButtonSize, string> = {
  sm: "size-8 text-xs",
  md: "size-9 text-sm",
  lg: "size-11 text-base",
};

export function Button({
  variant = "primary",
  size = "md",
  className,
  ...props
}: ButtonProps): React.ReactElement {
  const sizeClass = variant === "icon" ? iconSize[size] : paddedSize[size];
  return (
    <AriaButton {...props} className={cn(base, variantClass[variant], sizeClass, className)} />
  );
}
