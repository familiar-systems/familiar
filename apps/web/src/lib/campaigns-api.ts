// Campaign-tier client for catalog + initialize. Lives separately from
// `lib/api.ts` (which is the platform-side openapi-fetch client) because
// the campaign tier doesn't ship an OpenAPI spec yet; types come from
// `@familiar-systems/types-campaign` directly.
//
// Authentication: the campaign tier doesn't validate Hanko sessions today
// (the wizard payload is internally idempotent + the campaign id in the
// URL is unguessable). When per-user authz lands on the campaign tier,
// this client gains the same Bearer middleware as `lib/api.ts`.

import type {
  CatalogResponse,
  InitializeErrorResponse,
  InitializeRequest,
} from "@familiar-systems/types-campaign";
import { campaignPath, catalogPath } from "./paths";

export class CampaignApiError extends Error {
  // Surfaces structured error details from the campaign tier so the wizard
  // can render the deliberate-failure copy inline. `body` is the parsed
  // JSON when present; `status` is always set.
  constructor(
    public readonly status: number,
    public readonly body: InitializeErrorResponse | { error?: string } | null,
    message: string,
  ) {
    super(message);
    this.name = "CampaignApiError";
  }
}

export async function fetchCatalog(locale: string): Promise<CatalogResponse> {
  const url = `${catalogPath("systems")}?locale=${encodeURIComponent(locale)}`;
  const resp = await fetch(url, { headers: { accept: "application/json" } });
  if (!resp.ok) {
    throw new CampaignApiError(resp.status, null, `catalog fetch failed: ${resp.status}`);
  }
  return (await resp.json()) as CatalogResponse;
}

export async function initializeCampaign(
  campaignId: string,
  body: InitializeRequest,
): Promise<void> {
  const url = campaignPath(`${campaignId}/initialize`);
  const resp = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json", accept: "application/json" },
    body: JSON.stringify(body),
  });
  if (resp.ok) {
    return;
  }
  // Try to parse the structured error body; fall back to status-only if not JSON.
  let parsed: InitializeErrorResponse | { error?: string } | null = null;
  try {
    parsed = (await resp.json()) as InitializeErrorResponse;
  } catch {
    parsed = null;
  }
  const message =
    parsed && "error" in parsed && typeof parsed.error === "string"
      ? parsed.error
      : `initialize failed: ${resp.status}`;
  throw new CampaignApiError(resp.status, parsed, message);
}
