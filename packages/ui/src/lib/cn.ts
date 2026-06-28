import { twMerge } from "tailwind-merge";

// Joins class parts and resolves Tailwind conflicts so the LAST of two competing
// utilities wins (e.g. a caller's `bg-red-700` overrides a variant's baked
// `bg-gold`, a `rounded-sm` overrides `rounded-lg`). Without this, two conflicting
// utilities both ship and the winner is cascade-order luck. twMerge ignores falsy
// parts, so callers can pass `cond && "class"` directly.
export function cn(...parts: Array<string | false | null | undefined>): string {
  return twMerge(parts);
}
