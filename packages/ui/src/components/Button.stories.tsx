import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn } from "storybook/test";

import { Button } from "./Button";

const meta = {
  title: "UI/Button",
  component: Button,
  args: { children: "Read the Vision", onPress: fn() },
} satisfies Meta<typeof Button>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = { args: { variant: "primary" } };
export const Secondary: Story = { args: { variant: "secondary", children: "View on GitHub" } };
export const Outline: Story = { args: { variant: "outline", children: "Outline" } };
export const Ghost: Story = { args: { variant: "ghost", children: "Ghost" } };
export const Danger: Story = { args: { variant: "danger", children: "Delete permanently" } };

// Proves the whole styling pipeline end to end: the gold token from
// packages/design, surfaced as `bg-gold`, generated because apps/web @sources
// packages/ui. rgb(184, 149, 48) is --gold (#b89530) in the light theme.
export const Pressable: Story = {
  args: { variant: "primary" },
  play: async ({ args, canvas, userEvent }) => {
    const button = canvas.getByRole("button", { name: "Read the Vision" });
    await expect(button).toHaveStyle({ backgroundColor: "rgb(184, 149, 48)" });
    await userEvent.click(button);
    await expect(args.onPress).toHaveBeenCalled();
  },
};
