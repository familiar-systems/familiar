import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, waitFor, within } from "storybook/test";

import { Button } from "./Button";
import { Dialog, DialogTrigger, Heading, Modal } from "./Dialog";
import { TextField } from "./TextField";

// Annotated (not `satisfies`) to avoid TS2883: React Aria's prop types aren't
// portably nameable under this repo's declaration emit.
const meta: Meta<typeof Modal> = {
  title: "UI/Dialog",
  component: Modal,
};

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  render: () => (
    <DialogTrigger>
      <Button>New page</Button>
      <Modal>
        <Dialog className="flex flex-col gap-4 outline-none">
          {({ close }) => (
            <>
              <Heading slot="title" className="font-display text-xl">
                Create a page
              </Heading>
              <TextField label="Name" placeholder="Name this page" />
              <div className="flex justify-end gap-2">
                <Button variant="ghost" size="sm" onPress={close}>
                  Cancel
                </Button>
                <Button variant="primary" size="sm" onPress={close}>
                  Create
                </Button>
              </div>
            </>
          )}
        </Dialog>
      </Modal>
    </DialogTrigger>
  ),
  play: async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByRole("button", { name: "New page" }));
    const body = within(document.body);
    const dialog = await body.findByRole("dialog");
    await expect(
      within(dialog).getByRole("heading", { name: "Create a page" }),
    ).toBeInTheDocument();
    // Escape dismisses; with no exit animation it unmounts promptly.
    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(body.queryByRole("dialog")).not.toBeInTheDocument());
  },
};
