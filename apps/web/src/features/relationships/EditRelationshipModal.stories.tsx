// Component tests for the edit-relationship modal. The modal is presentational: it
// takes the row + session list as data and one onSubmit callback (spied with fn()),
// so every op runs from in-memory data with no socket - useEditRelationship owns the
// real network. The modal portals to document.body, so queries go through `screen`.
// Branded ids are cast like the RelationshipRow fixtures; PageIds go through
// pageIdSchema.

import {
  pageIdSchema,
  type RelationshipId,
  type RelationshipView,
  type SessionId,
  type SessionsResponse,
} from "@familiar-systems/types-campaign";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, screen, waitFor } from "storybook/test";

import { EditRelationshipModal } from "./EditRelationshipModal";

const rid = (s: string): RelationshipId => s as RelationshipId;
const sid = (s: string): SessionId => s as SessionId;

const REL_ID = rid("01TEST00000000000000000REL");
const SUBJECT = pageIdSchema.parse("01ARZ3NDEKTSV4RRFFQ69G5FAV");
const OTHER = pageIdSchema.parse("01ARZ3NDEKTSV4RRFFQ69G5FB0");
const S3 = sid("01ARZ3NDEKTSV4RRFFQ69G5S03");
const S14 = sid("01ARZ3NDEKTSV4RRFFQ69G5S14");

const SESSIONS: SessionsResponse = {
  sessions: [
    { id: S3, ordinal: 3 },
    { id: S14, ordinal: 14 },
  ],
  current: { id: S14, ordinal: 14 },
};
const SESSIONS_EMPTY: SessionsResponse = { sessions: [], current: null };

function view(over: Partial<RelationshipView>): RelationshipView {
  return {
    id: REL_ID,
    other: { id: OTHER, name: "Thren Ferrymen's Guild" },
    predicate: "is captain of",
    predicate_reverse: "is captained by",
    visibility: "gm",
    origin: { kind: "session", content: { ordinal: 3 } },
    invalidation: null,
    ...over,
  };
}

// fn() spies aren't typed as mocks at the prop boundary; this narrows just enough
// to set per-story behavior without importing vitest's Mock type.
interface MockCtl {
  mockResolvedValue: (v: unknown) => void;
  mockImplementation: (impl: (...a: never[]) => unknown) => void;
}
const ctl = (f: unknown): MockCtl => f as MockCtl;

const meta = {
  title: "Features/Relationships/EditRelationshipModal",
  component: EditRelationshipModal,
  args: {
    subjectName: "Wren Aldwater",
    subjectPageId: SUBJECT,
    view: view({}),
    sessions: SESSIONS,
    onSubmit: fn(async () => {}),
    onClose: fn(),
  },
} satisfies Meta<typeof EditRelationshipModal>;

export default meta;
type Story = StoryObj<typeof meta>;

// Opens on Supersede (the default), the visibility toggle pre-set to the row's GM,
// and submit disabled until something changes.
export const Default: Story = {
  play: async () => {
    await expect(screen.getByRole("heading", { name: "Edit relationship" })).toBeInTheDocument();
    await expect(screen.getByRole("radio", { name: /Supersede/ })).toBeChecked();
    await expect(screen.getByRole("radio", { name: /GM only/ })).toBeChecked();
    await expect(screen.getByRole("button", { name: /Supersede/ })).toBeDisabled();
  },
};

// Editing a predicate is a supersession: it posts a new row pointing back at the old
// one, born at the chosen session, carrying the (unchanged here) visibility.
export const Supersede: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSubmit).mockResolvedValue(undefined);
    const forward = screen.getByLabelText("Forward predicate");
    await userEvent.clear(forward);
    await userEvent.type(forward, "is admiral of");
    await userEvent.click(screen.getByRole("button", { name: /Supersede/ }));
    await waitFor(() =>
      expect(args.onSubmit).toHaveBeenCalledWith({
        kind: "supersede",
        body: {
          subject_page_id: SUBJECT,
          other_page_id: OTHER,
          predicate_forward: "is admiral of",
          predicate_reverse: "is captained by",
          visibility: "gm",
          origin: { kind: "session", content: S14 },
          supersedes: REL_ID,
        },
      }),
    );
  },
};

// Ending invalidates the row at a session, with no replacement. Visibility was not
// touched, so the patch carries it as null.
export const End: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSubmit).mockResolvedValue(undefined);
    await userEvent.click(screen.getByRole("radio", { name: /End/ }));
    await userEvent.click(screen.getByRole("button", { name: /End to S14/ }));
    await waitFor(() =>
      expect(args.onSubmit).toHaveBeenCalledWith({
        kind: "patch",
        body: { invalidation: { reason: "superseded", as_of: S14 }, visibility: null },
      }),
    );
  },
};

