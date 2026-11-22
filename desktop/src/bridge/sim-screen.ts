/**
 * Browser preview only: draws a clearly labelled *simulated* phone screen, so the
 * mirror view can be designed and tested without a phone. The real app shows the
 * phone's actual screen, streamed by the Devti Android app.
 */
export interface SimScreen {
  name: string;
  touches: { x: number; y: number; t: number; long: boolean }[];
  typed: string;
  app: string | null;
  timer: number;
}

const APPS = ["Camera", "Chrome", "Clock", "Maps", "Messages", "Photos", "Spotify", "Settings"];
const HUES = [262, 192, 12, 158, 42, 330, 220, 100];

export function drawSimulatedPhone(canvas: HTMLCanvasElement, sim: SimScreen, width: number) {
  const w = Math.round(width);
  const h = Math.round(w * 2.16);
  if (canvas.width !== w) canvas.width = w;
  if (canvas.height !== h) canvas.height = h;
  const g = canvas.getContext("2d");
  if (!g) return;
  const u = w / 360;
  const grad = g.createLinearGradient(0, 0, w, h);
  grad.addColorStop(0, "#241d4a");
  grad.addColorStop(1, "#0d3b46");
  g.fillStyle = grad;
  g.fillRect(0, 0, w, h);

  const now = new Date();
  g.fillStyle = "#f2f0fb";
  g.font = `${600} ${13 * u}px system-ui, sans-serif`;
  g.fillText(now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }), 18 * u, 26 * u);
  g.textAlign = "right";
  g.fillText("84 %", w - 18 * u, 26 * u);
  g.textAlign = "left";

  if (sim.app) {
    const name = APPS.find((a) => sim.app?.endsWith(a.toLowerCase())) ?? sim.app;
    g.fillStyle = "rgba(255,255,255,0.08)";
    g.fillRect(0, 44 * u, w, h - 44 * u);
    g.fillStyle = "#f2f0fb";
    g.font = `${700} ${30 * u}px system-ui, sans-serif`;
    g.fillText(name, 24 * u, 110 * u);
    g.font = `${400} ${14 * u}px system-ui, sans-serif`;
    g.fillStyle = "#aaa4ca";
    g.fillText("Simulated app — opened from the Hub", 24 * u, 136 * u);
  } else {
    g.font = `${700} ${64 * u}px system-ui, sans-serif`;
    g.fillStyle = "#f2f0fb";
    g.fillText(now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }), 24 * u, 150 * u);
    const cols = 4;
    const cell = (w - 40 * u) / cols;
    APPS.forEach((a, i) => {
      const cx = 20 * u + (i % cols) * cell + cell / 2;
      const cy = 300 * u + Math.floor(i / cols) * 110 * u;
      g.fillStyle = `oklch(0.72 0.14 ${HUES[i]})`;
      g.beginPath();
      g.arc(cx, cy, 26 * u, 0, Math.PI * 2);
      g.fill();
      g.fillStyle = "#f2f0fb";
      g.font = `${500} ${12 * u}px system-ui, sans-serif`;
      g.textAlign = "center";
      g.fillText(a, cx, cy + 46 * u);
      g.textAlign = "left";
    });
  }

  // A text field that shows what the Hub's keyboard typed.
  g.fillStyle = "rgba(255,255,255,0.12)";
  roundRect(g, 20 * u, h - 150 * u, w - 40 * u, 44 * u, 22 * u);
  g.fill();
  g.fillStyle = sim.typed ? "#f2f0fb" : "#aaa4ca";
  g.font = `${400} ${15 * u}px system-ui, sans-serif`;
  g.fillText(sim.typed || "Type on your computer's keyboard…", 36 * u, h - 122 * u);

  // Where the Hub tapped.
  for (const t of sim.touches) {
    const age = (Date.now() - t.t) / 900;
    if (age > 1) continue;
    g.strokeStyle = `rgba(255,255,255,${1 - age})`;
    g.lineWidth = 3 * u;
    g.beginPath();
    g.arc(t.x * w, t.y * h, (t.long ? 34 : 18) * u * (1 + age), 0, Math.PI * 2);
    g.stroke();
  }

  g.fillStyle = "rgba(0,0,0,0.45)";
  roundRect(g, w / 2 - 92 * u, h - 70 * u, 184 * u, 30 * u, 15 * u);
  g.fill();
  g.fillStyle = "#ffcb62";
  g.font = `${600} ${12 * u}px system-ui, sans-serif`;
  g.textAlign = "center";
  g.fillText("SIMULATED SCREEN · PREVIEW", w / 2, h - 50 * u);
  g.textAlign = "left";
}

function roundRect(g: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  g.beginPath();
  g.moveTo(x + r, y);
  g.arcTo(x + w, y, x + w, y + h, r);
  g.arcTo(x + w, y + h, x, y + h, r);
  g.arcTo(x, y + h, x, y, r);
  g.arcTo(x, y, x + w, y, r);
  g.closePath();
}
