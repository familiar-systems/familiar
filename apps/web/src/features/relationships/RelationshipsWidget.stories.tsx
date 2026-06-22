// Component tests for the relationships widget across its branches: the full row
// list (an entity), the entity empty state, the template affordance, loading, and
// error, plus the "+ add" wiring. The widget takes plain `state` + callbacks
// (spied with fn()), so every branch renders from in-memory data with no socket -
// the connector RelationshipsSection owns the fetch. Branded-id fixtures are cast
// like the TreeID fixtures in TocTree.stories.tsx; PageIds go through pageIdSchema.

import type { CampaignId } from "@familiar-systems/types-app";
import {
  pageIdSchema,
  type RelationshipId,
  type RelationshipView,
} from "@familiar-systems/types-campaign";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn } from "storybook/test";

import { RelationshipsWidget } from "./RelationshipsWidget";

const cid = (s: string): CampaignId => s as CampaignId;
const rid = (s: string): RelationshipId => s as RelationshipId;
const CAMPAIGN = cid("01CAMPAIGN0000000000000000");
const PAGE = pageIdSchema.parse("01ARZ3NDEKTSV4RRFFQ69G5FAV");

// The wireframe's seven rows: prior, session, superseded, another session, two
// GM-only, and a retcon - one of each visual state the widget must render.
const ROWS: RelationshipView[] = [
  {
    id: rid("01R0000000000000000000PRIO"),
    other: { id: PAGE, name: "Grimhollow" },
    predicate: "is a resident of",
    predicate_reverse: "is the home of",
    visibility: "players",
    origin: { kind: "prior" },
    invalidation: null,
  },
  {
    id: rid("01R0000000000000000000KEY0"),
    other: { id: PAGE, name: "North Watchtower" },
    predicate: "keeps the key to",
    predicate_reverse: "is kept by",
    visibility: "players",
    origin: { kind: "session", content: { ordinal: 3 } },
    invalidation: null,
  },
  {
    id: rid("01R0000000000000000000CAPT"),
    other: { id: PAGE, name: "Thren Ferrymen's Guild" },
    predicate: "is captain of",
    predicate_reverse: "is captained by",
    visibility: "players",
    origin: { kind: "session", content: { ordinal: 6 } },
    invalidation: {
      kind: "superseded",
      content: { ended: { kind: "session", content: { ordinal: 12 } } },
    },
  },
  {
    id: rid("01R0000000000000000000SUSP"),
    other: { id: PAGE, name: "Marda" },
    predicate: "is suspicious of",
    predicate_reverse: "is distrusted by",
    visibility: "players",
    origin: { kind: "session", content: { ordinal: 9 } },
    invalidation: null,
  },
  {
    id: rid("01R0000000000000000000DEBT"),
    other: { id: PAGE, name: "Crown of Ash" },
    predicate: "owes a debt to",
    predicate_reverse: "holds marker on",
    visibility: "gm",
    origin: { kind: "session", content: { ordinal: 11 } },
    invalidation: null,
  },
  {
    id: rid("01R0000000000000000000FIRE"),
    other: { id: PAGE, name: "Burning of the North Watch" },
    predicate: "set the signal fire at",
    predicate_reverse: "was started by",
    visibility: "gm",
    origin: { kind: "session", content: { ordinal: 14 } },
    invalidation: null,
  },
  {
    id: rid("01R0000000000000000000BROS"),
    other: { id: PAGE, name: "Tormund" },
    predicate: "is brother to",
    predicate_reverse: "is brother to",
    visibility: "players",
    origin: { kind: "session", content: { ordinal: 2 } },
    invalidation: { kind: "retconned" },
  },
];

const meta = {
  title: "Features/Relationships/RelationshipsWidget",
  component: RelationshipsWidget,
  decorators: [
    (Story) => (
      <div style={{ width: 620, textAlign: "left" }}>
        <Story />
      </div>
    ),
  ],
  args: {
    state: { status: "ready", relationships: ROWS },
    pageKind: "entity",
    campaignId: CAMPAIGN,
    onAdd: fn(),
  },
} satisfies Meta<typeof RelationshipsWidget>;

export default meta;
type Story = StoryObj<typeof meta>;

// The full list: the count badge, and a sampling of the five row states.
export const Ready: Story = {
  play: async ({ canvas }) => {
    await expect(canvas.getByText("7")).toBeInTheDocument();
    await expect(canvas.getByText("is a resident of")).toBeInTheDocument();
    await expect(canvas.getByText("S6 → S12")).toBeInTheDocument();
    await expect(canvas.getByText("S2 ↯")).toBeInTheDocument();
  },
};

// An entity with no relationships yet.
export const Empty: Story = {
  args: { state: { status: "ready", relationships: [] } },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("No relationships yet.")).toBeInTheDocument();
    await expect(canvas.getByText("0")).toBeInTheDocument();
  },
};

// A template shows an affordance, not a list: no count, no "+ add".
export const Template: Story = {
  args: { pageKind: "template", state: { status: "ready", relationships: [] } },
  play: async ({ canvas }) => {
    await expect(
      canvas.getByText("Relationships appear here on entities created from this template."),
    ).toBeInTheDocument();
    await expect(canvas.queryByRole("button", { name: "+ add" })).toBeNull();
  },
};

export const Loading: Story = {
  args: { state: { status: "loading" } },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("Loading relationships...")).toBeInTheDocument();
  },
};

export const Errored: Story = {
  args: { state: { status: "error", message: "Failed to load relationships (503)" } },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("Failed to load relationships (503)")).toBeInTheDocument();
  },
};

// Clicking "+ add" fires onAdd - the seam the create modal hangs off in a later
// slice.
export const Adds: Story = {
  args: { state: { status: "ready", relationships: [] } },
  play: async ({ args, canvas, userEvent }) => {
    await userEvent.click(canvas.getByRole("button", { name: "+ add" }));
    await expect(args.onAdd).toHaveBeenCalled();
  },
};
