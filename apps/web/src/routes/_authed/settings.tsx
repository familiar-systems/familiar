import { createFileRoute } from "@tanstack/react-router";
import { hanko } from "../../lib/hanko";
import { spaRoute } from "../../lib/paths";
import { m } from "../../paraglide/messages.js";

function Settings(): React.ReactElement {
  // user comes from the _authed layout's beforeLoad, which has already
  // narrowed AuthState to the 'authed' variant. No null check needed.
  const { user } = Route.useRouteContext();

  const onSignOut = async (): Promise<void> => {
    try {
      await hanko.logout();
    } finally {
      window.location.assign(spaRoute("login"));
    }
  };

  return (
    <section className="mx-auto max-w-2xl px-6 py-24">
      <header className="mb-12">
        <span className="block text-xs font-medium tracking-[0.28em] text-muted-foreground uppercase">
          {m.settingsEyebrow()}
        </span>
        <h1 className="mt-3 font-display text-3xl tracking-tight md:text-4xl">
          {m.settingsAccount()}
        </h1>
      </header>

      <article className="mb-6 rounded-2xl border border-foreground/10 bg-background p-6 shadow-[0_8px_32px_-16px_rgb(28_25_23/0.18)] dark:shadow-[0_12px_40px_-18px_rgb(0_0_0/0.45)]">
        <h2 className="mb-5 font-display text-xl">{m.settingsProfile()}</h2>
        <dl className="grid grid-cols-[auto_1fr] items-baseline gap-x-8 gap-y-4">
          <dt className="text-sm text-muted-foreground">{m.settingsEmail()}</dt>
          <dd className="font-display text-base break-all text-foreground md:text-lg">
            {user.email}
          </dd>
          <dt className="text-sm text-muted-foreground">{m.settingsUserId()}</dt>
          <dd className="font-mono text-xs break-all text-muted-foreground/80">{user.id}</dd>
        </dl>
      </article>

      <article className="rounded-2xl border border-foreground/10 bg-background p-6 shadow-[0_8px_32px_-16px_rgb(28_25_23/0.18)] dark:shadow-[0_12px_40px_-18px_rgb(0_0_0/0.45)]">
        <h2 className="mb-2 font-display text-xl">{m.settingsSession()}</h2>
        <p className="mb-5 text-sm text-muted-foreground">{m.settingsSessionDescription()}</p>
        <button
          type="button"
          onClick={() => {
            void onSignOut();
          }}
          className="inline-flex items-center justify-center rounded-lg border border-foreground/10 bg-transparent px-4 py-2 text-sm font-medium transition-colors hover:bg-foreground/5"
        >
          {m.settingsSignOut()}
        </button>
      </article>
    </section>
  );
}

export const Route = createFileRoute("/_authed/settings")({
  component: Settings,
});
