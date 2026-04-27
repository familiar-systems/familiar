import { siteLink } from "../lib/paths";

export function CookieNotice(): React.ReactElement {
  return (
    <p className="mx-auto max-w-3xl px-6 py-4 text-center text-sm leading-relaxed text-muted-foreground">
      By signing up or logging in, you consent to functional cookies. We never have and never will
      sell your data. We just want to know if things are working well. See our{" "}
      <a href={siteLink("/privacy")} className="text-gold underline-offset-2 hover:underline">
        privacy policy
      </a>{" "}
      for further details.
    </p>
  );
}
