import { campaignIdSchema, type Campaign } from "@familiar-systems/types-app";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect } from "storybook/test";

import { CampaignCard } from "./CampaignCard";

// A fully-initialized campaign; variants below override the fields that drive
// the card's state machine (deriveState in CampaignCard.tsx).
const baseCampaign: Campaign = {
  id: campaignIdSchema.parse("abcdefghijklmnopqrstu"),
  name: "The Hollow Crown",
  tagline: "A kingdom rots from the throne down; the heirs do not yet know.",
  game_system: "Pathfinder 2e",
  content_locale: "en",
  last_init_error: null,
  loaded: false,
  wizard_completed_at: "2026-05-01T12:00:00Z",
  created_at: "2026-04-01T12:00:00Z",
  updated_at: "2026-06-01T12:00:00Z",
};

const meta = {
  title: "Components/CampaignCard",
  component: CampaignCard,
  // The card is a hub-grid tile; give it a realistic column width in isolation.
  decorators: [
    (Story) => (
      <div style={{ width: 360 }}>
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof CampaignCard>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Ready: Story = {
  args: { campaign: baseCampaign, loaded: false },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("The Hollow Crown")).toBeInTheDocument();
    await expect(canvas.getByText("Ready to Load")).toBeInTheDocument();
  },
};

export const Loaded: Story = {
  args: { campaign: baseCampaign, loaded: true },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("Loaded")).toBeInTheDocument();
  },
};

export const Draft: Story = {
  args: {
    campaign: {
      ...baseCampaign,
      name: null,
      tagline: null,
      game_system: null,
      wizard_completed_at: null,
    },
    loaded: false,
  },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("Draft")).toBeInTheDocument();
  },
};

export const InitFailed: Story = {
  args: {
    campaign: { ...baseCampaign, last_init_error: "Failed to seed starter content" },
    loaded: false,
  },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("Init failed")).toBeInTheDocument();
  },
};
