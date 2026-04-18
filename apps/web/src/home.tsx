import { useEffect, useState } from "react";
import { getSessionToken } from "./lib/hanko";

type Me = { id: string; hanko_sub: string; email: string | null };

export function Home() {
  const [me, setMe] = useState<Me | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const token = getSessionToken();
    if (!token) {
      window.location.assign("/login");
      return;
    }
    fetch("/api/me", { headers: { Authorization: `Bearer ${token}` } })
      .then(async (r) => {
        if (r.status === 401) {
          window.location.assign("/login");
          return;
        }
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        setMe((await r.json()) as Me);
      })
      .catch((e: unknown) => setError(String(e)));
  }, []);

  if (error) return <pre>Error: {error}</pre>;
  if (!me) return <div>Loading...</div>;
  return <pre>{JSON.stringify(me, null, 2)}</pre>;
}
