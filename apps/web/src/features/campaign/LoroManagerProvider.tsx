import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import {
  LoroClientManager,
  type TocSnapshot,
} from "../../lib/loro-manager";
import type { LoroDoc } from "loro-crdt";

const LoroManagerContext = createContext<LoroClientManager | null>(null);

interface LoroManagerProviderProps {
  wsUrl: string;
  children: ReactNode;
}

export function LoroManagerProvider({
  wsUrl,
  children,
}: LoroManagerProviderProps): React.ReactElement {
  const manager = useMemo(() => new LoroClientManager(wsUrl), [wsUrl]);

  useEffect(() => {
    manager.connect();
    return () => {
      manager.close();
    };
  }, [manager]);

  return (
    <LoroManagerContext.Provider value={manager}>
      {children}
    </LoroManagerContext.Provider>
  );
}

export function useLoroManager(): LoroClientManager {
  const manager = useContext(LoroManagerContext);
  if (!manager) {
    throw new Error("useLoroManager must be used within a LoroManagerProvider");
  }
  return manager;
}

export function useToc(): TocSnapshot {
  const manager = useLoroManager();
  return useSyncExternalStore(manager.subscribeToc, manager.getTocSnapshot);
}

export function usePageDoc(pageId: string): LoroDoc | null {
  const manager = useLoroManager();

  useEffect(() => {
    manager.acquirePage(pageId);
    return () => {
      manager.releasePage(pageId);
    };
  }, [manager, pageId]);

  return useSyncExternalStore(
    (listener) => manager.subscribePageDoc(pageId, listener),
    () => manager.getPageDoc(pageId),
  );
}
