import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, within } from "storybook/test";

import { Button } from "./Button";
import { Menu, MenuItem, MenuTrigger } from "./Menu";

const meta: Meta<typeof Menu> = {
  title: "UI/Menu",
  component: Menu,
};

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  render: () => (
    <MenuTrigger>
      <Button variant="ghost">Account</Button>
      <Menu>
        <MenuItem id="settings">Settings</MenuItem>
        <MenuItem id="signout">Sign out</MenuItem>
      </Menu>
    </MenuTrigger>
  ),
  play: async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByRole("button", { name: "Account" }));
    const menu = await within(document.body).findByRole("menu");
    await expect(within(menu).getAllByRole("menuitem")).toHaveLength(2);
  },
};
