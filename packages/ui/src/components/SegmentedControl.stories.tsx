import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect } from "storybook/test";

import { SegmentedControl, SegmentedItem } from "./SegmentedControl";

const meta: Meta<typeof SegmentedControl> = {
  title: "UI/SegmentedControl",
  component: SegmentedControl,
};

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  render: () => (
    <SegmentedControl aria-label="Knowledge" defaultSelectedKeys={["hidden"]}>
      <SegmentedItem id="hidden">Hidden</SegmentedItem>
      <SegmentedItem id="known">Known</SegmentedItem>
    </SegmentedControl>
  ),
  // Single selection gives the segments radio semantics: picking one checks it
  // and unchecks the other.
  play: async ({ canvas, userEvent }) => {
    const hidden = canvas.getByRole("radio", { name: "Hidden" });
    const known = canvas.getByRole("radio", { name: "Known" });
    await expect(hidden).toBeChecked();
    await userEvent.click(known);
    await expect(known).toBeChecked();
    await expect(hidden).not.toBeChecked();
  },
};
