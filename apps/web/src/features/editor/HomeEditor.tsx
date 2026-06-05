// The campaign home-page editor: a TipTap editor bound to a CRDT-synced Loro
// document. Paragraphs and headings only (this slice). The shared schema and
// Loro binding come from @familiar-systems/editor; this component owns the
// React/transport wiring and the on-page chrome.

import { BlockId, LoroExtension, NODE_EXTENSIONS, readPageTitle } from "@familiar-systems/editor";
import type { PageId } from "@familiar-systems/types-campaign";
import { EditorContent, useEditor } from "@tiptap/react";
import type { ContainerID, LoroDoc } from "loro-crdt";

import { roomErrorMessage } from "./loro-manager";
import { usePageDoc } from "./usePageDoc";

interface HomeEditorProps {
  pageId: PageId;
}

export function HomeEditor({ pageId }: HomeEditorProps): React.ReactElement {
  const state = usePageDoc(pageId);

  if (state.status === "error") {
    return (
      <section className="mx-auto w-full max-w-3xl px-8 pt-24">
        <p className="text-sm text-red-700 dark:text-red-400">{roomErrorMessage(state.error)}</p>
      </section>
    );
  }

  if (state.status === "connecting") {
    return (
      <section className="mx-auto w-full max-w-3xl px-8 pt-24">
        <p className="text-sm text-muted-foreground">Opening your campaign...</p>
      </section>
    );
  }

  // synced or reconnecting: keep the editor mounted either way (edits buffer into
  // the local Loro doc and flush when the socket returns); only the indicator
  // differs.
  return (
    <BoundEditor
      doc={state.doc}
      containerId={state.containerId}
      reconnecting={state.status === "reconnecting"}
    />
  );
}

interface BoundEditorProps {
  doc: LoroDoc;
  containerId: ContainerID;
  reconnecting: boolean;
}

// Separate component so `useEditor` runs unconditionally (rules of hooks) and
// only after the doc has synced. The editor is created once per doc.
function BoundEditor({ doc, containerId, reconnecting }: BoundEditorProps): React.ReactElement {
  const title = readPageTitle(doc);
  const editor = useEditor(
    {
      extensions: [...NODE_EXTENSIONS, BlockId, LoroExtension.configure({ doc, containerId })],
      // No `content`: loro-prosemirror builds the document from the synced Loro
      // container. Seeding TipTap content here would race the Loro init.
      editorProps: {
        attributes: { class: "outline-none" },
      },
    },
    [doc],
  );

  return (
    <article className="mx-auto w-full max-w-3xl px-8 pt-16 pb-24">
      {reconnecting ? (
        <p className="mb-4 inline-flex items-center gap-1.5 text-xs text-amber-500">
          <span className="size-1.5 animate-pulse rounded-full bg-amber-500" />
          Reconnecting...
        </p>
      ) : null}
      <h1 className="mb-8 font-display text-3xl font-medium tracking-tight">{title}</h1>
      <EditorContent
        editor={editor}
        className={[
          "font-sans text-base leading-7 text-foreground",
          "[&_.ProseMirror]:min-h-[50vh] [&_.ProseMirror]:outline-none",
          "[&_h1]:font-display [&_h1]:text-3xl [&_h1]:font-semibold [&_h1]:tracking-tight [&_h1]:mt-8 [&_h1]:mb-3",
          "[&_h2]:font-display [&_h2]:text-2xl [&_h2]:font-semibold [&_h2]:mt-6 [&_h2]:mb-2",
          "[&_h3]:font-display [&_h3]:text-xl [&_h3]:font-semibold [&_h3]:mt-5 [&_h3]:mb-2",
          "[&_p]:my-3",
        ].join(" ")}
      />
    </article>
  );
}
