/**
 * Marks <html data-idle="true"> after a quiet period with no pointer, keyboard or focus
 * activity, and false again on the next interaction. CSS uses it to rest ambient motion.
 */
export const IDLE_AFTER_MS = 15_000;

export function trackIdle(doc: Document = document, idleAfter = IDLE_AFTER_MS): () => void {
  const html = doc.documentElement;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const wake = () => {
    if (html.dataset.idle !== "false") html.dataset.idle = "false";
    clearTimeout(timer);
    timer = setTimeout(() => {
      html.dataset.idle = "true";
    }, idleAfter);
  };
  const events = ["pointermove", "pointerdown", "keydown", "wheel", "focus"] as const;
  for (const e of events) doc.addEventListener(e, wake, { passive: true, capture: true });
  wake();
  return () => {
    clearTimeout(timer);
    for (const e of events) doc.removeEventListener(e, wake, { capture: true });
  };
}
