// React hook that wraps `POST /api/campaigns` with a fresh idempotency
// token, navigates to `/c/<id>` on success, and exposes loading + error
// state to the calling component.
//
// One call site (EmptyHubCard's CTA) today; HubNav's "New campaign"
// button will use the same hook once the layout exposes hasCampaigns
// dynamically.

import type { CampaignId } from "@familiar-systems/types-app";
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { client } from "../../lib/api";

interface State {
  creating: boolean;
  error: string | null;
}

export function useCreateCampaign(): {
  state: State;
  create: () => Promise<void>;
} {
  const navigate = useNavigate();
  const [state, setState] = useState<State>({ creating: false, error: null });

  const create = async (): Promise<void> => {
    setState({ creating: true, error: null });
    const idempotency_token = crypto.randomUUID();
    const { data, response } = await client.POST("/campaigns", {
      body: { idempotency_token },
    });
    if (!response.ok || !data) {
      setState({ creating: false, error: `Create failed (${response.status})` });
      return;
    }
    setState({ creating: false, error: null });
    await navigate({
      to: "/c/$campaignId",
      params: { campaignId: data.campaign_id as unknown as CampaignId },
    });
  };

  return { state, create };
}
