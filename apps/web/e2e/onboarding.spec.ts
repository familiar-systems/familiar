import { type Page, expect, test } from "@playwright/test";

// End-to-end exercise of the new-campaign wizard.
//
// The platform mints a campaign id, the SPA navigates into the campaign,
// the wizard fetches the catalog, the user walks through the four steps,
// and pressing the seal fires `PATCH /campaign/{id}` with
// `wizard_complete: true`. Two tests: success (transitions to the
// initialized view) and failure (hub shows the init-failed badge).
//
// All network calls are stubbed: this test does not need a running
// platform, campaign, or Hanko backend. The shape of each stub mirrors
// the wire types in `@familiar-systems/types-app` and
// `@familiar-systems/types-campaign`.

const MOCK_USER = {
  id: "00000000-0000-0000-0000-000000000000",
  email: "mock@example.com",
};

const CAMPAIGN_ID = "test-campaign-onboarding-1";

const MOCK_SYSTEMS = {
  systems: [
    {
      id: "blades-in-the-dark",
      name: "Blades in the Dark",
      tagline: "The Crew does jobs in a haunted, electrified city.",
      color: "#212227",
      popular: true,
      bundle: [
        { slug: "common/player", name: "Player", description: "", icon: "user" },
        { slug: "common/npc", name: "NPC", description: "", icon: "person-standing" },
        { slug: "blades-in-the-dark/crew", name: "Crew", description: "", icon: "users" },
      ],
    },
    {
      id: "dnd-5e",
      name: "D&D 5e (2014)",
      tagline: "Heroic high fantasy.",
      color: "#FF0000",
      popular: true,
      bundle: [
        { slug: "common/player", name: "Player", description: "", icon: "user" },
        { slug: "common/npc", name: "NPC", description: "", icon: "person-standing" },
      ],
    },
  ],
  byo: {
    bundle: [
      { slug: "common/player", name: "Player", description: "", icon: "user" },
      { slug: "common/npc", name: "NPC", description: "", icon: "person-standing" },
    ],
  },
};

interface MockState {
  campaigns: Array<{
    id: string;
    name: string | null;
    tagline: string | null;
    game_system: string | null;
    content_locale: string | null;
    last_init_error: string | null;
    wizard_completed_at: string | null;
    created_at: string;
    updated_at: string;
  }>;
}

async function installMocks(page: Page, state: MockState): Promise<void> {
  // Hanko: any request to the placeholder host resolves as a valid session.
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

  await page.route("**/api/me", async (route) => {
    return route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(MOCK_USER),
    });
  });

  // Platform: single-campaign fetch (lease + metadata).
  await page.route(`**/api/campaigns/${CAMPAIGN_ID}`, async (route) => {
    if (route.request().method() === "GET") {
      const row = state.campaigns.find((c) => c.id === CAMPAIGN_ID);
      if (!row) {
        return route.fulfill({ status: 404, contentType: "application/json", body: "{}" });
      }
      return route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(row),
      });
    }
    return route.fallback();
  });

  // Platform: list + create campaigns.
  await page.route("**/api/campaigns", async (route) => {
    const req = route.request();
    if (req.method() === "GET") {
      return route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(state.campaigns),
      });
    }
    if (req.method() === "POST") {
      const now = new Date().toISOString();
      // Mimic the platform's create: mint id, write a draft row, return id.
      const exists = state.campaigns.find((c) => c.id === CAMPAIGN_ID);
      if (!exists) {
        state.campaigns.push({
          id: CAMPAIGN_ID,
          name: null,
          tagline: null,
          game_system: null,
          content_locale: null,
          last_init_error: null,
          wizard_completed_at: null,
          created_at: now,
          updated_at: now,
        });
      }
      return route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ campaign_id: CAMPAIGN_ID }),
      });
    }
    return route.fallback();
  });

  // Campaign tier: GET + PATCH /campaign/{id}, catalog.
  await page.route(`**/campaign/${CAMPAIGN_ID}`, async (route) => {
    const method = route.request().method();
    const row = state.campaigns.find((c) => c.id === CAMPAIGN_ID);

    if (method === "GET") {
      if (!row) {
        return route.fulfill({ status: 404, contentType: "application/json", body: "{}" });
      }
      return route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          campaign_id: row.id,
          name: row.name ?? "",
          tagline: row.tagline,
          game_system: row.game_system,
          content_locale: row.content_locale,
          wizard_completed_at: row.wizard_completed_at,
          created_at: row.created_at,
          updated_at: row.updated_at,
        }),
      });
    }

    if (method === "PATCH") {
      // Default: deliberate failure so the hub shows the badge.
      if (row) {
        row.last_init_error = "deliberate_thin_slice_failure";
        row.updated_at = new Date().toISOString();
      }
      return route.fulfill({
        status: 500,
        contentType: "application/json",
        body: JSON.stringify({
          error: "Campaign initialization is not yet wired up. This is a known thin-slice failure.",
          campaign_id: CAMPAIGN_ID,
        }),
      });
    }

    return route.fallback();
  });

  await page.route("**/catalog/systems**", async (route) => {
    return route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(MOCK_SYSTEMS),
    });
  });
}

