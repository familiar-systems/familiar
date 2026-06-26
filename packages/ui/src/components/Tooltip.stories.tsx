import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, within } from "storybook/test";

import { Button } from "./Button";
import { Tooltip } from "./Tooltip";

const meta = {
  title: "UI/Tooltip",
  component: Tooltip,
  args: {
    content: "Retcon this page",
    children: (
      <Button variant="icon" aria-label="Retcon">
        ✕
      </Button>
    ),
  },
} satisfies Meta<typeof Tooltip>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

// Tooltip renders in a portal on document.body, so query the body, not canvas.
// Keyboard focus shows it immediately (no hover warmup).
export const ShowsOnFocus: Story = {
  play: async ({ userEvent }) => {
    await userEvent.tab();
    const body = within(document.body);
    await expect(await body.findByRole("tooltip")).toHaveTextContent("Retcon this page");
  },
};
