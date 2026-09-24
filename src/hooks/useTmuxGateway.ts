/**
 * Subscribes to tmux control-mode gateway state for one session.
 *
 * Returns `null` while the session is an ordinary terminal, and the current
 * snapshot once `tmux -CC` has been detected on that session.
 */
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import {
  fetchTmuxSnapshot,
  type TmuxGatewaySnapshot,
  tmuxStateEvent,
} from "@/lib/tmuxGateway";

export function useTmuxGateway(
  sessionId: string | null | undefined,
): TmuxGatewaySnapshot | null {
  const [snapshot, setSnapshot] = useState<TmuxGatewaySnapshot | null>(null);

  useEffect(() => {
    if (!sessionId) {
      setSnapshot(null);
      return;
    }

    let disposed = false;
    setSnapshot(null);

    // The gateway may already be active (e.g. a remounted view), so pull once
    // as well as subscribing.
    void fetchTmuxSnapshot(sessionId)
      .then((initial) => {
        if (!disposed && initial) setSnapshot(initial);
      })
      .catch(() => {
        // A session without a gateway is the normal case; nothing to report.
      });

    let unlisten: (() => void) | null = null;
    void listen<TmuxGatewaySnapshot>(tmuxStateEvent(sessionId), (event) => {
      if (!disposed) setSnapshot(event.payload);
    })
      .then((dispose) => {
        if (disposed) dispose();
        else unlisten = dispose;
      })
      .catch(() => {});

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [sessionId]);

  return snapshot;
}
