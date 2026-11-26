import { MESSAGES } from "../lib/messages";

export function MainApp() {
  return (
    <main className="flex h-full flex-col justify-between bg-canvas p-6 text-ink">
      <section className="space-y-2">
        <h1 className="text-xl font-semibold tracking-tight text-ink">{MESSAGES.main.heading}</h1>
        <p className="text-sm text-ink-secondary">{MESSAGES.app.tagline}</p>
      </section>
      <footer className="rounded-md border border-line bg-elevated px-4 py-3 text-xs text-ink-muted">
        {MESSAGES.main.status}
      </footer>
    </main>
  );
}
