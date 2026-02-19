import { Hono } from "hono";
import { serve } from "@hono/node-server";

const app = new Hono();

app.get("/api/health", (c) => c.json({ status: "ok" }));

serve({ fetch: app.fetch, port: 3001 }, (info) => {
  console.log(`API server listening on http://localhost:${String(info.port)}`);
});
