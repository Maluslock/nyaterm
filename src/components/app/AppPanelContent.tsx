import { useRef } from "react";
import ResizeHandle from "@/components/layout/ResizeHandle";
import ActiveSessions from "@/components/panel/ActiveSessions";
import AscendNpuMonitor from "@/components/panel/AscendNpuMonitor";
import AIAssistantPanel from "@/components/panel/ai/AIAssistantPanel";
import CommandHistory from "@/components/panel/CommandHistory";
import DockerManager from "@/components/panel/DockerManager";
import FileExplorer from "@/components/panel/file-explorer";
import FileTransfer from "@/components/panel/file-explorer/FileTransfer";
import GpuMonitor from "@/components/panel/GpuMonitor";
import NetworkPanel from "@/components/panel/NetworkPanel";
import NotesPanel from "@/components/panel/notes/NotesPanel";
import ProcessManager from "@/components/panel/ProcessManager";
import RecordingPanel from "@/components/panel/RecordingPanel";
import ResourceMonitor from "@/components/panel/ResourceMonitor";
import SyncBackupHistoryPanel from "@/components/panel/SyncBackupHistoryPanel";
import SavedConnections from "@/components/panel/saved-connections";
import SecurityAuthPanel from "@/components/panel/security-auth";
import type { NetworkHistoryStore } from "@/hooks/useNetworkHistory";
import type { RemoteGpuOverviewState } from "@/hooks/useRemoteGpuOverview";
import type { RemoteNpuOverviewState } from "@/hooks/useRemoteNpuOverview";
import type { RemoteStatsState } from "@/hooks/useRemoteStats";
import type { AIOpenIntent } from "@/lib/aiEvents";
import type { NewSessionTarget } from "@/lib/windowManager";
import type {
  RecordingMode,
  RecordingStatus,
  SavedConnection,
  SessionInfo,
  SessionPane,
} from "@/types/global";

interface AppPanelContentProps {
  panelId: string | null;
  activePane: SessionPane | null;
  activeConnection: SavedConnection | null;
  activeSessionId: string | null;
  shellInputEnabled: boolean;
  activeStatsSessionId: string | null;
  remoteStatsEnabled: boolean;
  remoteStats: RemoteStatsState;
  networkHistoryStore: NetworkHistoryStore;
  gpuMonitorEnabled: boolean;
  gpuOverviewState: RemoteGpuOverviewState;
  npuMonitorEnabled: boolean;
  npuOverviewState: RemoteNpuOverviewState;
  recordingStatuses: RecordingStatus[];
  aiIntent: AIOpenIntent | null;
  transferHeight: number;
  onTransferResize: (delta: number) => void;
  onTemporarySshLink: () => void;
  onNewConnection: (parentGroupId?: string) => void;
  onEditConnection: (
    connection: SavedConnection,
    autoConnect?: boolean,
    target?: NewSessionTarget,
  ) => void;
  onConnectConnection: (connection: SavedConnection) => Promise<void> | void;
  onOpenSftpConnection: (connection: SavedConnection) => Promise<void> | void;
  onSessionClick: (sessionId: string) => void;
  onSessionReconnect: (sessionId: string) => Promise<void> | void;
  onSessionDisconnect: (sessionId: string) => Promise<void> | void;
  canReconnect: (sessionId: string) => boolean;
  onCommandSend: (command: string, execute?: boolean) => void;
  onOpenDirectoryInNewTerminal: (sessionId: string, path: string) => void;
  onToggleSessionRecording: (
    session: SessionInfo,
    mode?: RecordingMode,
  ) => Promise<void> | void;
  onSaveSessionTranscript: (session: SessionInfo) => Promise<void> | void;
}

