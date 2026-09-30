import { describe, expect, it } from "vitest";
import { buildZmodemReceiveCommand } from "./terminalZmodemUpload";

describe("buildZmodemReceiveCommand", () => {
  it("requests control escaping for overwrite and skip uploads", () => {
    expect(buildZmodemReceiveCommand("overwrite")).toBe("rz -e -y");
    expect(buildZmodemReceiveCommand("skip")).toBe("rz -e");
  });
});
