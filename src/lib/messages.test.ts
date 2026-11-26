import { describe, expect, it } from "vitest";
import { formatApiError, formatCaptureError, MESSAGES } from "./messages";

describe("formatCaptureError", () => {
  it("maps every capture error code to plain-word user copy", () => {
    expect(formatCaptureError("nothing_selected")).toBe(MESSAGES.captureErrors.nothing_selected);
    expect(formatCaptureError("elevated_target")).toBe(MESSAGES.captureErrors.elevated_target);
    expect(formatCaptureError("selection_too_long")).toBe(
      MESSAGES.captureErrors.selection_too_long,
    );
    expect(formatCaptureError("clipboard_busy")).toBe(MESSAGES.captureErrors.clipboard_busy);
    expect(formatCaptureError("unknown_code")).toBe(MESSAGES.captureErrors.default);
  });
});

describe("formatApiError", () => {
  it("maps all OpenRouter AI error codes with the spec next steps", () => {
    expect(formatApiError({ code: "ai_no_key", detail: null, retryAfterSecs: null }).nextStep).toBe(
      "settings",
    );
    expect(
      formatApiError({ code: "ai_bad_request", detail: "Bad model", retryAfterSecs: null })
        .nextStep,
    ).toBe("settings");
    expect(
      formatApiError({ code: "ai_unauthorized", detail: null, retryAfterSecs: null }).nextStep,
    ).toBe("settings");
    expect(
      formatApiError({ code: "ai_payment_required", detail: "No credits", retryAfterSecs: null })
        .nextStep,
    ).toBe("credits");
    expect(
      formatApiError({ code: "ai_forbidden", detail: "Blocked", retryAfterSecs: null }).nextStep,
    ).toBe("try_another_model");
    expect(
      formatApiError({ code: "ai_no_provider", detail: null, retryAfterSecs: null }).nextStep,
    ).toBe("settings");
    expect(
      formatApiError({ code: "ai_bad_gateway", detail: null, retryAfterSecs: null }).nextStep,
    ).toBe("retry_or_model");
    expect(
      formatApiError({ code: "ai_network", detail: null, retryAfterSecs: null }).nextStep,
    ).toBe("retry");
    expect(
      formatApiError({ code: "ai_cut_off", detail: null, retryAfterSecs: null }).nextStep,
    ).toBe("retry");
    expect(
      formatApiError({ code: "ai_mid_stream", detail: null, retryAfterSecs: null }).nextStep,
    ).toBe("retry");
    expect(formatApiError({ code: "ai_empty", detail: null, retryAfterSecs: null }).nextStep).toBe(
      "retry",
    );
  });

  it("formats 429 rate-limit messages with and without Retry-After", () => {
    const withSeconds = formatApiError({
      code: "ai_rate_limited",
      detail: null,
      retryAfterSecs: 15,
    });
    expect(withSeconds.headline).toContain("15 s");
    expect(withSeconds.nextStep).toBe("retry");

    const withoutSeconds = formatApiError({
      code: "ai_rate_limited",
      detail: null,
      retryAfterSecs: null,
    });
    expect(withoutSeconds.headline).toContain("in a moment");
    expect(withoutSeconds.nextStep).toBe("retry");
  });

  it("maps non-AI system error codes without falling back to generic prose", () => {
    const codes = [
      "selection_too_long",
      "clipboard_busy",
      "paste_failed",
      "invalid_model",
      "invalid_action",
      "hotkey_conflict",
      "secret_store_error",
      "settings_save_failed",
      "autostart_error",
      "history_error",
      "opener_error",
      "lint_failed",
    ];
    for (const code of codes) {
      const formatted = formatApiError({ code, detail: "detail", retryAfterSecs: null });
      expect(formatted.headline).not.toBe("Something went wrong.");
      expect(formatted.headline.length).toBeGreaterThan(0);
    }
  });
});
