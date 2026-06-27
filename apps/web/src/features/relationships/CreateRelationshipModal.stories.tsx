// Component tests for the create-relationship modal. The modal is presentational:
// it takes the predicate/session data plus search/create/submit as callbacks
// (spied with fn()), so every flow runs from in-memory data with no socket - the
// connector useCreateRelationship owns the real network. The modal portals to
// document.body, so queries go through `screen`, not the story canvas. Branded ids
// are cast like the relationshipDisplay fixtures; PageIds go through pageIdSchema.

import {
  type EntitySearchResult,
  pageIdSchema,
  type PredicatePairView,
  type SessionId,
  type SessionsResponse,
} from "@familiar-systems/types-campaign";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, screen, waitFor } from "storybook/test";

import { CreateRelationshipModal } from "./CreateRelationshipModal";

const sid = (s: string): SessionId => s as SessionId;
const SUBJECT = pageIdSchema.parse("01ARZ3NDEKTSV4RRFFQ69G5FAV");
const GRIMHOLLOW = pageIdSchema.parse("01ARZ3NDEKTSV4RRFFQ69G5FB0");
const MARDA = pageIdSchema.parse("01ARZ3NDEKTSV4RRFFQ69G5FB1");
const NEW_ID = pageIdSchema.parse("01ARZ3NDEKTSV4RRFFQ69G5FB2");
const CURRENT_SID = sid("01ARZ3NDEKTSV4RRFFQ69G5S14");

const PREDICATES: PredicatePairView[] = [
  { forward: "is a resident of", reverse: "is the home of", count: 42 },
  { forward: "is suspicious of", reverse: "is distrusted by", count: 18 },
  { forward: "is captain of", reverse: "is captained by", count: 9 },
  { forward: "is brother to", reverse: "is brother to", count: 7 },
];

const SESSIONS: SessionsResponse = {
  sessions: [
    { id: sid("01ARZ3NDEKTSV4RRFFQ69G5S03"), ordinal: 3 },
    { id: CURRENT_SID, ordinal: 14 },
  ],
  current: { id: CURRENT_SID, ordinal: 14 },
};
const SESSIONS_EMPTY: SessionsResponse = { sessions: [], current: null };

const SAMPLE_ENTITIES: EntitySearchResult[] = [
  { id: GRIMHOLLOW, name: "Grimhollow" },
  { id: MARDA, name: "Marda" },
];
// Search results that include the subject itself, to prove the self-edge filter.
const SELF_INCLUSIVE: EntitySearchResult[] = [
  { id: SUBJECT, name: "Wren Aldwater" },
  { id: GRIMHOLLOW, name: "Grimhollow" },
];

// fn() spies aren't typed as mocks at the prop boundary; this narrows just enough
// to set per-story behavior without importing vitest's Mock type.
interface MockCtl {
  mockResolvedValue: (v: unknown) => void;
  mockRejectedValue: (e: unknown) => void;
  mockImplementation: (impl: (...a: never[]) => unknown) => void;
}
const ctl = (f: unknown): MockCtl => f as MockCtl;

const meta = {
  title: "Features/Relationships/CreateRelationshipModal",
  component: CreateRelationshipModal,
  args: {
    subjectName: "Wren Aldwater",
    subjectPageId: SUBJECT,
    predicates: PREDICATES,
    sessions: SESSIONS,
    onSearchEntities: fn(async () => SAMPLE_ENTITIES),
    onCreateEntity: fn(async (name: string) => ({ id: NEW_ID, name })),
    onSubmit: fn(async () => {}),
    onClose: fn(),
  },
} satisfies Meta<typeof CreateRelationshipModal>;

export default meta;
type Story = StoryObj<typeof meta>;

// Pick Grimhollow as the object and a known predicate, leaving a submittable form.
async function fillValidForm(
  userEvent: {
    click: (el: Element) => Promise<void>;
    type: (el: Element, text: string) => Promise<void>;
    keyboard: (text: string) => Promise<void>;
  },
  args: { onSearchEntities: unknown },
): Promise<void> {
  ctl(args.onSearchEntities).mockResolvedValue(SAMPLE_ENTITIES);
  await userEvent.type(screen.getByLabelText("Search entities"), "grim");
  await userEvent.click(await screen.findByRole("option", { name: /Grimhollow/ }));
  await userEvent.type(screen.getByLabelText("Predicate"), "is a resident of");
  await userEvent.keyboard("{Escape}"); // close the predicate dropdown
}

