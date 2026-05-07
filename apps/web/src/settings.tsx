import { Shell } from "./components/Shell";
import { useAuthedMe } from "./lib/auth";
import { hanko } from "./lib/hanko";
import { spaRoute } from "./lib/paths";

export function Settings(): React.ReactElement {
  const { me, error } = useAuthedMe();

  if (error) return <pre className="p-8">Error: {error}</pre>;
  if (!me) return <div className="p-8 text-muted-foreground">Loading...</div>;

  const onSignOut = async (): Promise<void> => {
    try {
      await hanko.logout();
    } finally {
      window.location.assign(spaRoute("login"));
    }
  };

  return (
    <Shell me={me} hasCampaigns={false}>
      <section className="mx-auto max-w-2xl px-6 pt-24 pb-24">
        <header className="mb-12">
          <span className="block text-xs uppercase tracking-[0.28em] font-medium text-muted-foreground">
            Settings
          </span>
          <h1 className="font-display text-3xl md:text-4xl mt-3 tracking-tight">Account</h1>
        </header>

        <article className="rounded-2xl border border-foreground/10 bg-background p-6 mb-6 shadow-[0_8px_32px_-16px_rgb(28_25_23/0.18)] dark:shadow-[0_12px_40px_-18px_rgb(0_0_0/0.45)]">
          <h2 className="font-display text-xl mb-5">Profile</h2>
          <dl className="grid grid-cols-[auto_1fr] gap-x-8 gap-y-4 items-baseline">
            <dt className="text-sm text-muted-foreground">Email</dt>
            <dd className="font-display text-base md:text-lg text-foreground break-all">
              {me.email}
            </dd>
            <dt className="text-sm text-muted-foreground">User ID</dt>
            <dd className="font-mono text-xs text-muted-foreground/80 break-all">{me.id}</dd>
          </dl>
        </article>

        <article className="rounded-2xl border border-foreground/10 bg-background p-6 shadow-[0_8px_32px_-16px_rgb(28_25_23/0.18)] dark:shadow-[0_12px_40px_-18px_rgb(0_0_0/0.45)]">
          <h2 className="font-display text-xl mb-2">Session</h2>
          <p className="text-sm text-muted-foreground mb-5">
            Sign out on this device. Your campaign data stays.
          </p>
          <button
            type="button"
            onClick={() => {
              void onSignOut();
            }}
            className="inline-flex items-center justify-center rounded-lg border border-foreground/10 bg-transparent hover:bg-foreground/5 transition-colors px-4 py-2 text-sm font-medium"
          >
            Sign out
          </button>
        </article>
      </section>
    </Shell>
  );
}
