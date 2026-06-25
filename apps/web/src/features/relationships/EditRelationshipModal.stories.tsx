// Component tests for the edit-relationship modal. The modal is presentational: it
// takes the row + session list as data and one onSubmit callback (spied with fn()),
// so every edit runs from in-memory data with no socket - useEditRelationship owns
// the real network. The modal portals to document.body, so queries go through
// `screen`. Branded ids are cast like the RelationshipRow fixtures; PageIds go
// through pageIdSchema.

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
    origin: { kind: "session", content: { ordinal: 3 } },
    superseded: null,
    retcon: null,
    knowledge: { kind: "public" },
    ...over,
  };
}

// fn() spies aren't typed as mocks at the prop boundary; this narrows just enough to
// set per-story behavior without importing vitest's Mock type.
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

// Opens born public and ongoing; submit disabled until something changes.
export const Default: Story = {
  play: async () => {
    await expect(screen.getByRole("heading", { name: "Edit relationship" })).toBeInTheDocument();
    await expect(screen.getByRole("radio", { name: /Public/ })).toBeChecked();
    await expect(screen.getByRole("radio", { name: /Ongoing/ })).toBeChecked();
    await expect(screen.getByRole("button", { name: "No changes" })).toBeDisabled();
  },
};

// A born-public row can now be concealed: clicking Hidden flips the knowledge to hidden
// (the secret bit is mutable) and submit is a wholesale knowledge PATCH.
export const ConcealPublicFact: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSubmit).mockResolvedValue(undefined);
    await expect(screen.getByRole("radio", { name: /Public/ })).toBeChecked();
    await userEvent.click(screen.getByRole("radio", { name: /Hidden/ }));
    await userEvent.click(screen.getByRole("button", { name: "Conceal" }));
    await waitFor(() =>
      expect(args.onSubmit).toHaveBeenCalledWith({
        kind: "patch",
        body: { knowledge: { kind: "hidden" }, superseded: null, retcon: null },
      }),
    );
  },
};

// Revealing a secret fact: clicking Revealed defaults to the current session, and the
// knowledge PATCH carries Revealed(S14).
export const RevealSecret: Story = {
  args: { view: view({ knowledge: { kind: "hidden" } }) },
  play: async ({ args, userEvent }) => {
    ctl(args.onSubmit).mockResolvedValue(undefined);
    await expect(screen.getByRole("radio", { name: /Hidden/ })).toBeChecked();
    await userEvent.click(screen.getByRole("radio", { name: /Revealed/ }));
    await userEvent.click(screen.getByRole("button", { name: "Reveal S14" }));
    await waitFor(() =>
      expect(args.onSubmit).toHaveBeenCalledWith({
        kind: "patch",
        body: { knowledge: { kind: "revealed", content: S14 }, superseded: null, retcon: null },
      }),
    );
  },
};

// Ending without a successor is a superseded PATCH; the reveal/retcon axes are null.
export const End: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSubmit).mockResolvedValue(undefined);
    await userEvent.click(screen.getByRole("radio", { name: /Ended/ }));
    await userEvent.click(screen.getByRole("button", { name: "End S14" }));
    await waitFor(() =>
      expect(args.onSubmit).toHaveBeenCalledWith({
        kind: "patch",
        body: { knowledge: null, superseded: { kind: "set", content: S14 }, retcon: null },
      }),
    );
  },
};

// Ending with both successor predicates filled is a supersede POST: the new row is
// born at the end session, carrying the row's (public) knowledge.
export const SupersedeViaSuccessor: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSubmit).mockResolvedValue(undefined);
    await userEvent.click(screen.getByRole("radio", { name: /Ended/ }));
    await userEvent.type(
      screen.getByLabelText("Successor forward predicate"),
      "is quartermaster of",
    );
    await userEvent.type(
      screen.getByLabelText("Successor reverse predicate"),
      "is quartermastered by",
    );
    await userEvent.click(screen.getByRole("button", { name: /End S14.*successor/ }));
    await waitFor(() =>
      expect(args.onSubmit).toHaveBeenCalledWith({
        kind: "supersede",
        body: {
          subject_page_id: SUBJECT,
          other_page_id: OTHER,
          predicate_forward: "is quartermaster of",
          predicate_reverse: "is quartermastered by",
          origin: { kind: "session", content: S14 },
          knowledge: { kind: "public" },
          supersedes: REL_ID,
        },
      }),
    );
  },
};

