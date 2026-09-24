/**
 * Decides whether a session pane shows the ordinary terminal or the tmux
 * control-mode view.
 *
 * The subscription lives here (one per session pane) so `TmuxGatewayView` stays
 * a pure function of its snapshot.
 */
import type { ReactNode } from "react";
import { useTmuxGateway } from "@/hooks/useTmuxGateway";
import { TmuxGatewayView } from "./TmuxGatewayView";

interface TmuxPaneHostProps {
  sessionId: string;
  children: ReactNode;
}

export function TmuxPaneHost({ sessionId, children }: TmuxPaneHostProps) {
  const snapshot = useTmuxGateway(sessionId);

  if (snapshot && snapshot.windows.length > 0) {
    return <TmuxGatewayView sessionId={sessionId} snapshot={snapshot} />;
  }

  return <>{children}</>;
}

export default TmuxPaneHost;
