// Read-only subscription to the campaign's table of contents. The "toc" room is
// always-on in LoroClientManager (joined on connect, torn down on close), so this
// hook has no acquire/release: it just reads the derived snapshot via
// useSyncExternalStore. Mirrors useThingDoc minus the per-room lifecycle.

import { useSyncExternalStore } from "react";

import type { TocSnapshot } from "../editor/loro-manager";
import { useLoroManager } from "../editor/LoroManagerProvider";

export function useToc(): TocSnapshot {
  const manager = useLoroManager();
  // subscribeToc / getTocSnapshot are stable bound fields on the manager instance
  // (itself stable for the campaign mount), so they can be passed directly.
  return useSyncExternalStore(manager.subscribeToc, manager.getTocSnapshot);
}
