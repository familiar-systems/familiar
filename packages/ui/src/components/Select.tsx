import type { ReactElement, ReactNode } from "react";
import {
  Select as AriaSelect,
  Button as AriaButton,
  Label,
  ListBox,
  ListBoxItem,
  Popover,
  SelectValue,
} from "react-aria-components";
import type { SelectProps as AriaSelectProps, ListBoxItemProps } from "react-aria-components";

import { cn } from "../lib/cn";

export interface SelectProps<T extends object = object> extends Omit<
  AriaSelectProps<T>,
  "className" | "children"
> {
  label?: string;
  /** ListBox content: static <SelectItem>s or a render fn over `items`. */
  children: ReactNode | ((item: T) => ReactNode);
  className?: string;
}

// Popover-backed select that replaces native <select> styling (the relationship
// session pickers). The trigger shows SelectValue (or the `placeholder`); the
// list reuses the same option treatment as ComboBox for a coherent dropdown.
// ps/pe spacing mirrors under RTL; React Aria flips placement and arrow keys.
const labelClass = "text-[11px] uppercase tracking-[0.2em] text-muted-foreground";

const triggerClass =
  "flex items-center justify-between gap-2 rounded-lg border border-foreground/15 bg-background/60 py-2 ps-3 pe-2 text-sm text-foreground outline-none transition-colors data-[focus-visible]:border-primary data-[focus-visible]:ring-2 data-[focus-visible]:ring-primary/20 data-[disabled]:opacity-50";

const popoverClass =
  "min-w-[var(--trigger-width)] rounded-xl border border-foreground/10 bg-background p-1 shadow-2xl shadow-primary/10 outline-none data-[entering]:animate-in data-[entering]:fade-in-0 data-[entering]:zoom-in-95";

const optionClass =
  "flex cursor-default items-center rounded-lg px-3 py-2 text-sm text-foreground outline-none select-none data-[focused]:bg-primary/5 data-[focused]:text-primary data-[selected]:font-medium data-[selected]:text-primary";

export function Select<T extends object = object>({
  label,
  children,
  className,
  ...props
}: SelectProps<T>): ReactElement {
  return (
    <AriaSelect {...props} className={cn("group flex flex-col gap-1.5", className)}>
      {label !== undefined && <Label className={labelClass}>{label}</Label>}
      <AriaButton className={triggerClass}>
        <SelectValue className="truncate data-[placeholder]:italic data-[placeholder]:text-foreground/40" />
        <svg
          viewBox="0 0 16 16"
          aria-hidden="true"
          className="size-4 text-muted-foreground"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.5}
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d="M4 6l4 4 4-4" />
        </svg>
      </AriaButton>
      <Popover className={popoverClass}>
        <ListBox className="outline-none">{children}</ListBox>
      </Popover>
    </AriaSelect>
  );
}

export interface SelectItemProps extends Omit<ListBoxItemProps, "className"> {
  className?: string;
}

export function SelectItem({ className, ...props }: SelectItemProps): ReactElement {
  return <ListBoxItem {...props} className={cn(optionClass, className)} />;
}
