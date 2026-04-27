import { useEffect } from "react";
import { register } from "@teamhanko/hanko-elements";
import { hanko, hankoApiUrl } from "./lib/hanko";
import { siteLink, spaRoute } from "./lib/paths";

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

  return (
    <div style={{ maxWidth: 420, margin: "0 auto", padding: 24 }}>
      <hanko-auth />
      <p
        style={{
          marginTop: 16,
          fontSize: 13,
          color: "#666",
          lineHeight: 1.5,
        }}
      >
        By signing up or logging in, you consent to functional cookies. We never have and never will
        sell your data. We just want to know if things are working well. See our{" "}
        <a href={siteLink("/privacy")} style={{ textDecoration: "underline" }}>
          privacy policy
        </a>{" "}
        for further details.
      </p>
    </div>
  );
}
