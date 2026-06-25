// The campaign home-page editor: a TipTap editor bound to a CRDT-synced Loro
// document. Paragraphs and headings only (this slice). The shared schema and
// Loro binding come from @familiar-systems/editor; this component owns the
// React/transport wiring and the on-page chrome.

import {
  BlockId,
  bodyContainerId,
  LoroExtension,
  NODE_EXTENSIONS,
  preambleContainerId,
  readPageTitle,
  writePageTitle,
} from "@familiar-systems/editor";
import type { CampaignId } from "@familiar-systems/types-app";
import type { PageId, TocPageKind } from "@familiar-systems/types-campaign";
import { EditorContent, useEditor } from "@tiptap/react";
import { type LoroDoc, UndoManager } from "loro-crdt";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";

import { RelationshipsSection } from "../relationships/RelationshipsSection";
import { usePageKind, usePagePrefix } from "../toc/useToc";
import { roomErrorMessage } from "./loro-manager";
import { usePageDoc } from "./usePageDoc";

// Shared per-section editor styling (the heading/paragraph rhythm). Each section
// adds its own `.ProseMirror` min-height: the preamble is a compact index card,
// the body fills the page.
const SECTION_EDITOR_CLASS = [
  "font-sans text-base leading-7 text-foreground",
  "[&_.ProseMirror]:outline-none",
  "[&_h1]:font-display [&_h1]:text-3xl [&_h1]:font-semibold [&_h1]:tracking-tight [&_h1]:mt-8 [&_h1]:mb-3",
  "[&_h2]:font-display [&_h2]:text-2xl [&_h2]:font-semibold [&_h2]:mt-6 [&_h2]:mb-2",
  "[&_h3]:font-display [&_h3]:text-xl [&_h3]:font-semibold [&_h3]:mt-5 [&_h3]:mb-2",
  "[&_p]:my-3",
].join(" ");

interface HomeEditorProps {
  campaignId: CampaignId;
  pageId: PageId;
}

export function HomeEditor({ campaignId, pageId }: HomeEditorProps): React.ReactElement {
  const state = usePageDoc(pageId);
  // The immutable kind/ordinal prefix ("Session 3:", "Template:") comes from the
  // ToC entry; the editable name comes from the live page doc (below). Null for
  // an entity or before the page appears in the synced ToC.
  const prefix = usePagePrefix(pageId);
  // Same source as the prefix: the relationships widget renders only for entities
  // and templates, the two kinds with the preamble/body layout.
  const pageKind = usePageKind(pageId);

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
      campaignId={campaignId}
      pageId={pageId}
      pageKind={pageKind}
      prefix={prefix}
      reconnecting={state.status === "reconnecting"}
    />
  );
}

interface BoundEditorProps {
  doc: LoroDoc;
  campaignId: CampaignId;
  pageId: PageId;
  /** The page's kind, or null before it appears in the synced ToC. */
  pageKind: TocPageKind | null;
  /** Non-editable kind/ordinal prefix shown before the editable title, or null. */
  prefix: string | null;
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
// only after the doc has synced. The editors are created once per doc. The page
// has two section containers (preamble + body); each binds its own editor to the
// same doc, separated by a horizontal bar.
function BoundEditor({
  doc,
  campaignId,
  pageId,
  pageKind,
  prefix,
  reconnecting,
}: BoundEditorProps): React.ReactElement {
  const committedTitle = usePageTitle(doc);
  const titleRef = useRef<HTMLTextAreaElement>(null);
  // The title field is a draft over the committed Loro title. It may be empty
  // mid-edit, but an empty/whitespace title is never committed and reverts on
  // blur, so a Page always keeps a non-empty title (matching the create path).
  const [draft, setDraft] = useState(committedTitle);
  const editingRef = useRef(false);

  // One UndoManager shared by both section editors gives unified page-level undo
  // (a single Ctrl-Z stack across preamble + body), the multi-section design's
  // "Now" behavior. Known rough edge accepted for now: loro-prosemirror registers
  // cursor-restore per plugin instance, so after a cross-section undo the caret
  // can land in the wrong section. Undo itself is correct; only caret placement
  // is rough (fixed by the deferred per-section undo work).
  const undoManager = useMemo(() => new UndoManager(doc, {}), [doc]);

  // No `content` on either editor: loro-prosemirror builds each section from its
  // Loro container. Seeding TipTap content here would race the Loro init.
  const preambleEditor = useEditor(
    {
      extensions: [
        ...NODE_EXTENSIONS,
        BlockId,
        LoroExtension.configure({ doc, containerId: preambleContainerId(doc), undoManager }),
      ],
      editorProps: { attributes: { class: "outline-none" } },
    },
    [doc],
  );
  const bodyEditor = useEditor(
    {
      extensions: [
        ...NODE_EXTENSIONS,
        BlockId,
        LoroExtension.configure({ doc, containerId: bodyContainerId(doc), undoManager }),
      ],
      editorProps: { attributes: { class: "outline-none" } },
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
      {/* The title row: an optional non-editable kind/ordinal prefix ("Session
          3:", "Template:") sits inline before the editable name, so the prefix
          never enters the `meta.title` value. The title itself is a single
          logical line that soft-wraps (a textarea, not an input, so long fantasy
          nouns wrap rather than truncate); newlines are stripped and Enter jumps
          to the body, so it never holds a literal `\n`, matching the `meta.title`
          LWW-string model. */}
      <div className="mb-8 flex items-baseline gap-2">
        {prefix !== null ? (
          <span className="shrink-0 font-display text-3xl font-medium tracking-tight text-muted-foreground/70">
            {prefix}
          </span>
        ) : null}
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
              preambleEditor?.commands.focus("start");
            }
          }}
          rows={1}
          // A prefixed page (template/session) carries its own label, so an empty
          // name needs no "Untitled" filler; an entity keeps it.
          placeholder={prefix === null ? "Untitled" : ""}
          aria-label="Page title"
          spellCheck={false}
          className="min-w-0 flex-1 resize-none overflow-hidden border-0 bg-transparent p-0 font-display text-3xl font-medium tracking-tight outline-none placeholder:text-muted-foreground/40"
        />
      </div>
      {/* Preamble: the bounded "index card" section. */}
      <div data-testid="preamble-editor">
        <EditorContent
          editor={preambleEditor}
          className={`${SECTION_EDITOR_CLASS} [&_.ProseMirror]:min-h-12`}
        />
      </div>
      {/* Relationships sit at the preamble/body seam, for the two kinds with this
          layout. Server-authoritative (REST), not collaborative CRDT content. */}
      {pageKind?.kind === "entity" || pageKind?.kind === "template" ? (
        <RelationshipsSection
          campaignId={campaignId}
          pageId={pageId}
          pageKind={pageKind.kind}
          subjectName={committedTitle}
        />
      ) : null}
      {/* Horizontal bar separating the preamble from the freeform body. */}
      <div className="my-6 border-b border-foreground/10" />
      {/* Body: the freeform section. */}
      <div data-testid="body-editor">
        <EditorContent
          editor={bodyEditor}
          className={`${SECTION_EDITOR_CLASS} [&_.ProseMirror]:min-h-[50vh]`}
        />
      </div>
    </article>
  );
}
