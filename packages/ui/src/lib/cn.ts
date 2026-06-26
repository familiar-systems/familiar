// Minimal class joiner: drops falsy parts and space-joins the rest. Sufficient
// for composing variant + size + caller className. If Tailwind class-conflict
// resolution becomes a real need (caller overriding a baked-in utility), swap
// this for clsx + tailwind-merge then.
export function cn(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}
