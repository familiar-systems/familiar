import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect } from "storybook/test";

import { TextField } from "./TextField";

// Explicit annotation (not `satisfies`): TextField's React Aria props pull in
// internal types TS can't name portably under declaration emit (TS2883).
const meta: Meta<typeof TextField> = {
  title: "UI/TextField",
  component: TextField,
  args: { label: "Campaign name", placeholder: "Name your world…" },
  decorators: [(Story) => <div style={{ width: 320 }}>{<Story />}</div>],
};

export default meta;
type Story = StoryObj<typeof meta>;

export const Empty: Story = {};
export const Filled: Story = { args: { defaultValue: "The Siege of Grimhollow" } };

export const Typing: Story = {
  play: async ({ canvas, userEvent }) => {
    const input = canvas.getByLabelText("Campaign name");
    await userEvent.type(input, "Grimhollow");
    await expect(input).toHaveValue("Grimhollow");
  },
};
