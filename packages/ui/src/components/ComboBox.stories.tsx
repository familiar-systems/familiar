import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, within } from "storybook/test";

import { ComboBox, ComboBoxItem } from "./ComboBox";

// Annotated (not `satisfies`) to avoid TS2883: React Aria's generic prop types
// aren't portably nameable under this repo's declaration emit.
const meta: Meta<typeof ComboBox> = {
  title: "UI/ComboBox",
  component: ComboBox,
  decorators: [(Story) => <div style={{ width: 280 }}>{<Story />}</div>],
};

export default meta;
type Story = StoryObj<typeof meta>;

const items = (
  <>
    <ComboBoxItem id="dnd">D&D 5e</ComboBoxItem>
    <ComboBoxItem id="pf2e">Pathfinder 2e</ComboBoxItem>
    <ComboBoxItem id="bitd">Blades in the Dark</ComboBoxItem>
  </>
);

export const Default: Story = {
  render: () => (
    <ComboBox aria-label="Game system" allowsCustomValue>
      {items}
    </ComboBox>
  ),
  // Typing filters the list client-side; picking writes the option's text back.
  play: async ({ canvas, userEvent }) => {
    const input = canvas.getByRole("combobox");
    await userEvent.click(input);
    await userEvent.type(input, "Blades");
    const listbox = await within(document.body).findByRole("listbox");
    await expect(within(listbox).getAllByRole("option")).toHaveLength(1);
    await userEvent.click(within(listbox).getByRole("option", { name: "Blades in the Dark" }));
    await expect(input).toHaveValue("Blades in the Dark");
  },
};

// The borderless, dashed-underline variant used inside running text.
export const Inline: Story = {
  render: () => (
    <p>
      runs on{" "}
      <ComboBox aria-label="Game system" variant="inline" allowsCustomValue>
        {items}
      </ComboBox>
    </p>
  ),
  play: async ({ canvas, userEvent }) => {
    const input = canvas.getByRole("combobox");
    await userEvent.type(input, "Path");
    const listbox = await within(document.body).findByRole("listbox");
    await expect(
      within(listbox).getByRole("option", { name: "Pathfinder 2e" }),
    ).toBeInTheDocument();
  },
};