// Retcon invalidates as "never true", timeless (no as-of).
export const Retcon: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSubmit).mockResolvedValue(undefined);
    await userEvent.click(screen.getByRole("radio", { name: /Retcon/ }));
    await userEvent.click(screen.getByRole("button", { name: "Retcon" }));
    await waitFor(() =>
      expect(args.onSubmit).toHaveBeenCalledWith({
        kind: "patch",
        body: { invalidation: { reason: "retconned", as_of: null }, visibility: null },
      }),
    );
  },
};

// Delete is the only destructive op: a bare DELETE, no body.
export const Delete: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSubmit).mockResolvedValue(undefined);
    await userEvent.click(screen.getByRole("radio", { name: /Delete/ }));
    await userEvent.click(screen.getByRole("button", { name: "Delete permanently" }));
    await waitFor(() => expect(args.onSubmit).toHaveBeenCalledWith({ kind: "delete" }));
  },
};

// Flipping only the visibility (no predicate edit, Supersede still selected) is a
// plain visibility PATCH, labelled "Update visibility".
export const VisibilityOnly: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSubmit).mockResolvedValue(undefined);
    await userEvent.click(screen.getByRole("radio", { name: /Players/ }));
    await userEvent.click(screen.getByRole("button", { name: "Update visibility" }));
    await waitFor(() =>
      expect(args.onSubmit).toHaveBeenCalledWith({
        kind: "patch",
        body: { visibility: "players", invalidation: null },
      }),
    );
  },
};

// A lifecycle op and a visibility change fold into one PATCH carrying both.
export const EndAndVisibility: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSubmit).mockResolvedValue(undefined);
    await userEvent.click(screen.getByRole("radio", { name: /End/ }));
    await userEvent.click(screen.getByRole("radio", { name: /Players/ }));
    await userEvent.click(screen.getByRole("button", { name: /End to S14/ }));
    await waitFor(() =>
      expect(args.onSubmit).toHaveBeenCalledWith({
        kind: "patch",
        body: { invalidation: { reason: "superseded", as_of: S14 }, visibility: "players" },
      }),
    );
  },
};

// No sessions: end and supersede need one, so their cards are disabled; retcon and
// delete don't. Submit stays disabled until visibility changes, then becomes a
// visibility-only update.
export const NoSessionsGating: Story = {
  args: { sessions: SESSIONS_EMPTY },
  play: async ({ userEvent }) => {
    await expect(screen.getByRole("radio", { name: /Supersede/ })).toBeDisabled();
    await expect(screen.getByRole("radio", { name: /End/ })).toBeDisabled();
    await expect(screen.getByRole("radio", { name: /Retcon/ })).toBeEnabled();
    await expect(screen.getByRole("radio", { name: /Delete/ })).toBeEnabled();
    await expect(screen.getByText(/no sessions yet/i)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("radio", { name: /Players/ }));
    await expect(screen.getByRole("button", { name: "Update visibility" })).toBeEnabled();
  },
};

// An already-invalidated row can't be re-invalidated: end / supersede / retcon are
// disabled, only delete (and a visibility change) remain.
export const AlreadyInvalidatedGating: Story = {
  args: {
    view: view({
      invalidation: {
        kind: "superseded",
        content: { ended: { kind: "session", content: { ordinal: 12 } } },
      },
    }),
  },
  play: async () => {
    await expect(screen.getByRole("radio", { name: /Supersede/ })).toBeDisabled();
    await expect(screen.getByRole("radio", { name: /End/ })).toBeDisabled();
    await expect(screen.getByRole("radio", { name: /Retcon/ })).toBeDisabled();
    await expect(screen.getByRole("radio", { name: /Delete/ })).toBeEnabled();
    await expect(screen.getByText(/already invalidated/i)).toBeInTheDocument();
  },
};

// Escape dismisses the dialog.
export const EscapeCloses: Story = {
  play: async ({ args, userEvent }) => {
    await userEvent.keyboard("{Escape}");
    await expect(args.onClose).toHaveBeenCalled();
  },
};

// The ref guard collapses a rapid double-trigger to a single submit.
export const DoubleSubmitGuarded: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSubmit).mockImplementation(() => new Promise(() => {})); // never settles
    await userEvent.click(screen.getByRole("radio", { name: /Delete/ }));
    const del = screen.getByRole("button", { name: "Delete permanently" });
    await userEvent.click(del);
    await userEvent.click(del);
    await expect(args.onSubmit).toHaveBeenCalledTimes(1);
  },
};
