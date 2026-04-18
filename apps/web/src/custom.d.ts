import type { DetailedHTMLProps, HTMLAttributes } from "react";

declare global {
  namespace JSX {
    interface IntrinsicElements {
      "hanko-auth": DetailedHTMLProps<
        HTMLAttributes<HTMLElement> & { api?: string },
        HTMLElement
      >;
    }
  }
}

export {};
