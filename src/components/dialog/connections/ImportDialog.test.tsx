import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ImportDialog from "./ImportDialog";

const handleImport = vi.fn();

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    i18n: { language: "en" },
    t: (key: string) => key,
  }),
}));

vi.mock("@/context/AppContext", () => ({
  useApp: () => ({ refreshConnections: vi.fn() }),
}));

vi.mock("@/hooks/useConfigTransfer", () => ({
  useConfigTransfer: () => ({ handleImport, passwordAlert: null }),
}));

function TestImportDialog() {
  const [open, setOpen] = useState(true);
  return <ImportDialog open={open} onClose={() => setOpen(false)} />;
}

describe("ImportDialog", () => {
  beforeEach(() => {
    handleImport.mockClear();
  });

  it("keeps backup restore separate from session import", () => {
    render(<TestImportDialog />);

    const sessionSection = screen
      .getByText("savedConnections.sessionImportTitle")
      .closest("section");
    expect(sessionSection?.textContent).toContain("savedConnections.importMergeHint");
    expect(sessionSection?.textContent).not.toContain("NyaTerm (.nya)");

    fireEvent.click(screen.getByRole("button", { name: /NyaTerm \(\.nya\)/ }));
    expect(handleImport).not.toHaveBeenCalled();
    expect(screen.getByText("savedConnections.restoreBackupConfirmDesc")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    expect(handleImport).not.toHaveBeenCalled();
  });

  it("starts backup restore only after confirmation", () => {
    render(<TestImportDialog />);
    fireEvent.click(screen.getByRole("button", { name: /NyaTerm \(\.nya\)/ }));
    expect(handleImport).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: "savedConnections.restoreBackupConfirmAction" }),
    );
    expect(handleImport).toHaveBeenCalledTimes(1);
  });
});
