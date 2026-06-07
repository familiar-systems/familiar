// Full-stack end-to-end smoke test.
//
// Unlike the integration specs (apps/web/integration/), this drives the REAL
// stack: SPA -> Caddy -> platform -> campaign -> Loro WebSocket -> SQLite. The
// `.mise/tasks/e2e` harness boots everything on an isolated port block and
// points this config's baseURL at the Caddy app apex; only the third-party
// Hanko edge is stubbed (a tiny mock /sessions/validate the servers and the
// browser both trust). The harness owns shutdown + the SQLite assertion; this
// spec is the browser flow.
//
// The happy path: create a Daggerheart campaign through the wizard, edit the
// seeded home page, create and edit a second page, navigate between them via the
// table of contents and assert the content survived the server round-trip, then
// rename the page and assert the ToC updates live (which, because room actors
// flush on last-subscriber-leave / on stop, also proves it reached the campaign
// DB).

import { expect, test } from "@playwright/test";

const APP = "http://app.localhost:18080";

test.beforeEach(async ({ context }) => {
  // This is the entire "login". The Hanko SDK's getSessionToken() reads the
  // `hanko` cookie (verified: top-level getSessionToken() === cookie
  // .getAuthCookie()), and that token rides both the Authorization: Bearer
  // header and the WebSocket ?token=. The value is arbitrary because the mock
  // Hanko accepts any token; no passcode/passkey flow needed.
  await context.addCookies([{ name: "hanko", value: "e2e-token", url: APP }]);
});

// The editor opens with one empty paragraph. Type the first line into it, press
// Enter to split off a second block, type the second line -> two content blocks
// (which is what the harness's DB assertion checks per Page).
async function typeTwoLines(
  page: import("@playwright/test").Page,
  editor: import("@playwright/test").Locator,
  first: string,
  second: string,
): Promise<void> {
  await editor.click();
  await page.keyboard.type(first);
  await page.keyboard.press("Enter");
  await page.keyboard.type(second);
  await expect(editor).toContainText(first);
  await expect(editor).toContainText(second);
}

test("create a campaign, edit two pages, and navigate between them via the ToC", async ({
  page,
}) => {
  // --- Hub (empty on a fresh DB) -> start the first campaign. ---
  await page.goto("/");
  await page.getByTestId("start-first-campaign").click();

  // --- Wizard (lands on /c/{id}, wizard-incomplete). ---
  await expect(page.getByTestId("campaign-wizard")).toBeVisible();
  await page.getByTestId("wizard-name-input").fill("The Smoke Test Saga");
  await page.getByTestId("wizard-next").click();
  // Picking Daggerheart auto-selects its template bundle, so the wizard lets us advance.
  await page.getByTestId("system-card-daggerheart").click();
  await page.getByTestId("wizard-next").click();
  // Privacy: both choices are required before Continue enables.
  await page.getByTestId("audio-text-only").click();
  await page.getByTestId("evals-off").click();
  await page.getByTestId("wizard-next").click();
  // Seal -> PATCH wizard_complete -> index refetches -> redirect to home editor.
  await page.getByTestId("wax-seal").click();

  // --- Home editor ("Campaign Base Camp"), once the Loro doc has synced. ---
  await expect(page).toHaveURL(/\/c\/[^/]+\/p\/[^/]+$/);
  await expect(page.locator(".ProseMirror")).toBeVisible();

  // Sentinel for the no-full-reload invariant: SPA navigation must not reload
  // the document (which would wipe this), so we assert it persists after the
  // ToC navigations below.
  await page.evaluate(() => {
    (window as unknown as { __smokeNoReload?: boolean }).__smokeNoReload = true;
  });

  // --- Edit the home page: two paragraph blocks. ---
  await typeTwoLines(page, page.locator(".ProseMirror"), "Home line one", "Home line two");

  // --- Create a second page ("Test page") via the ToC. ---
  const sidebar = page.locator("aside");
  await sidebar.getByRole("button", { name: "New page" }).click();
  const nameInput = page.getByPlaceholder("Page name");
  await nameInput.fill("Test page");
  await nameInput.press("Enter");

  // Creation navigates to the new page; the new node arrives over the ToC room.
  await expect(sidebar.getByRole("button", { name: "Test page" })).toBeVisible();
  await expect(page.locator(".ProseMirror")).toBeVisible();

  // --- Edit the test page: two paragraph blocks. ---
  await typeTwoLines(page, page.locator(".ProseMirror"), "Test line one", "Test line two");

  // --- Navigate back to home via the ToC; its content survived. ---
  await sidebar.getByRole("button", { name: "Campaign Base Camp" }).click();
  await expect(page.locator(".ProseMirror")).toContainText("Home line one");
  await expect(page.locator(".ProseMirror")).toContainText("Home line two");

  // --- Navigate to the test page via the ToC; its content survived too. ---
  await sidebar.getByRole("button", { name: "Test page" }).click();
  await expect(page.locator(".ProseMirror")).toContainText("Test line one");
  await expect(page.locator(".ProseMirror")).toContainText("Test line two");

  // --- Rename the open page; the ToC updates live. This is the
  // server-authoritative path: the client writes only `meta.title`, the
  // PageActor mirrors it to `pages.name` and pushes the rename to the ToC. The
  // sidebar label changing proves the server round-trip happened; the harness's
  // DB assertion proves the new name reached `pages.name`. ---
  const title = page.getByLabel("Page title");
  await expect(title).toHaveValue("Test page");
  await title.fill("The Sunken Bastion");
  await expect(sidebar.getByRole("button", { name: "The Sunken Bastion" })).toBeVisible();
  await expect(sidebar.getByRole("button", { name: "Test page" })).toHaveCount(0);

  // Empty titles are not allowed: clearing the field and blurring reverts to the
  // last name (and never blanks the ToC).
  await title.fill("");
  await page.locator(".ProseMirror").click();
  await expect(title).toHaveValue("The Sunken Bastion");
  await expect(sidebar.getByRole("button", { name: "The Sunken Bastion" })).toBeVisible();

  // The whole flow was client-side navigation: the shell never reloaded.
  const stillMounted = await page.evaluate(
    () => (window as unknown as { __smokeNoReload?: boolean }).__smokeNoReload === true,
  );
  expect(stillMounted).toBe(true);
});
