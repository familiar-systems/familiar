// Component tests for the New menu modal. The modal takes plain callbacks
// (spied with fn()) and renders through a portal to document.body, so queries go
// through `screen` (document-scoped) rather than the story `canvas`.

import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, screen, waitFor } from "storybook/test";

import { NewPageModal } from "./NewPageModal";

const meta = {
  title: "Features/Toc/NewPageModal",
  component: NewPageModal,
  args: {
    onSubmit: fn(),
    onClose: fn(),
  },
} satisfies Meta<typeof NewPageModal>;

export default meta;
type Story = StoryObj<typeof meta>;

// Step 1: the picker offers exactly the kinds with a working create route today
// (Session + Entity). Templates and folders are deliberately absent.
export const Pick: Story = {
  play: async () => {
    await expect(screen.getByText("What are you creating?")).toBeInTheDocument();
    await expect(screen.getByRole("button", { name: /New session/ })).toBeInTheDocument();
    await expect(screen.getByRole("button", { name: /New entity/ })).toBeInTheDocument();
  },
};

// Choosing Session prefills "Untitled Session" and lands the cursor in the
// field; Create sends that name straight through.
export const SessionDefault: Story = {
  play: async ({ args, userEvent }) => {
    await userEvent.click(screen.getByRole("button", { name: /New session/ }));
    const input = screen.getByLabelText("Name");
    await expect(input).toHaveValue("Untitled Session");
    await waitFor(() => expect(input).toHaveFocus());
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    await expect(args.onSubmit).toHaveBeenCalledWith("session", "Untitled Session");
  },
};

// A session may be left unnamed: clearing the field sends null so the server
// applies its own "Untitled Session".
export const SessionBlankSendsNull: Story = {
  play: async ({ args, userEvent }) => {
    await userEvent.click(screen.getByRole("button", { name: /New session/ }));
    await userEvent.clear(screen.getByLabelText("Name"));
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    await expect(args.onSubmit).toHaveBeenCalledWith("session", null);
  },
};

// An entity must be named: Create stays disabled until the field is non-empty.
export const EntityRequiresName: Story = {
  play: async ({ args, userEvent }) => {
    await userEvent.click(screen.getByRole("button", { name: /New entity/ }));
    const create = screen.getByRole("button", { name: "Create" });
    await expect(create).toBeDisabled();
    await userEvent.type(screen.getByLabelText("Name"), "Wren Aldwater");
    await expect(create).toBeEnabled();
    await userEvent.click(create);
    await expect(args.onSubmit).toHaveBeenCalledWith("entity", "Wren Aldwater");
  },
};
