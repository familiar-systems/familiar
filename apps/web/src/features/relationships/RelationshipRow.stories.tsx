// Component tests for one relationship row across its five visual states, plus the
// onSelect wiring (a real click that no pure test reaches). The row takes a plain
// RelationshipView + callbacks (spied with fn()), so it renders from in-memory
// data with no socket. Branded-id fixtures are cast like the TreeID fixtures in
// TocTree.stories.tsx (no runtime guard exists for these brands); PageIds go
// through pageIdSchema, which does.

import type { CampaignId } from "@familiar-systems/types-app";
import {
  pageIdSchema,
  type RelationshipId,
  type RelationshipView,
} from "@familiar-systems/types-campaign";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn } from "storybook/test";

import { RelationshipRow } from "./RelationshipRow";

const cid = (s: string): CampaignId => s as CampaignId;
const rid = (s: string): RelationshipId => s as RelationshipId;
const CAMPAIGN = cid("01CAMPAIGN0000000000000000");

const GRIMHOLLOW = pageIdSchema.parse("01ARZ3NDEKTSV4RRFFQ69G5FAV");
const WATCHTOWER = pageIdSchema.parse("01BX5ZZKBKACTAV9WEVGEMMVRY");
const GUILD = pageIdSchema.parse("01CSGZ8M4Q9N7P2K3J5H6T8WXR");
const CROWN = pageIdSchema.parse("01D78XYFJ1E2K3M4N5P6Q7R8S9");
const TORMUND = pageIdSchema.parse("01EM8K2P9XQ4R7T3V6W1Y5Z0AB");

function view(over: Partial<RelationshipView>): RelationshipView {
  return {
    id: rid("01TEST00000000000000000REL"),
    other: { id: WATCHTOWER, name: "North Watchtower" },
    predicate: "keeps the key to",
    predicate_reverse: "is kept by",
    visibility: "players",
    origin: { kind: "session", content: { ordinal: 3 } },
    invalidation: null,
    ...over,
  };
}

const meta = {
  title: "Features/Relationships/RelationshipRow",
  component: RelationshipRow,
  decorators: [
    (Story) => (
      <div style={{ width: 560, textAlign: "left" }}>
        <Story />
      </div>
    ),
  ],
  args: {
    view: view({}),
    campaignId: CAMPAIGN,
    onSelect: fn(),
  },
} satisfies Meta<typeof RelationshipRow>;

export default meta;
type Story = StoryObj<typeof meta>;

// A live, player-visible fact: the predicate, the linked entity, a plain origin.
export const Live: Story = {
  play: async ({ canvas }) => {
    await expect(canvas.getByText("keeps the key to")).toBeInTheDocument();
    await expect(canvas.getByText("North Watchtower")).toBeInTheDocument();
    await expect(canvas.getByText("Session 3")).toBeInTheDocument();
  },
};

// A fact true before the campaign began: the origin reads "Prior".
export const Prior: Story = {
  args: {
    view: view({
      predicate: "is a resident of",
      other: { id: GRIMHOLLOW, name: "Grimhollow" },
      origin: { kind: "prior" },
    }),
  },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("Prior")).toBeInTheDocument();
  },
};

// Superseded: faded, a "S6 → S12" span, and the history glyph in the gutter.
export const Superseded: Story = {
  args: {
    view: view({
      predicate: "is captain of",
      other: { id: GUILD, name: "Thren Ferrymen's Guild" },
      origin: { kind: "session", content: { ordinal: 6 } },
      invalidation: {
        kind: "superseded",
        content: { ended: { kind: "session", content: { ordinal: 12 } } },
      },
    }),
  },
  play: async ({ canvas, canvasElement }) => {
    await expect(canvas.getByText("S6 → S12")).toBeInTheDocument();
    await expect(canvasElement.querySelector(".lucide-history")).toBeInTheDocument();
  },
};

// GM-only: the plum wash and the eye glyph mark a fact the players can't see.
export const GmOnly: Story = {
  args: {
    view: view({
      predicate: "owes a debt to",
      other: { id: CROWN, name: "Crown of Ash" },
      visibility: "gm",
      origin: { kind: "session", content: { ordinal: 11 } },
    }),
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector(".lucide-eye-off")).toBeInTheDocument();
  },
};

// Retconned: struck through, a "S2 ↯" origin, and the X glyph.
export const Retconned: Story = {
  args: {
    view: view({
      predicate: "is brother to",
      other: { id: TORMUND, name: "Tormund" },
      origin: { kind: "session", content: { ordinal: 2 } },
      invalidation: { kind: "retconned" },
    }),
  },
  play: async ({ canvas, canvasElement }) => {
    await expect(canvas.getByText("S2 ↯")).toBeInTheDocument();
    await expect(canvasElement.querySelector(".lucide-x")).toBeInTheDocument();
  },
};

// Clicking the row (not the chip) fires onSelect with the row's view - the seam
// the edit modal hangs off in a later slice.
export const Selects: Story = {
  play: async ({ args, canvas, userEvent }) => {
    await userEvent.click(canvas.getByText("keeps the key to"));
    await expect(args.onSelect).toHaveBeenCalledWith(args.view);
  },
};