// The subject is fixed to the current entity (a divergence from the wireframe,
// where it is editable); the form opens unsubmittable, born public by default.
export const Default: Story = {
  play: async () => {
    await expect(screen.getByText("Wren Aldwater")).toBeInTheDocument();
    await expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
    await expect(screen.getByRole("radio", { name: /Public/ })).toBeChecked();
    await expect((screen.getByLabelText("As of") as HTMLSelectElement).value).toBe(CURRENT_SID);
  },
};

// With no sessions yet, the as-of picker offers only Prior.
export const NoSessionsOffersPriorOnly: Story = {
  args: { sessions: SESSIONS_EMPTY },
  play: async () => {
    const asOf = screen.getByLabelText("As of") as HTMLSelectElement;
    await expect(asOf.value).toBe("prior");
    await expect(asOf.querySelectorAll("option")).toHaveLength(1);
  },
};

// Typing the object queries the server (callback) and renders the results; picking
// one commits it as a chip.
export const ObjectSearch: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSearchEntities).mockResolvedValue(SAMPLE_ENTITIES);
    await userEvent.type(screen.getByLabelText("Search entities"), "ma");
    await expect(args.onSearchEntities).toHaveBeenCalledWith("ma");
    await userEvent.click(await screen.findByRole("option", { name: /Marda/ }));
    await expect(screen.queryByLabelText("Search entities")).toBeNull();
    await expect(screen.getByText("Marda")).toBeInTheDocument();
  },
};

// The subject can't be its own object: it's filtered out of the results.
export const SelfEdgeFiltered: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSearchEntities).mockResolvedValue(SELF_INCLUSIVE);
    await userEvent.type(screen.getByLabelText("Search entities"), "w");
    await screen.findByRole("option", { name: /Grimhollow/ });
    await expect(screen.queryByRole("option", { name: "Wren Aldwater" })).toBeNull();
  },
};

// A new thing is minted on submit (not on selection): create the entity, then post
// the relationship pointing at the freshly-minted id.
export const CreateNewEntity: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSearchEntities).mockResolvedValue([]);
    ctl(args.onCreateEntity).mockResolvedValue({ id: NEW_ID, name: "Tormund" });
    ctl(args.onSubmit).mockResolvedValue(undefined);
    await userEvent.type(screen.getByLabelText("Search entities"), "Tormund");
    await userEvent.click(await screen.findByRole("option", { name: /Create Tormund/ }));
    await expect(screen.getByText("new")).toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Predicate"), "is brother to");
    await userEvent.keyboard("{Escape}");
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    await waitFor(() => expect(args.onCreateEntity).toHaveBeenCalledWith("Tormund"));
    await expect(args.onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        other_page_id: NEW_ID,
        predicate_forward: "is brother to",
        predicate_reverse: "is brother to",
        supersedes: null,
      }),
    );
  },
};

// Choosing a known predicate autofills its reverse from the graph.
export const ReverseAutofillFromGraph: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSearchEntities).mockResolvedValue(SAMPLE_ENTITIES);
    await userEvent.type(screen.getByLabelText("Search entities"), "grim");
    await userEvent.click(await screen.findByRole("option", { name: /Grimhollow/ }));
    // Typing a known forward predicate autofills its reverse from the graph.
    await userEvent.type(screen.getByLabelText("Predicate"), "is a resident of");
    await expect((screen.getByLabelText("Reverse predicate") as HTMLInputElement).value).toBe(
      "is the home of",
    );
    await expect(screen.getByText("from graph")).toBeInTheDocument();
  },
};

// Reverse-autofill checks both pair directions: a predicate stored in the reverse
// slot still resolves its partner (canonicalization).
export const ReverseCanonicalization: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSearchEntities).mockResolvedValue(SAMPLE_ENTITIES);
    await userEvent.type(screen.getByLabelText("Search entities"), "grim");
    await userEvent.click(await screen.findByRole("option", { name: /Grimhollow/ }));
    await userEvent.type(screen.getByLabelText("Predicate"), "is the home of");
    await expect((screen.getByLabelText("Reverse predicate") as HTMLInputElement).value).toBe(
      "is a resident of",
    );
  },
};

