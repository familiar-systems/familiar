import { thingIdSchema } from "@familiar-systems/types-campaign";
import { createFileRoute, useParams } from "@tanstack/react-router";
import { usePageDoc, useToc } from "../../../../../features/campaign/LoroManagerProvider";
import type { TocTreeEntry } from "../../../../../lib/loro-manager";

function ThingPage(): React.ReactElement {
  const { thingId } = Route.useParams();
  const { campaignId } = useParams({ from: "/_authed/c/$campaignId" });

  usePageDoc(thingId as string);

  const toc = useToc();
  const tocEntry =
    toc.status === "ready"
      ? findThingEntry(toc.entries, thingId as string)
      : undefined;

  return (
    <section className="mx-auto w-full max-w-3xl space-y-4 px-8 pt-12">
      <h1 className="font-display text-3xl font-medium tracking-tight">
        {tocEntry?.title ?? "Loading..."}
      </h1>
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm text-muted-foreground">
        <dt className="font-medium">Thing ID</dt>
        <dd className="font-mono text-xs">{thingId}</dd>
        <dt className="font-medium">Campaign</dt>
        <dd className="font-mono text-xs">{campaignId}</dd>
      </dl>
    </section>
  );
}

function findThingEntry(
  entries: readonly TocTreeEntry[],
  thingId: string,
): TocTreeEntry | undefined {
  for (const entry of entries) {
    if (entry.thingId === thingId) return entry;
    const found = findThingEntry(entry.children, thingId);
    if (found) return found;
  }
  return undefined;
}

export const Route = createFileRoute("/_authed/c/$campaignId/t/$thingId")({
  parseParams: ({ thingId }) => ({
    thingId: thingIdSchema.parse(thingId),
  }),
  component: ThingPage,
});
