import { describe, expect, it } from "vitest";
import {
  formatCostUsd,
  formatElapsedSeconds,
  formatSourceAppLabel,
  isEditableTarget,
  resolveEnterAction,
  shouldIgnoreGlobalEnter,
} from "./popupLogic";

describe("resolveEnterAction (section 4 table)", () => {
  it("does nothing while streaming", () => {
    expect(
      resolveEnterAction({
        phase: "ready",
        aiStatus: "streaming",
        aiUnchanged: false,
        harperFixesApplied: true,
      }),
    ).toBe("noop");
  });

  it("does nothing on AI error or cut-off and never falls back to Harper fixes", () => {
    expect(
      resolveEnterAction({
        phase: "ready",
        aiStatus: "error",
        aiUnchanged: false,
        harperFixesApplied: true,
      }),
    ).toBe("noop");

    expect(
      resolveEnterAction({
        phase: "ready",
        aiStatus: "cut_off",
        aiUnchanged: false,
        harperFixesApplied: true,
      }),
    ).toBe("noop");
  });

  it("pastes complete AI result when different from input", () => {
    expect(
      resolveEnterAction({
        phase: "ready",
        aiStatus: "complete",
        aiUnchanged: false,
        harperFixesApplied: false,
      }),
    ).toBe("paste_ai");
  });

  it("closes when complete AI result equals the input", () => {
    expect(
      resolveEnterAction({
        phase: "ready",
        aiStatus: "complete",
        aiUnchanged: true,
        harperFixesApplied: true,
      }),
    ).toBe("close");
  });

  it("pastes Harper-fixed text when no AI result exists and fixes were applied", () => {
    expect(
      resolveEnterAction({
        phase: "ready",
        aiStatus: "idle",
        aiUnchanged: false,
        harperFixesApplied: true,
      }),
    ).toBe("paste_fixed");
  });

  it("closes when nothing changed", () => {
    expect(
      resolveEnterAction({
        phase: "ready",
        aiStatus: "idle",
        aiUnchanged: false,
        harperFixesApplied: false,
      }),
    ).toBe("close");
  });

  it("does nothing while capturing or in capture error", () => {
    expect(
      resolveEnterAction({
        phase: "capturing",
        aiStatus: "idle",
        aiUnchanged: false,
        harperFixesApplied: false,
      }),
    ).toBe("noop");
    expect(
      resolveEnterAction({
        phase: "error",
        aiStatus: "idle",
        aiUnchanged: false,
        harperFixesApplied: false,
      }),
    ).toBe("noop");
  });
});

describe("keyboard guards and formatting", () => {
  it("detects editable targets and global Enter exclusions", () => {
    expect(isEditableTarget("INPUT", false)).toBe(true);
    expect(isEditableTarget("textarea", false)).toBe(true);
    expect(isEditableTarget("div", true)).toBe(true);
    expect(isEditableTarget("BUTTON", false)).toBe(false);

    expect(shouldIgnoreGlobalEnter("BUTTON", false, false)).toBe(true);
    expect(shouldIgnoreGlobalEnter("DIV", false, true)).toBe(true);
    expect(shouldIgnoreGlobalEnter("DIV", false, false)).toBe(false);
  });

  it("formats cost, duration, and process names cleanly", () => {
    expect(formatCostUsd(0.00002)).toBe("$0.00002");
    expect(formatCostUsd(0)).toBe("$0");
    expect(formatCostUsd(null)).toBe("");
    expect(formatElapsedSeconds(620)).toBe("0.6 s");
    expect(formatSourceAppLabel("Slack.exe")).toBe("Slack");
    expect(formatSourceAppLabel("notepad.exe")).toBe("Notepad");
  });
});
