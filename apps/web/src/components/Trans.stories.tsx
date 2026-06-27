import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect } from "storybook/test";

import { Trans } from "./Trans";

const meta = {
  title: "Components/Trans",
  component: Trans,
} satisfies Meta<typeof Trans>;

export default meta;
type Story = StoryObj<typeof meta>;

// A registered tag renders its element; the surrounding text stays plain.
export const Basic: Story = {
  args: {
    message: "Your <gold>worlds</gold> await.",
    components: { gold: (c) => <em data-testid="gold">{c}</em> },
  },
  play: async ({ canvas, canvasElement }) => {
    await expect(canvas.getByTestId("gold")).toHaveTextContent("worlds");
    await expect(canvasElement).toHaveTextContent("Your worlds await.");
  },
};

// Two sibling tags in one message each render independently.
export const TwoTags: Story = {
  args: {
    message: "Data is <b>never</b> sold or <b>shared</b>.",
    components: { b: (c) => <strong>{c}</strong> },
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll("strong")).toHaveLength(2);
    await expect(canvasElement).toHaveTextContent("Data is never sold or shared.");
  },
};

// An unregistered tag degrades to its inner text, no wrapping element.
export const Fallback: Story = {
  args: {
    message: "Plain <mystery>text</mystery> here.",
    components: {},
  },
  play: async ({ canvasElement }) => {
    await expect(canvasElement).toHaveTextContent("Plain text here.");
    await expect(canvasElement.querySelector("em")).toBeNull();
  },
};
