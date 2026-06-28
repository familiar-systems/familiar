import type { ReactElement, ReactNode } from "react";
import {
  ComboBox as AriaComboBox,
  Button as AriaButton,
  Input,
  Label,
  ListBox,
  ListBoxItem,
  Popover,
} from "react-aria-components";
import type {
  ComboBoxProps as AriaComboBoxProps,
  InputProps,
  ListBoxItemProps,
} from "react-aria-components";

import { cn } from "../lib/cn";

export interface ComboBoxProps<T extends object = object> extends Omit<
  AriaComboBoxProps<T>,
  "className" | "children"
> {
  /** Optional uppercase field label; omit for an inline (sentence) combobox. */
  label?: string;
  placeholder?: string;
  /** ListBox content: static <ComboBoxItem>s or a render fn over `items`. */
  children: ReactNode | ((item: T) => ReactNode);
  /**
   * `boxed` (default) is a bordered field with a chevron; `inline` is a
   * borderless dashed-underline input that flows inside running text (the
   * relationship sentence builder).
   */
  variant?: "boxed" | "inline";
  /** Escape hatch for raw <input> attributes (data-testid, width emphasis). */
  inputProps?: Omit<InputProps, "className"> & {
    className?: string;
    [data: `data-${string}`]: string | undefined;
  };
  className?: string;
}

// Generic on purpose: it filters whatever items it's given (client-side by
// default; pass already-filtered `items` + controlled `inputValue`/`onInputChange`
// for server search) and knows nothing about the domain. The app composes "use
// custom" / "create" rows on top via `allowsCustomValue` and its own sentinel
// items. ps/pe (not pl/pr) keep the chevron on the inline-end so it mirrors under
// RTL; React Aria flips popover placement and arrow keys.
const labelClass = "text-[11px] uppercase tracking-[0.2em] text-muted-foreground";

const boxedInputClass =
  "w-full rounded-lg border border-foreground/15 bg-background/60 py-2 ps-3 pe-9 text-sm text-foreground caret-primary outline-none transition-colors placeholder:italic placeholder:text-foreground/30 focus:border-primary focus:ring-2 focus:ring-primary/20";

const inlineInputClass =
  "w-full border-0 border-b border-dashed border-foreground/30 bg-transparent px-1 py-0.5 text-foreground caret-primary outline-none transition-colors placeholder:italic placeholder:text-foreground/40 focus:border-solid focus:border-primary";

const popoverClass =
  "min-w-[max(12rem,var(--trigger-width))] rounded-xl border border-foreground/10 bg-background p-1 shadow-2xl shadow-primary/10 outline-none data-[entering]:animate-in data-[entering]:fade-in-0 data-[entering]:zoom-in-95";

const optionClass =
  "flex cursor-default items-center rounded-lg px-3 py-2 text-sm text-foreground outline-none select-none data-[focused]:bg-primary/5 data-[focused]:text-primary data-[selected]:font-medium data-[selected]:text-primary";

const chevron = (
  <svg
    viewBox="0 0 16 16"
    aria-hidden="true"
    className="size-4"
    fill="none"
    stroke="currentColor"
    strokeWidth={1.5}
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <path d="M4 6l4 4 4-4" />
  </svg>
);

export function ComboBox<T extends object = object>({
  label,
  placeholder,
  children,
  variant = "boxed",
  inputProps,
  className,
  ...props
}: ComboBoxProps<T>): ReactElement {
  const isBoxed = variant === "boxed";
  const { className: inputClassName, ...restInputProps } = inputProps ?? {};
  const inputEl = (
    <Input
      {...restInputProps}
      {...(placeholder === undefined ? {} : { placeholder })}
      className={cn(isBoxed ? boxedInputClass : inlineInputClass, inputClassName)}
    />
  );
  return (
    <AriaComboBox
      {...props}
      className={cn(
        isBoxed ? "group flex flex-col gap-1.5" : "group inline-flex flex-col",
        className,
      )}
    >
      {label !== undefined && <Label className={labelClass}>{label}</Label>}
      {isBoxed ? (
        <div className="relative">
          {inputEl}
          <AriaButton className="absolute inset-y-0 end-2 flex items-center text-muted-foreground outline-none">
            {chevron}
          </AriaButton>
        </div>
      ) : (
        inputEl
      )}
      <Popover className={popoverClass}>
        <ListBox className="outline-none">{children}</ListBox>
      </Popover>
    </AriaComboBox>
  );
}

export interface ComboBoxItemProps extends Omit<ListBoxItemProps, "className"> {
  className?: string;
}

export function ComboBoxItem({ className, ...props }: ComboBoxItemProps): ReactElement {
  return <ListBoxItem {...props} className={cn(optionClass, className)} />;
}
