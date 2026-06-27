// Renders a localized message that carries inline markup.
//
// Paraglide messages are plain strings, so a sentence like "Your worlds await."
// where one word is emphasized can't be a single message and still be
// translatable: splitting it into "Your " + <em>worlds</em> + " await." would
// freeze English word order. Instead the message keeps the whole sentence with
// named tags around the emphasized span ("Your <gold>worlds</gold> await."), and
// the call site maps each tag name to a render function. Translatable text lives
// in the message; the styling (className, element) lives in the component, where
// it belongs.
//
// Tags are non-nested: every case we have is a single span inside prose. A tag
// with no matching render function falls back to its inner text, so a typo
// degrades to plain words rather than a blank UI.

import { Fragment, type ReactNode } from "react";

// Renders one tagged span. `chunk` is the inner text (non-nested, so always a
// string); the function supplies the wrapping element and styling.
export type TransRender = (chunk: string) => ReactNode;

export interface TransProps {
  // The already-interpolated string from a Paraglide message, e.g. m.hubHero().
  message: string;
  // Maps each tag name in `message` to its renderer.
  components: Readonly<Record<string, TransRender>>;
}

type Segment =
  | { readonly kind: "text"; readonly text: string }
  | { readonly kind: "tag"; readonly name: string; readonly inner: string };

// Matched open/close via the \1 backreference; non-greedy inner so two sibling
// tags in one message each match their own span. [\s\S] (not the /s flag) keeps
// the intent obvious: inner content is opaque text, never parsed for more tags.
const TAG_PATTERN = /<([a-zA-Z][a-zA-Z0-9]*)>([\s\S]*?)<\/\1>/g;

export function parseTrans(message: string): Segment[] {
  const segments: Segment[] = [];
  let lastIndex = 0;

  for (const match of message.matchAll(TAG_PATTERN)) {
    const whole = match[0];
    const name = match[1];
    const inner = match[2];
    // Unreachable when the overall regex matches, but noUncheckedIndexedAccess
    // types capture groups as possibly-undefined; guard instead of asserting.
    if (whole === undefined || name === undefined || inner === undefined) continue;

    const start = match.index;
    if (start > lastIndex) {
      segments.push({ kind: "text", text: message.slice(lastIndex, start) });
    }
    segments.push({ kind: "tag", name, inner });
    lastIndex = start + whole.length;
  }

  if (lastIndex < message.length) {
    segments.push({ kind: "text", text: message.slice(lastIndex) });
  }
  return segments;
}

export function Trans({ message, components }: TransProps): React.ReactElement {
  const segments = parseTrans(message);

  if (import.meta.env.DEV) {
    // A leftover tag-like token in a text segment means an unclosed or nested
    // tag the parser couldn't pair (it renders as literal text). Surface it in
    // dev so an authoring typo in the message JSON is caught early.
    const stray = segments.some((s) => s.kind === "text" && /<\/?[a-zA-Z]/.test(s.text));
    if (stray) {
      console.error(`Trans: unmatched markup tag in message ${JSON.stringify(message)}`);
    }
  }

  return (
    <>
      {segments.map((segment, i) => {
        // Static message → stable order, so the index is a safe key.
        if (segment.kind === "text") return <Fragment key={i}>{segment.text}</Fragment>;
        const render = components[segment.name];
        return <Fragment key={i}>{render ? render(segment.inner) : segment.inner}</Fragment>;
      })}
    </>
  );
}
