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
// table of contents and assert the content survived the server round-trip,
// rename the page and assert the ToC updates live (which, because room actors
// flush on last-subscriber-leave / on stop, also proves it reached the campaign
// DB), then create a third entity and relate the two through the create-
// relationship modal, and edit that relationship to conceal it (a knowledge PATCH
// flipping the now-mutable secret bit; server-authoritative REST; the harness asserts
// the row, and that it persisted secret, in SQLite).

import { expect, test } from "@playwright/test";

const APP = "http://app.localhost:18080";

// A page now has two section editors (preamble + body). The smoke flow edits the
// freeform body; scope every `.ProseMirror` query to it so the locator is
// unambiguous (and the harness's DB assertion counts `section = 'body'` blocks).
const BODY_EDITOR = "[data-testid=body-editor] .ProseMirror";

test.beforeEach(async ({ context }) => {
  // This is the entire "login". The Hanko SDK's getSessionToken() reads the
  // `hanko` cookie (verified: top-level getSessionToken() === cookie
  // .getAuthCookie()), and that token rides both the Authorization: Bearer
  // header and the WebSocket ?token=. The value is arbitrary because the mock
  // Hanko accepts any token; no passcode/passkey flow needed.
  await context.addCookies([{ name: "hanko", value: "e2e-token", url: APP }]);
});

// The body section opens with one empty paragraph. Type the first line into it,
// press Enter to split off a second block, type the second line -> two body
// blocks (which is what the harness's DB assertion checks per Page).
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

test("create a campaign, edit pages, navigate the ToC, and relate two entities", async ({
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
  await expect(page.locator(BODY_EDITOR)).toBeVisible();

  // Sentinel for the no-full-reload invariant: SPA navigation must not reload
  // the document (which would wipe this), so we assert it persists after the
  // ToC navigations below.
  await page.evaluate(() => {
    (window as unknown as { __smokeNoReload?: boolean }).__smokeNoReload = true;
  });

  // --- Edit the home page: two body paragraph blocks. ---
  await typeTwoLines(page, page.locator(BODY_EDITOR), "Home line one", "Home line two");

  // --- Create a second page ("Test page") via the New menu modal. ---
  const sidebar = page.locator("aside");
  await sidebar.getByRole("button", { name: "New page" }).click();
  // Pick Entity, then name it. The modal is portaled to <body>, so query the
  // page (not the sidebar locator).
  await page.getByRole("button", { name: /New entity/ }).click();
  const nameInput = page.getByLabel("Name");
  await nameInput.fill("Test page");
  await nameInput.press("Enter");

  // Creation navigates to the new page; the new node arrives over the ToC room.
  await expect(sidebar.getByRole("button", { name: "Test page" })).toBeVisible();
  await expect(page.locator(BODY_EDITOR)).toBeVisible();

  // --- Edit the test page: two body paragraph blocks. ---
  await typeTwoLines(page, page.locator(BODY_EDITOR), "Test line one", "Test line two");

  // --- Create a third entity ("Grimhollow") and relate the two. Do this before
  // the rename below, while the second page still carries its creation name in
  // `pages.name`: the entity search reads that column, and the in-editor rename
  // only reaches it on a later flush. ---
  await sidebar.getByRole("button", { name: "New page" }).click();
  await page.getByRole("button", { name: /New entity/ }).click();
  const grimName = page.getByLabel("Name");
  await grimName.fill("Grimhollow");
  await grimName.press("Enter");
  await expect(sidebar.getByRole("button", { name: "Grimhollow" })).toBeVisible();
  await expect(page.locator(BODY_EDITOR)).toBeVisible();
  // Body content so the harness's per-page block assertion holds here too.
  await typeTwoLines(page, page.locator(BODY_EDITOR), "Grim line one", "Grim line two");

  // The create flow: Grimhollow (the open page = subject) relates to "Test page".
  // A fresh campaign has no known predicates (the typeahead offers only "use
  // custom") and no sessions (origin defaults to Prior). Create it Public (the
  // default); the conceal happens in the edit modal below. The modal is portaled to
  // <body>, so query the page.
  await page.getByRole("button", { name: "+ add" }).click();
  await page.getByLabel("Search entities").fill("Test");
  await page.getByRole("option", { name: /Test page/ }).click();
  await page.getByLabel("Predicate", { exact: true }).fill("is a resident of");
  await page.getByLabel("Reverse predicate", { exact: true }).fill("is the home of");
  await page.getByRole("button", { name: "Create" }).click();
  // On success the modal closes and the widget refetches; the row appears. The
  // predicate text is stable regardless of which page is canonical page_a.
  await expect(page.getByText("is a resident of")).toBeVisible();

  // --- Edit the relationship to conceal it: Public -> Hidden. The secret bit is
  // freely mutable, so this is a knowledge PATCH (no session needed - a fresh campaign
  // has none, so end / supersede / retcon / reveal are unavailable, but conceal isn't).
  // The live row ends up secret (is_secret = true) for the DB assertion. ---
  await page.getByRole("button", { name: /Edit relationship/ }).click();
  await expect(page.getByRole("heading", { name: "Edit relationship" })).toBeVisible();
  await page.getByRole("radio", { name: /Hidden/ }).click();
  await page.getByRole("button", { name: "Conceal" }).click();
  await expect(page.getByRole("heading", { name: "Edit relationship" })).toHaveCount(0);

  // --- Navigate back to home via the ToC; its content survived. ---
  await sidebar.getByRole("button", { name: "Campaign Base Camp" }).click();
  await expect(page.locator(BODY_EDITOR)).toContainText("Home line one");
  await expect(page.locator(BODY_EDITOR)).toContainText("Home line two");

  // --- Navigate to the test page via the ToC; its content survived too. ---
  await sidebar.getByRole("button", { name: "Test page" }).click();
  await expect(page.locator(BODY_EDITOR)).toContainText("Test line one");
  await expect(page.locator(BODY_EDITOR)).toContainText("Test line two");

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
  await page.locator(BODY_EDITOR).click();
  await expect(title).toHaveValue("The Sunken Bastion");
  await expect(sidebar.getByRole("button", { name: "The Sunken Bastion" })).toBeVisible();

  // The whole flow was client-side navigation: the shell never reloaded.
  const stillMounted = await page.evaluate(
    () => (window as unknown as { __smokeNoReload?: boolean }).__smokeNoReload === true,
  );
  expect(stillMounted).toBe(true);
});
