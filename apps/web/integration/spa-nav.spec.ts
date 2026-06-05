import { type Page, expect, test } from "@playwright/test";

// E2E smoke tests for the SPA's navigation contract. These are the
// assertions a code-based / file-based router refactor can silently
// break, and that vitest cannot catch because it doesn't drive a real
// browser:
//
//   1. Navigation between protected routes does not trigger a document
//      fetch (no full reload).
//   2. The persistent app chrome (the <nav> in Shell) stays mounted -
//      the same DOM node before and after the click. If anyone moves
//      Shell back into a leaf route, this assertion fails.
//   3. /me is fetched once per page load. If anyone re-introduces auth
//      into the router's beforeLoad without memoization, this fails.
//
// The Hanko + /me responses are mocked at the network layer, so the
// test runs against the Vite dev server alone (no platform binary, no
// Hanko backend). The spec is intentionally narrow: it asserts the
// shape of a successful hub <-> settings navigation. Other flows
// (auth gate redirect, /login when authed, 404) belong in their own
// specs as they're added.

const MOCK_USER = {
  id: "00000000-0000-0000-0000-000000000000",
  email: "mock@example.com",
};

async function installAuthMocks(page: Page, meCounter: { count: number }): Promise<void> {
  // Hanko session validation: any request to the configured auth host
  // resolves as a valid session. The placeholder VITE_HANKO_API_URL in
  // playwright.config.ts is auth.example.test; the SDK also probes
  // adjacent endpoints (login init, capabilities), so we catch the host
  // wholesale and respond with empty success bodies for anything we
  // don't explicitly model.
  await page.route("**/auth.example.test/**", async (route) => {
    if (route.request().url().endsWith("/sessions/validate")) {
      return route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ is_valid: true, expiration_time: "2099-01-01T00:00:00Z" }),
      });
    }
    return route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({}),
    });
  });

  // /api/me: count calls so the test can assert the "fetch once per page
  // load" invariant. The platform's real /me upserts the user row, so a
  // re-fetch on every navigation would silently amplify that side effect.
  await page.route("**/api/me", async (route) => {
    meCounter.count += 1;
    return route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(MOCK_USER),
    });
  });
}

test("hub -> settings navigation preserves chrome and does not refetch /me", async ({ page }) => {
  const meCounter = { count: 0 };
  await installAuthMocks(page, meCounter);

  const documentRequests: string[] = [];
  page.on("response", (response) => {
    if (response.request().resourceType() === "document") {
      documentRequests.push(response.url());
    }
  });

  await page.goto("/");
  await expect(page.getByRole("button", { name: "Open account menu" })).toBeVisible();

  // Tag the persistent <nav> with a unique attribute. If Shell remounts
  // during navigation, the new nav element won't carry the tag.
  await page
    .locator("nav")
    .first()
    .evaluate((el) => {
      el.setAttribute("data-mount-tag", "persisted");
    });

  const documentRequestsBeforeClick = documentRequests.length;
  const meCallsBeforeClick = meCounter.count;

  await page.getByRole("button", { name: "Open account menu" }).click();
  await page.getByRole("menuitem", { name: "Settings" }).click();

  await expect(page).toHaveURL("/settings");
  await expect(page.getByRole("heading", { name: "Account" })).toBeVisible();

  // 1. No full reload: clicking Settings produces zero new document fetches.
  expect(documentRequests.length).toBe(documentRequestsBeforeClick);

  // 2. Shell stays mounted: the tagged nav is still in the DOM after the
  // route swap. A leaf-rendered Shell would replace it, dropping the tag.
  await expect(page.locator("nav[data-mount-tag='persisted']")).toBeVisible();

  // 3. /me is not refetched on navigation. The route's beforeLoad must
  // not re-trigger the auth fetch.
  expect(meCounter.count - meCallsBeforeClick).toBe(0);
});
