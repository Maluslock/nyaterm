import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { RemoteGpuOverviewState } from "@/hooks/useRemoteGpuOverview";
import type { RemoteNpuOverviewState } from "@/hooks/useRemoteNpuOverview";
import type { RemoteStatsState } from "@/hooks/useRemoteStats";
import type { FileDocumentPane } from "@/types/global";
import AppPanelContent from "./AppPanelContent";

const { fileExplorerMock, fileTransferMock } = vi.hoisted(() => ({
  fileExplorerMock: vi.fn(),
  fileTransferMock: vi.fn(),
}));

vi.mock("@/components/panel/file-explorer", () => ({
  default: (props: unknown) => {
    fileExplorerMock(props);
    return null;
  },
}));

vi.mock("@/components/panel/file-explorer/FileTransfer", () => ({
  default: (props: unknown) => {
    fileTransferMock(props);
    return null;
  },
}));

function renderFileExplorer(activePane: FileDocumentPane, onOpenDirectoryInNewTerminal = vi.fn()) {
  return render(
    <AppPanelContent
      panelId="fileExplorer"
      activePane={activePane}
      activeConnection={null}
      activeSessionId={null}
      shellInputEnabled
      activeStatsSessionId={null}
      remoteStatsEnabled={false}
      remoteStats={{} as RemoteStatsState}
      networkHistoryStore={{
        getSeries: () => ({ summary: [], interfaces: {} }),
        subscribe: () => () => {},
      }}
      gpuMonitorEnabled={false}
      gpuOverviewState={{} as RemoteGpuOverviewState}
      npuMonitorEnabled={false}
      npuOverviewState={{} as RemoteNpuOverviewState}
      recordingStatuses={[]}
      aiIntent={null}
      transferHeight={180}
      onTransferResize={vi.fn()}
      onTemporarySshLink={vi.fn()}
      onNewConnection={vi.fn()}
      onEditConnection={vi.fn()}
      onConnectConnection={vi.fn()}
      onOpenSftpConnection={vi.fn()}
      onSessionClick={vi.fn()}
      onSessionReconnect={vi.fn()}
      onSessionDisconnect={vi.fn()}
      canReconnect={() => false}
      onCommandSend={vi.fn()}
      onOpenDirectoryInNewTerminal={onOpenDirectoryInNewTerminal}
      onToggleSessionRecording={vi.fn()}
      onSaveSessionTranscript={vi.fn()}
    />,
  );
}

describe("AppPanelContent file explorer", () => {
  it("keeps Files and Transfers attached to the session used by an active file pane", () => {
    const pane: FileDocumentPane = {
      id: "pane-file",
      kind: "leaf",
      paneKind: "file",
      sessionId: "session-1",
      name: "notes.txt",
      type: "SSH",
      connectionId: "connection-1",
      file: {
        backend: "remote",
        path: "/tmp/notes.txt",
        initial: {
          content: "notes",
          size: 5,
          mtime: 1,
          contentHash: "hash-notes",
        },
      },
    };

    const onOpenDirectoryInNewTerminal = vi.fn();
    renderFileExplorer(pane, onOpenDirectoryInNewTerminal);

    expect(fileExplorerMock).toHaveBeenCalledWith(
      expect.objectContaining({
        activeSessionId: "session-1",
        activeSessionType: "SSH",
        activeConnectionId: "connection-1",
        activeSessionName: null,
        onOpenDirectoryInNewTerminal,
      }),
    );
    expect(fileTransferMock).toHaveBeenCalledWith(
      expect.objectContaining({ activeSessionId: "session-1" }),
    );
  });
});
