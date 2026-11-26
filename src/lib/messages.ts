/**
 * Centralized user-facing copy.
 * Rust returns structured error codes; all wording lives here so tone stays consistent.
 */
export const MESSAGES = {
  app: {
    name: "Redline",
    tagline: "Lightweight writing assistant running in your system tray.",
  },
  main: {
    heading: "Redline",
    status: "Milestone 0 scaffold active. Close this window to return to the system tray.",
  },
  popup: {
    capturing: "Capturing…",
  },
} as const;
