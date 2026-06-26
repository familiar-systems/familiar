import { Input, Label, TextField as AriaTextField } from "react-aria-components";
import type { TextFieldProps as AriaTextFieldProps } from "react-aria-components";

import { cn } from "../lib/cn";

export interface TextFieldProps extends Omit<AriaTextFieldProps, "className"> {
  label: string;
  placeholder?: string;
  className?: string;
}

// Parchment-native field per the Inputs card: an uppercase label sits atop a
// hairline that deepens to plum on focus, and the input is borderless "ink"
// written directly on the page. Spacing is block-direction only (pb/mt), so it
// mirrors cleanly under RTL with no changes.
const labelClass =
  "border-b border-foreground/20 pb-1.5 text-[11px] uppercase tracking-[0.2em] text-muted-foreground group-focus-within:border-b-[1.5px] group-focus-within:border-primary";

const inputClass =
  "mt-1.5 border-0 bg-transparent p-0 font-display text-[22px] leading-[1.35] text-foreground caret-primary outline-none placeholder:italic placeholder:text-foreground/30";

export function TextField({
  label,
  placeholder,
  className,
  ...props
}: TextFieldProps): React.ReactElement {
  return (
    <AriaTextField {...props} className={cn("group flex flex-col", className)}>
      <Label className={labelClass}>{label}</Label>
      <Input {...(placeholder === undefined ? {} : { placeholder })} className={inputClass} />
    </AriaTextField>
  );
}
