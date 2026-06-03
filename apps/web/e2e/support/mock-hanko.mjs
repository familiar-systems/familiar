// Mock Hanko server for the full-stack e2e smoke test.
//
// Why this exists: Hanko is a third-party SaaS we can't self-host in a test.
// But neither the Rust servers nor the browser SDK verify tokens locally -
// they POST every token to `{HANKO_API_URL}/sessions/validate` and trust the
// reply (crates/app-shared/src/auth/validator.rs; the SDK's SessionClient).
// So stubbing exactly that one endpoint authenticates the entire stack, the
// same way the Rust integration tests do with wiremock. The seeded `hanko`
// cookie (set by the spec) is what getSessionToken() returns for the Bearer
// header and the WS `?token=`; the value is arbitrary because this mock
// accepts any token.
//
// Two consumers, one body:
//   - Rust validator deserializes { is_valid, claims:{ subject, email{...},
//     expiration, session_id } }; `subject` must parse as a UUID (UserId).
//   - The browser SDK (validateSession) deserializes SessionCheckResponse
//     { is_valid, expiration_time?, claims? }.
// The response below is a superset that satisfies both.
//
// Usage: `node mock-hanko.mjs [port]` (port also via MOCK_HANKO_PORT; default
// 19100, matching the e2e port block). Listens on 127.0.0.1.

import http from "node:http";

const port = Number(process.argv[2] ?? process.env.MOCK_HANKO_PORT ?? 19100);

// A fixed, known-parseable UUID (lifted from the Rust validator's own test so
// we know UserId parsing accepts it). Same subject on every call, so the
// campaign's creator and the WebSocket member resolve to one user.
const SUBJECT = "0195b4a0-0000-7000-8000-000000000001";

const VALIDATE_BODY = JSON.stringify({
  is_valid: true,
  expiration_time: "2099-01-01T00:00:00Z",
  claims: {
    subject: SUBJECT,
    email: { address: "e2e@example.test", is_primary: true, is_verified: true },
    expiration: "2099-01-01T00:00:00Z",
    session_id: "e2e",
  },
});

// CORS so the browser's cross-origin validateSession() succeeds (the app apex
// is a different origin than this mock). The Hanko SDK uses credentialed XHR,
// and under credentials the wildcards `*` are NOT honored: the allowed origin
// must be the echoed Origin, and allowed headers must be listed explicitly
// (reflecting the preflight's Access-Control-Request-Headers is the robust way).
function cors(req, res) {
  const origin = req.headers.origin ?? "*";
  res.setHeader("Access-Control-Allow-Origin", origin);
  res.setHeader("Vary", "Origin");
  res.setHeader("Access-Control-Allow-Credentials", "true");
  res.setHeader("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
  const requested = req.headers["access-control-request-headers"];
  res.setHeader(
    "Access-Control-Allow-Headers",
    requested ?? "content-type, authorization, x-session-token",
  );
}

const server = http.createServer((req, res) => {
  cors(req, res);
  if (req.method === "OPTIONS") {
    res.writeHead(204);
    res.end();
    return;
  }
  // Drain the request body (we don't inspect it; we accept any token).
  req.resume();
  req.on("end", () => {
    const url = req.url ?? "";
    res.setHeader("Content-Type", "application/json");
    // Both consumers hit /sessions/validate but with different verbs: the Rust
    // servers POST it (validator.rs), the browser SDK GETs it
    // (SessionClient.validate). Answer either.
    if (url.endsWith("/sessions/validate")) {
      res.writeHead(200);
      res.end(VALIDATE_BODY);
      return;
    }
    // Anything else the SDK probes (config, etc.) gets a benign empty object;
    // the SDK tolerates this (proven by the integration-tier specs).
    res.writeHead(200);
    res.end("{}");
  });
});

server.listen(port, "127.0.0.1", () => {
  console.error(`[mock-hanko] listening on http://127.0.0.1:${port}`);
});

// The harness stops us with SIGTERM; exit cleanly so it doesn't hang.
for (const sig of ["SIGTERM", "SIGINT"]) {
  process.on(sig, () => server.close(() => process.exit(0)));
}
