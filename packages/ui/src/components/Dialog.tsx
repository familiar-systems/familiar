import type { ReactElement, ReactNode } from "react";
import {
  Modal as AriaModal,
  Dialog,
  DialogTrigger,
  Heading,
  ModalOverlay,
} from "react-aria-components";
import type { ModalOverlayProps } from "react-aria-components";

import { cn } from "../lib/cn";

// Extends the overlay props so the modal works both ways: inside a DialogTrigger
// (open state from context) or controlled by a parent that mounts it on demand
// (isOpen + onOpenChange). isDismissable / isKeyboardDismissDisabled ride through
// too, so a caller can hold the modal open during an in-flight request.
export interface ModalProps extends Omit<ModalOverlayProps, "className" | "children"> {
  children: ReactNode;
  /** Click-outside / Escape dismissal. Default true. */
  isDismissable?: boolean;
  className?: string;
}

// A scrim over the parchment plus an opaque panel that sits on it (no
// glassmorphism, per the design). Only an entry animation: with no exit
// animation React Aria unmounts immediately on close, which keeps interaction
// tests deterministic. inset-0 + flex centering is symmetric, so RTL needs no
// changes; React Aria handles focus scope, Escape, and scroll lock.
const overlayClass =
  "fixed inset-0 z-50 flex items-center justify-center bg-foreground/20 p-4 backdrop-blur-sm data-[entering]:animate-in data-[entering]:fade-in-0";

const panelClass =
  "w-full max-w-lg rounded-2xl border border-foreground/10 bg-background p-6 shadow-2xl shadow-primary/10 outline-none data-[entering]:animate-in data-[entering]:fade-in-0 data-[entering]:zoom-in-95";

export function Modal({
  children,
  isDismissable = true,
  className,
  ...props
}: ModalProps): ReactElement {
  return (
    <ModalOverlay {...props} isDismissable={isDismissable} className={overlayClass}>
      <AriaModal className={cn(panelClass, className)}>{children}</AriaModal>
    </ModalOverlay>
  );
}

// Re-export the unstyled composition pieces so callers assemble the standard
// React Aria pattern: <DialogTrigger><Button/><Modal><Dialog>…</Dialog></Modal>.
export { Dialog, DialogTrigger, Heading };
