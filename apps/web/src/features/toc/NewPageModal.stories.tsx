// Component tests for the New menu modal. The modal takes plain callbacks
// (spied with fn()) and renders through a portal to document.body, so queries go
// through `screen` (document-scoped) rather than the story `canvas`.

import type { PageId } from "@familiar-systems/types-campaign";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, screen, waitFor } from "storybook/test";

import { NewPageModal } from "./NewPageModal";

const meta = {
  title: "Features/Toc/NewPageModal",
  component: NewPageModal,
  args: {
    onSubmit: fn(),
    onClose: fn(),
    // Most stories exercise the no-template flow; ClonesFromTemplate overrides this.
    templates: [],
  },
} satisfies Meta<typeof NewPageModal>;

export default meta;
type Story = StoryObj<typeof meta>;

// Step 1: the picker offers every creatable kind (Session, Entity, Template).
// Folders are deliberately absent (no create route yet).
export const Pick: Story = {
  play: async () => {
    await expect(screen.getByText("What are you creating?")).toBeInTheDocument();
    await expect(screen.getByRole("button", { name: /New session/ })).toBeInTheDocument();
    await expect(screen.getByRole("button", { name: /New entity/ })).toBeInTheDocument();
    await expect(screen.getByRole("button", { name: /New template/ })).toBeInTheDocument();
  },
};

// A session is named like every other kind now: the field opens empty with the
// cursor in it, Create stays disabled until it's non-empty, then sends the name.
export const SessionRequiresName: Story = {
  play: async ({ args, userEvent }) => {
    await userEvent.click(screen.getByRole("button", { name: /New session/ }));
    const input = screen.getByLabelText("Name");
    await expect(input).toHaveValue("");
    await waitFor(() => expect(input).toHaveFocus());
    const create = screen.getByRole("button", { name: "Create" });
    await expect(create).toBeDisabled();
    await userEvent.type(input, "The Fall of Perth");
    await expect(create).toBeEnabled();
    await userEvent.click(create);
    await expect(args.onSubmit).toHaveBeenCalledWith("session", "The Fall of Perth", null);
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
    // No templates in this campaign, so no clone: fromTemplateId is null.
    await expect(args.onSubmit).toHaveBeenCalledWith("entity", "Wren Aldwater", null);
  },
};

// A template must be named too; creating one sends kind "template".
export const TemplateRequiresName: Story = {
  play: async ({ args, userEvent }) => {
    await userEvent.click(screen.getByRole("button", { name: /New template/ }));
    const create = screen.getByRole("button", { name: "Create" });
    await expect(create).toBeDisabled();
    await userEvent.type(screen.getByLabelText("Name"), "NPC");
    await expect(create).toBeEnabled();
    await userEvent.click(create);
    await expect(args.onSubmit).toHaveBeenCalledWith("template", "NPC", null);
  },
};

// With templates available, the entity step gains a "Base on template" selector.
// Picking one sends its PageId as `fromTemplateId` so the server clones it.
const TEMPLATES = [
  { pageId: "01ARZ3NDEKTSV4RRFFQ69G5FAV" as PageId, name: "NPC" },
  { pageId: "01BX5ZZKBKACTAV9WEVGEMMVRZ" as PageId, name: "Region" },
];

export const ClonesFromTemplate: Story = {
  args: { templates: TEMPLATES },
  play: async ({ args, userEvent }) => {
    await userEvent.click(screen.getByRole("button", { name: /New entity/ }));

    // The selector opens on "Blank entity"; choose the NPC template.
    await userEvent.click(screen.getByRole("button", { name: /Base on template/ }));
    await userEvent.click(await screen.findByRole("option", { name: "NPC" }));

    await userEvent.type(screen.getByLabelText("Name"), "Grimhollow");
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    await expect(args.onSubmit).toHaveBeenCalledWith("entity", "Grimhollow", TEMPLATES[0]?.pageId);
  },
};