test("wizard success transitions to initialized campaign view", async ({ page }) => {
  const state: MockState = { campaigns: [] };
  await installMocks(page, state);

  // Override the PATCH handler on /campaign/{id} to succeed and mirror metadata.
  await page.route(`**/campaign/${CAMPAIGN_ID}`, async (route) => {
    if (route.request().method() !== "PATCH") return route.fallback();
    const body = JSON.parse(route.request().postData() ?? "{}") as {
      name?: string;
      tagline?: string | null;
      game_system?: string;
      content_locale?: string;
      wizard_complete?: boolean;
    };
    const row = state.campaigns.find((c) => c.id === CAMPAIGN_ID);
    if (row) {
      if (body.wizard_complete) row.wizard_completed_at = new Date().toISOString();
      if (body.name !== undefined) row.name = body.name;
      if (body.tagline !== undefined) row.tagline = body.tagline;
      if (body.game_system !== undefined) row.game_system = body.game_system;
      if (body.content_locale !== undefined) row.content_locale = body.content_locale;
      row.updated_at = new Date().toISOString();
    }
    return route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        campaign_id: CAMPAIGN_ID,
        name: row?.name ?? "",
        tagline: row?.tagline ?? null,
        game_system: row?.game_system ?? null,
        content_locale: row?.content_locale ?? null,
        wizard_completed_at: row?.wizard_completed_at ?? null,
        created_at: row?.created_at ?? new Date().toISOString(),
        updated_at: row?.updated_at ?? new Date().toISOString(),
      }),
    });
  });

  await page.goto("/");
  await page.getByTestId("start-first-campaign").click();
  await expect(page).toHaveURL(`/c/${CAMPAIGN_ID}`);
  await expect(page.getByTestId("campaign-wizard")).toBeVisible();

  // Step 1: name + tagline.
  await page.getByTestId("wizard-name-input").fill("Embergrove Saga");
  await page.getByTestId("wizard-tagline-input").fill("An autumn court, a debt come due.");
  await page.getByTestId("wizard-next").click();

  // Step 2: pick Blades.
  await page.getByTestId("system-search-input").fill("blades");
  await page.getByTestId("system-card-blades-in-the-dark").click();
  await page.getByTestId("wizard-next").click();

  // Step 3: privacy.
  await page.getByTestId("audio-opt-out").click();
  await page.getByTestId("evals-off").click();
  await page.getByTestId("wizard-next").click();

  // Step 4: seal. Should stay on the campaign, not bounce to hub.
  await page.getByTestId("wax-seal").click();
  await expect(page.getByTestId("campaign-placeholder")).toBeVisible();
  await expect(page).toHaveURL(`/c/${CAMPAIGN_ID}`);
  await expect(page.getByText("Embergrove Saga")).toBeVisible();
});

test("wizard walks through every step, fails on initialize, hub shows the badge", async ({
  page,
}) => {
  const state: MockState = { campaigns: [] };
  await installMocks(page, state);

  // Hub: empty list shows the start-your-first-campaign CTA.
  await page.goto("/");
  await expect(page.getByTestId("start-first-campaign")).toBeVisible();

  // Click the CTA. SPA POSTs /api/campaigns, gets the id, navigates into /c/<id>.
  await page.getByTestId("start-first-campaign").click();
  await expect(page).toHaveURL(`/c/${CAMPAIGN_ID}`);
  await expect(page.getByTestId("campaign-wizard")).toBeVisible();

  // Step 1: name + tagline.
  await page.getByTestId("wizard-name-input").fill("Embergrove Saga");
  await page.getByTestId("wizard-tagline-input").fill("An autumn court, a debt come due.");
  await page.getByTestId("wizard-next").click();

  // Step 2: pick Blades via the scriptorium search.
  await expect(page.getByTestId("system-search-input")).toBeVisible();
  // BYO card is always visible.
  await expect(page.getByTestId("byo-card")).toBeVisible();
  await page.getByTestId("system-search-input").fill("blades");
  await page.getByTestId("system-card-blades-in-the-dark").click();
  // Bundle is auto-populated; templates editor is visible.
  await expect(page.getByTestId("templates-editor")).toBeVisible();
  await page.getByTestId("wizard-next").click();

  // Step 3: privacy. Both fields required; Continue is disabled until both are set.
  await expect(page.getByTestId("wizard-next")).toBeDisabled();
  await page.getByTestId("audio-opt-out").click();
  await expect(page.getByTestId("wizard-next")).toBeDisabled();
  await page.getByTestId("evals-off").click();
  await expect(page.getByTestId("wizard-next")).toBeEnabled();
  await page.getByTestId("wizard-next").click();

  // Step 4: review + initialize.
  await expect(page.getByTestId("review-summary")).toBeVisible();
  await expect(page.getByTestId("wax-seal")).toHaveAttribute("data-state", "idle");

  // Pressing the seal fires the deliberate failure.
  await page.getByTestId("wax-seal").click();
  await expect(page.getByTestId("wax-seal")).toHaveAttribute("data-state", "cracked");
  await expect(page.getByTestId("seal-error")).toContainText("not yet wired up");

  // Navigate back to the hub via the seal-back button. The list now
  // includes our failed campaign with the badge.
  await page.getByTestId("seal-back").click();
  // From step 3 the user can navigate "Back" to hub via the SPA navigation;
  // the wizard's onBack returns to the previous step. Use the logo link
  // to get back to /.
  await page.getByRole("link", { name: "familiar.systems hub" }).click();
  await expect(page).toHaveURL("/");
  await expect(page.getByTestId(`campaign-card-${CAMPAIGN_ID}`)).toBeVisible();
  await expect(page.getByTestId(`campaign-card-${CAMPAIGN_ID}`)).toHaveAttribute(
    "data-state",
    "init-failed",
  );
});
