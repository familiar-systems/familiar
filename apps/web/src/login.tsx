import { useEffect } from "react";
import { register } from "@teamhanko/hanko-elements";
import { hanko, hankoApiUrl } from "./lib/hanko";
import { spaRoute } from "./lib/paths";

export function Login() {
  useEffect(() => {
    register(hankoApiUrl).catch((error: unknown) => {
      console.error("hanko register failed", error);
    });
    const unsub = hanko.onSessionCreated(() => {
      window.location.assign(spaRoute(""));
    });
    return () => {
      unsub();
    };
  }, []);

  return <hanko-auth />;
}
