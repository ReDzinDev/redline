import { MESSAGES } from "../lib/messages";

export function PopupApp() {
  return (
    <main className="flex h-full items-center justify-center bg-canvas p-4 text-sm text-ink-secondary">
      {MESSAGES.popup.capturing}
    </main>
  );
}