// A custom predicate has no known reverse, and the reverse is required (a
// divergence from the wireframe): the submit stays disabled until it's filled. The
// custom forward is kept verbatim by the ComboBox's allowsCustomValue.
export const CustomPredicateRequiresReverse: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSearchEntities).mockResolvedValue(SAMPLE_ENTITIES);
    await userEvent.type(screen.getByLabelText("Search entities"), "grim");
    await userEvent.click(await screen.findByRole("option", { name: /Grimhollow/ }));
    await userEvent.type(screen.getByLabelText("Predicate"), "befriended");
    await userEvent.keyboard("{Escape}"); // close the (no-match) dropdown
    const reverse = screen.getByLabelText("Reverse predicate") as HTMLInputElement;
    await expect(reverse.value).toBe("");
    await expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
    await userEvent.type(reverse, "was befriended by");
    await expect(screen.getByRole("button", { name: "Create" })).toBeEnabled();
  },
};

// Knowledge defaults to Public; clicking Hidden marks the new fact secret (GM-only).
// Create has no reveal control - the Public segment carries no session, and revealing a
// secret fact is an edit, not a create.
export const BornSecret: Story = {
  play: async ({ userEvent }) => {
    await expect(screen.getByRole("radio", { name: /Public/ })).toBeChecked();
    await userEvent.click(screen.getByRole("radio", { name: /Hidden/ }));
    await expect(screen.getByRole("radio", { name: /Hidden/ })).toBeChecked();
    await expect(screen.queryByLabelText("Reveal session")).toBeNull();
  },
};

// A complete form submits the full request, born public by default, originating in
// the current session, never a supersession.
export const SubmitSuccess: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSubmit).mockResolvedValue(undefined);
    await fillValidForm(userEvent, args);
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    await waitFor(() =>
      expect(args.onSubmit).toHaveBeenCalledWith(
        expect.objectContaining({
          subject_page_id: SUBJECT,
          other_page_id: GRIMHOLLOW,
          predicate_forward: "is a resident of",
          predicate_reverse: "is the home of",
          knowledge: { kind: "public" },
          origin: { kind: "session", content: CURRENT_SID },
          supersedes: null,
        }),
      ),
    );
  },
};

// A failed submit surfaces the error and leaves the modal open and re-submittable.
export const SubmitError: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSubmit).mockRejectedValue(new Error("A live relationship already exists."));
    await fillValidForm(userEvent, args);
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    await screen.findByText("A live relationship already exists.");
    await expect(screen.getByRole("button", { name: "Create" })).toBeEnabled();
  },
};

// Double-submit is guarded twice: the button disables the instant a submit is in
// flight (so a second click can't fire), backed by a synchronous ref guard for the
// same-tick race. One submit reaches the server.
export const DoubleSubmitGuarded: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSubmit).mockImplementation(() => new Promise(() => {})); // never settles
    await fillValidForm(userEvent, args);
    const create = screen.getByRole("button", { name: "Create" });
    await userEvent.click(create);
    await expect(create).toBeDisabled();
    await expect(args.onSubmit).toHaveBeenCalledTimes(1);
  },
};

// Arrow keys move the highlight and Enter commits it (React Aria owns the index
// math now; ArrowDown lands on the first, most-used option).
export const PredicateKeyboardNav: Story = {
  play: async ({ userEvent }) => {
    const predicate = screen.getByLabelText("Predicate");
    await userEvent.type(predicate, "is ");
    // Option accessible names include the usage count, so match on a regex.
    await screen.findByRole("option", { name: /is a resident of/ });
    await userEvent.keyboard("{ArrowDown}{Enter}");
    await expect((predicate as HTMLInputElement).value).toBe("is a resident of");
  },
};

// Escape closes an open dropdown first, then the dialog.
export const EscapeClosesDropdownThenDialog: Story = {
  play: async ({ args, userEvent }) => {
    ctl(args.onSearchEntities).mockResolvedValue(SAMPLE_ENTITIES);
    await userEvent.type(screen.getByLabelText("Search entities"), "grim");
    await screen.findByRole("option", { name: /Grimhollow/ });
    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("option", { name: /Grimhollow/ })).toBeNull());
    await expect(args.onClose).not.toHaveBeenCalled();
    await userEvent.keyboard("{Escape}");
    await expect(args.onClose).toHaveBeenCalled();
  },
};