// An already-ended row can be un-ended (reversible): toggle Ongoing, clearing the
// superseded stamp.
export const UnEnd: Story = {
  args: { view: view({ superseded: { ordinal: 14 } }) },
  play: async ({ args, userEvent }) => {
    ctl(args.onSubmit).mockResolvedValue(undefined);
    await expect(screen.getByRole("radio", { name: /Ended/ })).toBeChecked();
    await userEvent.click(screen.getByRole("radio", { name: /Ongoing/ }));
    await userEvent.click(screen.getByRole("button", { name: "Un-end" }));
    await waitFor(() =>
      expect(args.onSubmit).toHaveBeenCalledWith({
        kind: "patch",
        body: { knowledge: null, superseded: { kind: "clear" }, retcon: null },
      }),
    );
  },
};

// Retcon lives in the corrections drawer; arming it is a retcon PATCH at a session.
export const Retcon: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSubmit).mockResolvedValue(undefined);
    await userEvent.click(screen.getByRole("button", { name: /Corrections/ }));
    await userEvent.click(screen.getByLabelText("Retcon"));
    await userEvent.click(screen.getByRole("button", { name: "Retcon S14" }));
    await waitFor(() =>
      expect(args.onSubmit).toHaveBeenCalledWith({
        kind: "patch",
        body: { knowledge: null, superseded: null, retcon: { kind: "set", content: S14 } },
      }),
    );
  },
};

// An already-retconned row opens with corrections expanded; disarming retcon is a
// clear (un-retcon).
export const UnRetcon: Story = {
  args: { view: view({ retcon: { ordinal: 14 } }) },
  play: async ({ args, userEvent }) => {
    ctl(args.onSubmit).mockResolvedValue(undefined);
    await userEvent.click(screen.getByLabelText("Retcon")); // uncheck (it starts armed)
    await userEvent.click(screen.getByRole("button", { name: "Un-retcon" }));
    await waitFor(() =>
      expect(args.onSubmit).toHaveBeenCalledWith({
        kind: "patch",
        body: { knowledge: null, superseded: null, retcon: { kind: "clear" } },
      }),
    );
  },
};

// A row that is BOTH ended and retconned (the model allows it): un-retconning clears
// only the retcon stamp, never the end. Factuality and correction are independent sums,
// so editing one can't silently clobber the other - a single `mode` discriminant would
// have, emitting a stray `superseded: clear` here.
export const UnRetconKeepsEnd: Story = {
  args: { view: view({ superseded: { ordinal: 14 }, retcon: { ordinal: 14 } }) },
  play: async ({ args, userEvent }) => {
    ctl(args.onSubmit).mockResolvedValue(undefined);
    await userEvent.click(screen.getByLabelText("Retcon")); // starts armed; uncheck it
    await userEvent.click(screen.getByRole("button", { name: "Un-retcon" }));
    await waitFor(() =>
      expect(args.onSubmit).toHaveBeenCalledWith({
        kind: "patch",
        body: { knowledge: null, superseded: null, retcon: { kind: "clear" } },
      }),
    );
  },
};

// Retcon and delete are mutually exclusive (one `correction` sum, not two booleans):
// arming delete clears a previously-armed retcon, so the checkboxes can't both be on.
export const RetconAndDeleteAreExclusive: Story = {
  play: async ({ userEvent }) => {
    await userEvent.click(screen.getByRole("button", { name: /Corrections/ }));
    await userEvent.click(screen.getByLabelText("Retcon"));
    await expect(screen.getByLabelText("Retcon")).toBeChecked();
    await userEvent.click(screen.getByLabelText("Delete"));
    await expect(screen.getByLabelText("Delete")).toBeChecked();
    await expect(screen.getByLabelText("Retcon")).not.toBeChecked();
  },
};

// Delete is the destructive escape hatch in the corrections drawer: a bare DELETE.
export const Delete: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSubmit).mockResolvedValue(undefined);
    await userEvent.click(screen.getByRole("button", { name: /Corrections/ }));
    await userEvent.click(screen.getByLabelText("Delete"));
    await userEvent.click(screen.getByRole("button", { name: "Delete permanently" }));
    await waitFor(() => expect(args.onSubmit).toHaveBeenCalledWith({ kind: "delete" }));
  },
};

// With no sessions, a fact can't be ended or retconned (both need a session); delete
// stays available.
export const NoSessionsGating: Story = {
  args: { sessions: SESSIONS_EMPTY },
  play: async ({ userEvent }) => {
    await expect(screen.getByRole("radio", { name: /Ended/ })).toBeDisabled();
    await expect(screen.getByText(/no sessions yet/i)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Corrections/ }));
    await expect(screen.getByLabelText("Retcon")).toBeDisabled();
    await expect(screen.getByLabelText("Delete")).toBeEnabled();
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
    await userEvent.click(screen.getByRole("button", { name: /Corrections/ }));
    await userEvent.click(screen.getByLabelText("Delete"));
    const del = screen.getByRole("button", { name: "Delete permanently" });
    await userEvent.click(del);
    await userEvent.click(del);
    await expect(args.onSubmit).toHaveBeenCalledTimes(1);
  },
};
