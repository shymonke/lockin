// Builds index.html from the capture made by ../scripts/demo.ps1: one clip of the screen
// recording per scene (sped up, framed on what the scene is about), a caption per scene from
// marks.json, then the intro and end cards. Re-run after every new capture: node build.mjs
import { readFileSync, writeFileSync } from "node:fs";

const FOOTAGE = "assets/footage";
const capture = JSON.parse(readFileSync(`${FOOTAGE}/capture.json`, "utf8"));
const demo = JSON.parse(readFileSync(`${FOOTAGE}/marks.json`, "utf8"));
// FFmpeg's first frame lands a little after the capture process starts.
const LAG = Number(process.env.LAG ?? 0.5);
const offset = (Number(demo.start_unix_ms) - Number(capture.capture_start_unix_ms)) / 1000 - LAG;

const W = 1920, H = 1080;
const INTRO = 3.2, END = 4;
const { ppp, monitor, pill, pill_size, dash: dashPx } = demo.layout;
// Capture pixels → frame pixels (contain fit).
const k = Math.min(W / capture.width, H / capture.height);
const ox = (W - capture.width * k) / 2, oy = (H - capture.height * k) / 2;
const px = (r) => ({ x: ox + r.x * ppp * k, y: oy + r.y * ppp * k, w: r.w * ppp * k, h: r.h * ppp * k });

// What each scene is about, in screen points (the demo's own layout).
const pillRect = { x: pill[0], y: pill[1], w: pill_size[0], h: pill_size[1] };
const alertRect = { x: (monitor[0] - 440) / 2, y: 84, w: 440, h: 410 };
const union = (a, b) => {
  const x = Math.min(a.x, b.x), y = Math.min(a.y, b.y);
  return { x, y, w: Math.max(a.x + a.w, b.x + b.w) - x, h: Math.max(a.y + a.h, b.y + b.h) - y };
};
// Main window where it really was (physical pixels in the marks → points).
const dash = dashPx
  ? { x: dashPx[0] / ppp, y: dashPx[1] / ppp, w: dashPx[2] / ppp, h: dashPx[3] / ppp }
  : { x: (monitor[0] - 1040) / 2, y: (monitor[1] - 780) / 2, w: 1040, h: 780 };
// Whole screen minus the taskbar strip, which may show while the main window has focus.
const full = { x: 0, y: 0, w: monitor[0], h: monitor[1] - 48, tight: true };

// Per scene, in mark order: framing, playback speed.
const SHOTS = [
  { focus: pillRect, rate: 1.5 }, // on top of everything
  { focus: pillRect, rate: 1.5 }, // quick add
  { focus: pillRect, rate: 1.6 }, // themes
  { focus: pillRect, rate: 1.5 }, // folds into a clock
  { focus: pillRect, rate: 1.4 }, // hover
  { focus: union(pillRect, alertRect), rate: 1.5 }, // nudge
  { focus: union(pillRect, alertRect), rate: 1.5 }, // alarm
  { focus: pillRect, rate: 1.6 }, // step away
  { focus: full, rate: 1.6 }, // calculator
  { focus: pillRect, rate: 1.5 }, // camera
  { focus: { x: dash.x, y: dash.y + 0.06 * dash.h, w: 0.56 * dash.w, h: 0.5 * dash.h }, rate: 1.4 }, // gaze tracking: the camera panel
  { focus: dash, rate: 1.5 }, // tasks, ideas, year
  { focus: { x: dash.x + dash.w - 580, y: dash.y + 30, w: 580, h: 0.6 * dash.h }, rate: 1.4 }, // rebind: the settings sheet
  { focus: { x: dash.x + dash.w - 580, y: dash.y + 30, w: 580, h: 0.6 * dash.h }, rate: 1.4 }, // timer bar settings
  { focus: full, rate: 2.2, maxLen: 13 }, // calibration (the main window returns after ~12 s)
];

// Frame pixels where the taskbar starts; no framing ever shows below it.
const floor = oy + (monitor[1] - 48) * ppp * k;
const minScale = H / floor;
const fit = (s, cx, cy) => ({
  s: +s.toFixed(3),
  x: Math.round(Math.min(0, Math.max(W - W * s, W / 2 - cx * s))),
  y: Math.round(Math.min(0, Math.max(H - floor * s, H / 2 - cy * s))),
});

/** Scale and offset that fill the frame with `focus` (plus a margin unless `tight`). */
function frame(focus) {
  const r = px(focus);
  const [mx, my] = focus.tight ? [1, 1] : [1.25, 1.35];
  const s = Math.max(minScale, Math.min(3, W / (r.w * mx), H / (r.h * my)));
  return fit(s, r.x + r.w / 2, r.y + r.h / 2);
}