export default function AppPanelContent({
  panelId,
  activePane,
  activeConnection,
  activeSessionId,
  shellInputEnabled,
  activeStatsSessionId,
  remoteStatsEnabled,
  remoteStats,
  networkHistoryStore,
  gpuMonitorEnabled,
  gpuOverviewState,
  npuMonitorEnabled,
  npuOverviewState,
  recordingStatuses,
  aiIntent,
  transferHeight,
  onTransferResize,
  onTemporarySshLink,
  onNewConnection,
  onEditConnection,
  onConnectConnection,
  onOpenSftpConnection,
  onSessionClick,
  onSessionReconnect,
  onSessionDisconnect,
  canReconnect,
  onCommandSend,
  onOpenDirectoryInNewTerminal,
  onToggleSessionRecording,
  onSaveSessionTranscript,
}: AppPanelContentProps) {
  const liveActivePane =
    activePane && !activePane.connecting && !activePane.connectError
      ? activePane
      : null;
  const liveTerminalPane =
    liveActivePane?.paneKind === "terminal" ? liveActivePane : null;
  const filePanelPane =
    liveActivePane?.paneKind === "terminal" ||
    liveActivePane?.paneKind === "file"
      ? liveActivePane
      : null;
  const filePanelSessionId = filePanelPane?.sessionId ?? activeSessionId;

  const aiEverMounted = useRef(false);
  if (panelId === "aiAssistant") aiEverMounted.current = true;

  const otherPanel = (() => {
    switch (panelId) {
      case "fileExplorer":
        return (
          <div className="h-full flex flex-col overflow-hidden">
            <div className="flex-1 min-h-0 overflow-hidden">
              <FileExplorer
                activeSessionId={filePanelSessionId}
                activeSessionType={filePanelPane?.type ?? null}
                activeConnectionId={filePanelPane?.connectionId ?? null}
                activeSessionName={liveTerminalPane?.name ?? null}
                terminalInputEnabled={shellInputEnabled}
                onOpenDirectoryInNewTerminal={onOpenDirectoryInNewTerminal}
              />
            </div>
            <ResizeHandle direction="vertical" onResize={onTransferResize} />
            <div
              style={{ height: transferHeight }}
              className="shrink-0 overflow-hidden"
            >
              <FileTransfer activeSessionId={filePanelSessionId} />
            </div>
          </div>
        );
      case "network":
        return <NetworkPanel />;
      case "notes":
        return <NotesPanel />;
      case "securityAuth":
        return <SecurityAuthPanel activeSessionId={activeSessionId} />;
      case "syncBackupHistory":
        return <SyncBackupHistoryPanel />;
      case "savedConnections":
        return (
          <SavedConnections
            onTemporarySshLink={onTemporarySshLink}
            onNewConnection={onNewConnection}
            onEditConnection={onEditConnection}
            onConnectConnection={onConnectConnection}
            onOpenSftpConnection={onOpenSftpConnection}
          />
        );
      case "activeSessions":
        return (
          <ActiveSessions
            onSessionClick={onSessionClick}
            onSessionReconnect={onSessionReconnect}
            onSessionDisconnect={onSessionDisconnect}
            canReconnect={canReconnect}
          />
        );
      case "recording":
        return (
          <RecordingPanel
            activeSessionId={activeSessionId}
            recordingStatuses={recordingStatuses}
            onSessionClick={onSessionClick}
            onToggleRecording={onToggleSessionRecording}
            onSaveTranscript={onSaveSessionTranscript}
          />
        );
      case "commandHistory":
        return (
          <CommandHistory
            activeSessionId={shellInputEnabled ? activeSessionId : null}
            onCommandSend={onCommandSend}
          />
        );
      case "resourceMonitor":
        return (
          <ResourceMonitor
            activeSessionId={activeStatsSessionId}
            enabled={remoteStatsEnabled}
            remoteStats={remoteStats}
            networkHistoryStore={networkHistoryStore}
          />
        );
      case "gpuMonitor":
        return (
          <GpuMonitor
            activeSessionId={activeStatsSessionId}
            enabled={gpuMonitorEnabled}
            gpuOverviewState={gpuOverviewState}
          />
        );
      case "ascendNpuMonitor":
        return (
          <AscendNpuMonitor
            activeSessionId={activeStatsSessionId}
            enabled={npuMonitorEnabled}
            npuOverviewState={npuOverviewState}
          />
        );
      case "processManager":
        return <ProcessManager activeSessionId={activeStatsSessionId} />;
      case "dockerManager":
        return <DockerManager activeSessionId={activeStatsSessionId} />;
      case "aiAssistant":
        return null;
      default:
        return null;
    }
  })();

  const isAiActive = panelId === "aiAssistant";

  return (
    <>
      {otherPanel}
      {aiEverMounted.current && (
        <div className={isAiActive ? "h-full" : "hidden"}>
          <AIAssistantPanel
            activePane={shellInputEnabled ? liveTerminalPane : null}
            activeConnection={activeConnection}
            intent={aiIntent}
          />
        </div>
      )}
    </>
  );
}
