// The campaign home-page editor: a TipTap editor bound to a CRDT-synced Loro
// document. Paragraphs and headings only (this slice). The shared schema and
// Loro binding come from @familiar-systems/editor; this component owns the
// React/transport wiring and the on-page chrome.

import {
  BlockId,
  LoroExtension,
  NODE_EXTENSIONS,
  readPageTitle,
  writePageTitle,
} from "@familiar-systems/editor";
import type { PageId } from "@familiar-systems/types-campaign";
import { EditorContent, useEditor } from "@tiptap/react";
import type { ContainerID, LoroDoc } from "loro-crdt";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";

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
    // Key by page so the title draft state (below) resets on navigation rather
    // than briefly showing the previous page's title.
    <BoundEditor
      key={pageId}
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

// Reactive read of the Page title from its `meta.title` LWW string. Subscribes
// to the whole doc (cheap: getSnapshot returns the same string when the title is
// unchanged, so body edits don't re-render the title), so a remote rename shows
// up live, not just our own edits.
function usePageTitle(doc: LoroDoc): string {
  const subscribe = useCallback((onChange: () => void) => doc.subscribe(() => onChange()), [doc]);
  const getSnapshot = useCallback(() => readPageTitle(doc), [doc]);
  return useSyncExternalStore(subscribe, getSnapshot);
}

// Separate component so `useEditor` runs unconditionally (rules of hooks) and
// only after the doc has synced. The editor is created once per doc.
function BoundEditor({ doc, containerId, reconnecting }: BoundEditorProps): React.ReactElement {
  const committedTitle = usePageTitle(doc);
  const titleRef = useRef<HTMLTextAreaElement>(null);
  // The title field is a draft over the committed Loro title. It may be empty
  // mid-edit, but an empty/whitespace title is never committed and reverts on
  // blur, so a Page always keeps a non-empty title (matching the create path).
  const [draft, setDraft] = useState(committedTitle);
  const editingRef = useRef(false);
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

  // Mirror committed changes (our own commits and remote renames) into the draft
  // while not actively editing, so a remote rename shows up but never clobbers
  // in-progress typing.
  useEffect(() => {
    if (!editingRef.current) setDraft(committedTitle);
  }, [committedTitle]);

  // Auto-grow the title to fit wrapped lines (no inner scrollbar).
  useLayoutEffect(() => {
    const el = titleRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [draft]);

  return (
    <article className="mx-auto w-full max-w-3xl px-8 pt-16 pb-24">
      {reconnecting ? (
        <p className="mb-4 inline-flex items-center gap-1.5 text-xs text-amber-500">
          <span className="size-1.5 animate-pulse rounded-full bg-amber-500" />
          Reconnecting...
        </p>
      ) : null}
      {/* The title is a single logical line that soft-wraps (a textarea, not an
          input, so long fantasy nouns wrap rather than truncate). Newlines are
          stripped and Enter jumps to the body, so it never holds a literal `\n`,
          matching the `meta.title` LWW-string model. */}
      <textarea
        ref={titleRef}
        value={draft}
        onFocus={() => {
          editingRef.current = true;
        }}
        onChange={(e) => {
          const next = e.target.value.replace(/\n/g, "");
          setDraft(next);
          // Never commit an empty title; the field can show empty while editing,
          // but it reverts on blur (below).
          if (next.trim() !== "") writePageTitle(doc, next);
        }}
        onBlur={() => {
          editingRef.current = false;
          const trimmed = draft.trim();
          if (trimmed === "") {
            setDraft(committedTitle); // leaving it empty reverts to the last title
          } else if (trimmed !== committedTitle) {
            writePageTitle(doc, trimmed); // normalize surrounding whitespace
            setDraft(trimmed);
          }
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            editor?.commands.focus("start");
          }
        }}
        rows={1}
        placeholder="Untitled"
        aria-label="Page title"
        spellCheck={false}
        className="mb-8 w-full resize-none overflow-hidden border-0 bg-transparent p-0 font-display text-3xl font-medium tracking-tight outline-none placeholder:text-muted-foreground/40"
      />
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