const scenes = demo.marks.slice(0, -1).map((m, i) => {
  const shot = SHOTS[i] ?? { focus: full, rate: 1.5 };
  const len = Math.min(demo.marks[i + 1].t - m.t, shot.maxLen ?? Infinity);
  // Slow a scene down if its caption would not stay up long enough to read (0.5 s + 0.3 s a word).
  const read = 0.8 + 0.3 * m.text.split(" ").length;
  const rate = +Math.min(shot.rate, len / read).toFixed(3);
  return { i, text: m.text, mediaStart: m.t + offset, rate, dur: len / rate, cam: frame(shot.focus) };
});
let t = INTRO;
for (const s of scenes) { s.start = +t.toFixed(3); t += s.dur; }
const total = +(t + END).toFixed(3);
const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;");
const words = (s) => s.split(" ").map((w) => `<span class="w">${esc(w)}</span>`).join(" ");

const clips = scenes.map((s) => `
      <div id="cam${s.i}" class="cam">
        <video id="shot${s.i}" class="clip" src="${FOOTAGE}/raw.mp4" muted playsinline
          data-start="${s.start}" data-duration="${s.dur.toFixed(3)}" data-media-start="${s.mediaStart.toFixed(3)}"
          data-playback-rate="${s.rate}" data-track-index="1"></video>
      </div>`).join("");
const captions = scenes.map((s) => `
      <p id="cap${s.i}" class="clip caption" data-start="${s.start}" data-duration="${s.dur.toFixed(3)}" data-track-index="3"><i class="dot"></i>${words(s.text)}</p>`).join("");

// Camera: each scene's wrapper eases into its framing from a slightly wider one on the same centre.
const widen = ({ s, x, y }) => {
  const s2 = Math.max(minScale, s * 0.92);
  return fit(s2, (W / 2 - x) / s, (H / 2 - y) / s);
};
const camTweens = scenes.map((s) => {
  const from = widen(s.cam);
  return `      tl.fromTo("#cam${s.i}", { scale: ${from.s}, x: ${from.x}, y: ${from.y} }, { scale: ${s.cam.s}, x: ${s.cam.x}, y: ${s.cam.y}, duration: 0.6, ease: "power2.inOut" }, ${s.start});`;
}).join("\n");
const capTweens = scenes.map((s) =>
  `      tl.fromTo("#cap${s.i} .w, #cap${s.i} .dot", { opacity: 0, filter: "blur(6px)" }, { opacity: 1, filter: "blur(0px)", duration: 0.13, stagger: 0.067, ease: "power4.out" }, ${(s.start + 0.15).toFixed(3)});`
).join("\n");

const html = `<!doctype html>
<!-- Generated by build.mjs from assets/footage; edit build.mjs, then run: node build.mjs -->
<html lang="en" data-resolution="landscape">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=1920, height=1080" />
    <script src="https://cdn.jsdelivr.net/npm/gsap@3.15.0/dist/gsap.min.js"></script>
    <style>
      :root { --ground: #14171d; --ink: #e4e7ec; --muted: #8a93a3; --focus: #8cc7a5; --gaze: #9bb0f0; }
      * { margin: 0; padding: 0; box-sizing: border-box; }
      html, body { width: 1920px; height: 1080px; overflow: hidden; background: var(--ground); }
      #root { position: relative; width: 100%; height: 100%; font-family: Inter, sans-serif; color: var(--ink); }
      .cam { position: absolute; left: 0; top: 0; width: 1920px; height: 1080px; transform-origin: 0 0; }
      .cam video { position: absolute; left: 0; top: 0; width: 1920px; height: 1080px; object-fit: contain; }
      .caption { position: absolute; left: 120px; bottom: 96px; max-width: 1500px; z-index: 5; font-size: 58px; font-weight: 700;
        letter-spacing: -0.02em; line-height: 1.1; text-shadow: 0 2px 18px rgba(0, 0, 0, .55); }
      .caption .dot { display: inline-block; width: 16px; height: 16px; border-radius: 50%; background: var(--focus); margin-right: 22px; vertical-align: 8px; }
      /* Keeps captions legible over busy UI; only behind the footage, not the cards. */
      #scrim { position: absolute; left: 0; right: 0; bottom: 0; height: 340px; z-index: 4;
        background: linear-gradient(to bottom, rgba(12, 14, 19, 0), rgba(12, 14, 19, .88)); }
    </style>
  </head>
  <body>
    <div id="root" data-composition-id="main" data-start="0" data-duration="${total}" data-width="1920" data-height="1080">
      <div id="intro" data-composition-id="intro" data-composition-src="compositions/intro.html" data-start="0" data-duration="${INTRO}" data-track-index="0" data-track-kind="graphics" data-width="1920" data-height="1080"></div>
${clips}
      <div id="scrim" class="clip" data-start="${INTRO}" data-duration="${(total - END - INTRO).toFixed(3)}" data-track-index="2"></div>
${captions}
      <div id="end" data-composition-id="end" data-composition-src="compositions/end.html" data-start="${(total - END).toFixed(3)}" data-duration="${END}" data-track-index="4" data-track-kind="graphics" data-width="1920" data-height="1080"></div>
    </div>
    <script>
      const tl = gsap.timeline({ paused: true });
${camTweens}
${capTweens}
      window.__timelines["main"] = tl;
    </script>
  </body>
</html>
`;
writeFileSync("index.html", html);
console.log(`index.html: ${scenes.length} scenes, ${total}s (offset ${offset.toFixed(2)}s)`);
