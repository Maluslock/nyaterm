import { useRef } from "react";

export interface ShellIntegrationState {
  enabled: boolean;
  commandRunning: boolean;
}

export function useShellIntegration() {
  const shellIntegrationRef = useRef<ShellIntegrationState>({
    enabled: false,
    commandRunning: false,
  });
  return { shellIntegrationRef };
}
