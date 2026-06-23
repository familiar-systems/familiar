// Component tests for one relationship row across its rail states, plus the onSelect
// wiring (a real click that no pure test reaches). The row takes a plain
// RelationshipView + callbacks (spied with fn()), so it renders from in-memory data
// with no socket. Branded-id fixtures are cast like the TreeID fixtures in
// TocTree.stories.tsx (no runtime guard exists for these brands); PageIds go through
// pageIdSchema, which does.

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
const BURNING = pageIdSchema.parse("01EM8K2P9XQ4R7T3V6W1Y5Z0AB");
const TORMUND = pageIdSchema.parse("01F8MK6X2N4P7R9T3V6W1Y5Z0B");

function view(over: Partial<RelationshipView>): RelationshipView {
  return {
    id: rid("01TEST00000000000000000REL"),
    other: { id: WATCHTOWER, name: "North Watchtower" },
    predicate: "keeps the key to",
    predicate_reverse: "is kept by",
    origin: { kind: "session", content: { ordinal: 3 } },
    superseded: null,
    retcon: null,
    knowledge: { kind: "public" },
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

// A live, public fact: the predicate, the linked entity, a lone session pill.
export const Live: Story = {
  play: async ({ canvas, canvasElement }) => {
    await expect(canvas.getByText("keeps the key to")).toBeInTheDocument();
    await expect(canvas.getByText("North Watchtower")).toBeInTheDocument();
    await expect(canvasElement.textContent).toContain("S3");
  },
};

// A fact true before the campaign began: the origin pill reads "Prior".
export const Prior: Story = {
  args: {
    view: view({
      predicate: "is a resident of",
      other: { id: GRIMHOLLOW, name: "Grimhollow" },
      origin: { kind: "prior" },
    }),
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.textContent).toContain("Prior");
  },
};

// Superseded: faded, an origin -> ended rail (S6, S12) with the RotateCcw icon.
export const Superseded: Story = {
  args: {
    view: view({
      predicate: "is captain of",
      other: { id: GUILD, name: "Thren Ferrymen's Guild" },
      origin: { kind: "session", content: { ordinal: 6 } },
      superseded: { ordinal: 12 },
    }),
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.textContent).toContain("S6");
    await expect(canvasElement.textContent).toContain("S12");
    await expect(canvasElement.querySelector(".lucide-rotate-ccw")).toBeInTheDocument();
  },
};

// Born secret, unrevealed: the plum wash and a secret origin pill (EyeOff).
export const Secret: Story = {
  args: {
    view: view({
      predicate: "owes a debt to",
      other: { id: CROWN, name: "Crown of Ash" },
      origin: { kind: "session", content: { ordinal: 11 } },
      knowledge: { kind: "hidden" },
    }),
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector(".lucide-eye-off")).toBeInTheDocument();
  },
};

// Born secret then revealed: a secret origin (EyeOff) and a revealed pill (Eye).
export const RevealedSecret: Story = {
  args: {
    view: view({
      predicate: "set the signal fire at",
      other: { id: BURNING, name: "Burning of the North Watch" },
      origin: { kind: "session", content: { ordinal: 14 } },
      knowledge: { kind: "revealed", content: { ordinal: 15 } },
    }),
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelector(".lucide-eye-off")).toBeInTheDocument();
    await expect(canvasElement.querySelector(".lucide-eye")).toBeInTheDocument();
    await expect(canvasElement.textContent).toContain("S15");
  },
};

// Retconned: struck through, with a terminal "↯ S2" pill.
export const Retconned: Story = {
  args: {
    view: view({
      predicate: "is brother to",
      other: { id: TORMUND, name: "Tormund" },
      origin: { kind: "session", content: { ordinal: 1 } },
      retcon: { ordinal: 2 },
    }),
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.textContent).toContain("↯");
    await expect(canvasElement.textContent).toContain("S2");
    await expect(canvasElement.querySelector(".line-through")).toBeInTheDocument();
  },
};

// The whole row is one edit button (a sibling of the chip link, so the two aren't
// nested); clicking it fires onSelect with the row's view - the seam the edit modal
// hangs off.
export const Selects: Story = {
  play: async ({ args, canvas, userEvent }) => {
    await userEvent.click(canvas.getByRole("button", { name: /Edit relationship/ }));
    await expect(args.onSelect).toHaveBeenCalledWith(args.view);
  },
};
