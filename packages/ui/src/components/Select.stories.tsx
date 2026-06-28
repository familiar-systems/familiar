import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, within } from "storybook/test";

import { Select, SelectItem } from "./Select";

const meta: Meta<typeof Select> = {
  title: "UI/Select",
  component: Select,
  decorators: [(Story) => <div style={{ width: 220 }}>{<Story />}</div>],
};

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  render: () => (
    <Select aria-label="Session" defaultSelectedKey="s2">
      <SelectItem id="s1">Session 1</SelectItem>
      <SelectItem id="s2">Session 2</SelectItem>
      <SelectItem id="s3">Session 3</SelectItem>
    </Select>
  ),
  play: async ({ canvas, userEvent }) => {
    const trigger = canvas.getByRole("button");
    await expect(trigger).toHaveTextContent("Session 2");
    await userEvent.click(trigger);
    const listbox = await within(document.body).findByRole("listbox");
    await userEvent.click(within(listbox).getByRole("option", { name: "Session 3" }));
    await expect(trigger).toHaveTextContent("Session 3");
  },
};
