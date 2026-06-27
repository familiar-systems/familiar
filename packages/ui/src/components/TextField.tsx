import type { ReactElement } from "react";
import { Input, Label, TextField as AriaTextField } from "react-aria-components";
import type { InputProps, TextFieldProps as AriaTextFieldProps } from "react-aria-components";

import { cn } from "../lib/cn";

export interface TextFieldProps extends Omit<AriaTextFieldProps, "className"> {
  label: string;
  /** Right-aligned affordance opposite the label (e.g. "Required"). Boxed only. */
  hint?: string;
  placeholder?: string;
  /**
   * `boxed` (default) is the standard bordered form field per the style guide;
   * `inline` is the parchment-ink treatment (borderless ink on a hairline) for
   * ceremonial / hero inputs.
   */
  variant?: "boxed" | "inline";
  /**
   * Escape hatch for raw <input> attributes React Aria's TextField doesn't carry
   * (maxLength, data-testid) and per-field input styling (font/size emphasis).
   * Its className merges over the variant's; everything else spreads onto Input.
   */
  inputProps?: Omit<InputProps, "className"> & {
    className?: string;
    [data: `data-${string}`]: string | undefined;
  };
  className?: string;
}

// boxed bakes only structure (border, radius, padding, plum focus) — no font or
// text-size — so a hero field can set `font-display text-2xl` via inputProps
// without a class conflict. inline is the original parchment-ink field: an
// uppercase label on a hairline that deepens to plum on focus, over a borderless
// Cormorant input. Both space block-direction only, so they mirror under RTL.
const boxedLabelClass = "text-sm font-medium text-foreground";
const hintClass = "text-xs tracking-wider text-muted-foreground uppercase";
const boxedInputClass =
  "w-full rounded-xl border border-foreground/10 bg-background/60 px-4 py-3 text-foreground caret-primary outline-none transition-colors placeholder:text-foreground/40 focus:border-primary focus:ring-2 focus:ring-primary/20 disabled:opacity-60";

const inlineLabelClass =
  "border-b border-foreground/20 pb-1.5 text-[11px] uppercase tracking-[0.2em] text-muted-foreground group-focus-within:border-b-[1.5px] group-focus-within:border-primary";
const inlineInputClass =
  "mt-1.5 border-0 bg-transparent p-0 font-display text-[22px] leading-[1.35] text-foreground caret-primary outline-none placeholder:italic placeholder:text-foreground/30";

export function TextField({
  label,
  hint,
  placeholder,
  variant = "boxed",
  inputProps,
  className,
  ...props
}: TextFieldProps): ReactElement {
  const isBoxed = variant === "boxed";
  const { className: inputClassName, ...restInputProps } = inputProps ?? {};
  return (
    <AriaTextField {...props} className={cn("group flex flex-col", isBoxed && "gap-2", className)}>
      {isBoxed ? (
        <div className="flex items-baseline justify-between gap-4">
          <Label className={boxedLabelClass}>{label}</Label>
          {hint !== undefined && <span className={hintClass}>{hint}</span>}
        </div>
      ) : (
        <Label className={inlineLabelClass}>{label}</Label>
      )}
      <Input
        {...restInputProps}
        {...(placeholder === undefined ? {} : { placeholder })}
        className={cn(isBoxed ? boxedInputClass : inlineInputClass, inputClassName)}
      />
    </AriaTextField>
  );
}
