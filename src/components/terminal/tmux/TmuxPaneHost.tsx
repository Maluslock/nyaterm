/**
 * Decides whether a session pane shows the ordinary terminal or the tmux
 * control-mode view.
 *
 * The subscription lives here (one per session pane) so `TmuxGatewayView` stays
 * a pure function of its snapshot.
 */
import type { ReactNode } from "react";
import { useEffect, useRef } from "react";
import { useTmuxGateway } from "@/hooks/useTmuxGateway";
import { isGatewayExited } from "@/lib/tmuxGateway";
import { TmuxGatewayView } from "./TmuxGatewayView";

interface TmuxPaneHostProps {
  sessionId: string;
  children: ReactNode;
}

export function TmuxPaneHost({ sessionId, children }: TmuxPaneHostProps) {
  const snapshot = useTmuxGateway(sessionId);
  // A tmux client that exited (detach, or quitting the last shell inside it)
  // hands the session back to its shell, so the ordinary terminal takes over.
  const showTmux = Boolean(
    snapshot && snapshot.windows.length > 0 && !isGatewayExited(snapshot),
  );

  // `inert` drops whatever focus the terminal had when the tmux view appears, so
  // taking the view away again must hand the keyboard back: otherwise detaching
  // costs the user a click before they can type.
  const plainSurfaceRef = useRef<HTMLDivElement | null>(null);
  const wasShowingTmux = useRef(false);
  useEffect(() => {
    if (showTmux) {
      wasShowingTmux.current = true;
      return;
    }
    if (!wasShowingTmux.current) return;
    wasShowingTmux.current = false;
    const surface = plainSurfaceRef.current;
    const input =
      surface?.querySelector<HTMLTextAreaElement>("textarea.xterm-helper-textarea") ??
      surface?.querySelector<HTMLTextAreaElement>("textarea");
    input?.focus();
  }, [showTmux]);

  return (
    <div className="relative h-full w-full min-h-0">
      {/*
        The ordinary terminal stays mounted under the tmux view, `inert` so it
        cannot take focus while covered. Staying mounted is what makes detaching
        usable: it keeps consuming session output, so the shell prompt printed
        right after tmux exits is already in its buffer when the view unmounts.
      */}
      <div ref={plainSurfaceRef} className="absolute inset-0" inert={showTmux}>
        {children}
      </div>
      {showTmux && snapshot ? (
        <div className="absolute inset-0">
          <TmuxGatewayView sessionId={sessionId} snapshot={snapshot} />
        </div>
      ) : null}
    </div>
  );
}

export default TmuxPaneHost;
