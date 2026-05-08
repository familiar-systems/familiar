import { expect, test } from "@playwright/test";

// Asserts that Tailwind utilities backed by CSS-variable theme tokens
// actually mint CSS at build time. Cat A's defect class is "the class
// name was a no-op" - the linter catches the same shape now, but a
// computed-style assertion catches it again if anyone ever splits a
// theme var out of @theme without realizing the utility goes silent.
//
// LoginCookieNotice on /login renders unauthenticated and uses
// text-muted-foreground, so the test exercises the affected utility
// without auth mocks.
test("text-muted-foreground resolves to the --muted-foreground value", async ({ page }) => {
  // Hanko's session-validate probe fires on mount; without a mock the
  // SDK's network errors fill the console but don't fail the render.
  // Mock to "not validated" so the route stays on /login.
  await page.route("**/auth.example.test/**", async (route) => {
    return route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({}),
    });
  });

  await page.goto("/login");
  const notice = page.getByText(/By signing up or logging in/);
  await expect(notice).toBeVisible();

  const color = await notice.evaluate((el) => getComputedStyle(el).color);
  // Light theme: --muted-foreground = #57534e = rgb(87, 83, 78).
  expect(color).toBe("rgb(87, 83, 78)");
});
