"use strict";
// LockIn dashboard. Talks to the Rust host through:
//   GET  state            -> app snapshot (JSON)
//   GET  preview          -> { seq, w, h, overlay }   (camera overlay, normalised coords, not mirrored)
//   GET  frame?seq=N      -> latest camera JPEG
//   window.ipc.postMessage(JSON) -> actions

const $ = (s) => document.querySelector(s);
const send = (cmd, extra = {}) => window.ipc.postMessage(JSON.stringify({ cmd, ...extra }));
const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
let S = null; // last state

const fmt = (secs) => {
  const s = Math.max(0, Math.ceil(secs));
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
};
const mins = (secs) => {
  const m = Math.floor(secs / 60);
  return m >= 60 ? `${Math.floor(m / 60)}h ${m % 60}m` : `${m}m`;
};

function toast(text) {
  const t = $("#toast");
  t.textContent = text;
  t.hidden = false;
  requestAnimationFrame(() => t.classList.add("show"));
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => {
    t.classList.remove("show");
    setTimeout(() => (t.hidden = true), 260);
  }, 2600);
}

/* ------------------------------------------------------------ state */

async function poll() {
  try {
    const r = await fetch("state", { cache: "no-store" });
    S = await r.json();
    render();
    if (!S.tour_done && !tour.active && !poll.tourShown) {
      poll.tourShown = true;
      setTimeout(() => tour.start(), 600);
    }
  } catch (_) { /* host restarting */ }
  setTimeout(poll, 250);
}

function render() {
  const t = S.timer;
  const focus = t.phase === "focus";
  $("#time").textContent = fmt(t.remaining);
  $("#time").classList.toggle("held", t.held);
  const session = $(".session");
  session.dataset.phase = focus ? "focus" : "break";
  session.dataset.running = String(t.running && t.brb === null);
  $("#phase").textContent = t.brb !== null
    ? (t.brb >= 0 ? `Away, back in ${fmt(t.brb)}` : `Away ${fmt(-t.brb)} too long`)
    : focus ? (t.held ? "Focus, paused while distracted" : "Focus") : t.phase === "long" ? "Long break" : "Short break";
  $("#round").textContent = focus ? `Round ${t.round} of ${t.every}` : "";
  $("#penalty").textContent = t.added > 0 && focus ? `+${mins(t.added)} penalty` : "";
  setButton($("#start"), t.running ? "pause" : "play", t.running ? "Pause" : (t.started ? "Resume" : (focus ? "Start focus" : "Start break")));
  const brb = $("#brb");
  const back = t.brb !== null || S.alarm;
  setButton(brb, back ? "back" : "coffee", back ? "I'm back" : "Step away");
  brb.disabled = !(t.brb !== null || S.alarm || (t.running && focus));
  const brbKeys = (S.hotkeys.find((h) => h.id === "brb") || {}).keys;
  const pause = `Pause for ${Math.round(S.settings.brb_secs / 60 * 10) / 10} min without alarms`;
  brb.title = t.brb !== null || S.alarm ? brbKeys || "" : brbKeys ? `${pause} (${brbKeys})` : pause;

  const d = S.today;
  $("#t-focus").textContent = mins(d.focus_secs);
  $("#t-goal").style.width = `${Math.min(100, (d.focus_secs / 60 / Math.max(1, d.goal_min)) * 100)}%`;
  $("#t-score").textContent = d.score;
  $("#t-streak").textContent = d.clean_streak;
  $("#t-dist").textContent = d.distractions;

  const sel = $("#camera");
  const key = S.cam.cameras.join("|");
  if (sel.dataset.key !== key) {
    sel.dataset.key = key;
    sel.innerHTML = S.cam.cameras.length ? S.cam.cameras.map((c, i) => `<option value="${i}">${esc(c)}</option>`).join("") : `<option>No camera found</option>`;
  }
  if (document.activeElement !== sel) sel.value = String(S.settings.camera_index);

  renderTasks();
  cal.update();
  if (sheet.open) sheet.refresh();
  recorder.poll();
  feedback.update();
  // `lockin --demo` opens and closes the settings sheet on its timeline.
  if ((S.demo_sheet ?? null) !== render.demoSheet) {
    render.demoSheet = S.demo_sheet ?? null;
    if (render.demoSheet) sheet.show(render.demoSheet);
    else if (sheet.open) sheet.hide();
  }
  if (!drawing) updateChip();
}

// Phosphor glyphs (assets/fonts/phosphor-lockin.ttf).
const GLYPH = { play: "\ue3d0", pause: "\ue39e", coffee: "\ue1c2", back: "\ue08a", x: "\ue4f6" };
function setButton(el, glyph, text) {
  const i = el.querySelector(".ph"), span = el.querySelector("span");
  if (i.textContent !== GLYPH[glyph]) i.textContent = GLYPH[glyph];
  if (span.textContent !== text) span.textContent = text;
}

const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));

/* ------------------------------------------------------------ tasks */

const localDay = (date) => `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
const parseDay = (s) => { const [y, m, d] = String(s).split("-").map(Number); return new Date(y, m - 1, d, 12); };
const taskDots = (t) => {
  const normal = Math.min(t.actual, t.estimate), left = Math.max(0, t.estimate - t.actual), extra = Math.max(0, t.actual - t.estimate);
  return `<span>${"●".repeat(normal)}${"○".repeat(left)}</span>${extra ? `<span class="over">${"●".repeat(extra)}</span>` : ""}`;
};

function renderTasks() {
  const data = S.task_data;
  if (!data) return;
  const editing = document.activeElement && document.activeElement.matches && document.activeElement.matches("[data-task-title]");
  const key = JSON.stringify([data.tasks, data.ideas, data.active, data.history, data.today, data.streak]);
  if (editing || renderTasks.key === key) return;
  renderTasks.key = key;
  const open = data.tasks.filter((t) => t.done_at === null);
  $("#task-list").innerHTML = open.length ? open.map((t, i) => `
    <div class="task-item ${t.id === data.active ? "active" : ""}" data-task="${t.id}">
      <button class="task-check" data-task-action="complete" title="Mark done" aria-label="Mark ${esc(t.title)} done">✓</button>
      <div class="task-copy"><input class="task-name" data-task-title value="${esc(t.title)}" maxlength="160" aria-label="Task title"><div class="task-meta">${taskDots(t)}<span>${t.actual}/${t.estimate} pomodoros</span><span>${t.distractions} distractions</span></div></div>
      <div class="task-actions">
        <button class="make-active ${t.id === data.active ? "on" : ""}" data-task-action="active" title="Make active" aria-label="Make active">${t.id === data.active ? "●" : "○"}</button>
        <button data-task-action="up" title="Move up" aria-label="Move up" ${i === 0 ? "disabled" : ""}>↑</button><button data-task-action="down" title="Move down" aria-label="Move down" ${i === open.length - 1 ? "disabled" : ""}>↓</button>
        <span class="estimate-step"><button data-task-action="estimate-down" aria-label="Decrease estimate">−</button><b>${t.estimate}</b><button data-task-action="estimate-up" aria-label="Increase estimate">+</button></span>
        <button data-task-action="delete" title="Delete" aria-label="Delete task">×</button>
      </div>
    </div>`).join("") : `<p class="empty-note">Add the first thing you want to finish.</p>`;

  const done = data.tasks.filter((t) => t.done_at !== null && localDay(new Date(t.done_at * 1000)) === data.today);
  $("#done-count").textContent = done.length;
  $("#done-list").innerHTML = done.length ? done.map((t) => `<div class="done-row" data-task="${t.id}"><span>${esc(t.title)}</span><span>${t.actual}/${t.estimate} pomodoros <button data-done-delete aria-label="Delete task">×</button></span></div>`).join("") : `<p class="empty-note">Nothing checked off yet.</p>`;
  $("#idea-list").innerHTML = data.ideas.length ? data.ideas.map((idea) => `
    <div class="idea-item ${idea.done ? "done" : ""}" data-idea="${idea.id}"><button class="idea-check" data-idea-action="toggle" aria-label="${idea.done ? "Reopen" : "Check off"}">✓</button><span class="idea-title">${esc(idea.title)}</span><button class="idea-delete" data-idea-action="delete" aria-label="Delete idea">×</button></div>`).join("") : `<p class="empty-note">Ideas can wait here without interrupting the block.</p>`;
  renderHeatmap(data);
}

function renderHeatmap(data) {
  const byDay = new Map(data.history.map((d) => [d.date, d]));
  const today = parseDay(data.today), monday = new Date(today);
  monday.setDate(monday.getDate() - ((monday.getDay() + 6) % 7) - 52 * 7);
  const cells = [], months = [];
  let lastMonth = -1, total = 0;
  for (let i = 0; i < 53 * 7; i++) {
    const date = new Date(monday); date.setDate(monday.getDate() + i);
    const key = localDay(date), day = byDay.get(key) || { completed_focus: 0, earned_secs: 0 };
    const n = day.completed_focus, level = n === 0 ? 0 : n <= 2 ? 1 : n <= 4 ? 2 : n <= 7 ? 3 : 4;
    if (date <= today) total += n;
    const col = Math.floor(i / 7);
    if (date.getMonth() !== lastMonth && date.getDate() <= 7) {
      months.push(`<span style="left:calc(${col} * (var(--cell, 11px) + 3px))">${date.toLocaleString(undefined, { month: "short" })}</span>`);
      lastMonth = date.getMonth();
    }
    const future = date > today;
    const tip = `${date.toLocaleDateString(undefined, { weekday: "short", year: "numeric", month: "short", day: "numeric" })} · ${n} pomodoro${n === 1 ? "" : "s"} · ${mins(day.earned_secs)} focus`;
    cells.push(`<i class="heat-cell" data-level="${future ? 0 : level}" data-tip="${esc(tip)}" style="${future ? "opacity:.25" : ""}"></i>`);
  }
  $("#heat-months").innerHTML = months.join("");
  $("#heatmap").innerHTML = cells.join("");
  $("#history-total").textContent = `${total} pomodoro${total === 1 ? "" : "s"} in the last year`;
  $("#history-streak").textContent = data.streak;
  $(".history-head p").lastChild.textContent = ` day${data.streak === 1 ? "" : "s"} current streak`;
}

$("#task-add").addEventListener("submit", (e) => {
  e.preventDefault();
  const title = $("#task-title").value.trim();
  if (!title) return;
  send("task-add", { title, estimate: Math.min(99, Math.max(1, Number($("#task-estimate").value) || 1)) });
  $("#task-title").value = "";
});
$("#idea-add").addEventListener("submit", (e) => {
  e.preventDefault();
  const title = $("#idea-title").value.trim();
  if (!title) return;
  send("idea-add", { title });
  $("#idea-title").value = "";
});
$("#task-list").addEventListener("click", (e) => {
  const button = e.target.closest("[data-task-action]"); if (!button) return;
  const id = Number(button.closest("[data-task]").dataset.task), task = S.task_data.tasks.find((t) => t.id === id), action = button.dataset.taskAction;
  if (action === "complete") send("task-complete", { id });
  else if (action === "active") send("task-active", { id });
  else if (action === "up" || action === "down") send("task-move", { id, delta: action === "up" ? -1 : 1 });
  else if (action === "estimate-up" || action === "estimate-down") send("task-estimate", { id, estimate: task.estimate + (action === "estimate-up" ? 1 : -1) });
  else if (action === "delete") send("task-delete", { id });
});
$("#task-list").addEventListener("change", (e) => {
  if (!e.target.matches("[data-task-title]")) return;
  const id = Number(e.target.closest("[data-task]").dataset.task), title = e.target.value.trim();
  if (!title) { e.target.value = S.task_data.tasks.find((t) => t.id === id).title; return; }
  send("task-rename", { id, title });
});
$("#task-list").addEventListener("keydown", (e) => { if (e.key === "Enter" && e.target.matches("[data-task-title]")) e.target.blur(); });
$("#done-list").addEventListener("click", (e) => { if (e.target.closest("[data-done-delete]")) send("task-delete", { id: Number(e.target.closest("[data-task]").dataset.task) }); });
$("#idea-list").addEventListener("click", (e) => {
  const button = e.target.closest("[data-idea-action]"); if (!button) return;
  const id = Number(button.closest("[data-idea]").dataset.idea);
  send(button.dataset.ideaAction === "toggle" ? "idea-toggle" : "idea-delete", { id });
});
const heatTip = $("#heat-tip");
$("#heatmap").addEventListener("pointermove", (e) => {
  const cell = e.target.closest("[data-tip]"); if (!cell) return;
  heatTip.textContent = cell.dataset.tip; heatTip.hidden = false;
  heatTip.style.left = `${Math.min(innerWidth - heatTip.offsetWidth - 8, e.clientX + 12)}px`; heatTip.style.top = `${e.clientY + 14}px`;
});
$("#heatmap").addEventListener("pointerleave", () => { heatTip.hidden = true; });

/* ------------------------------------------------------------ camera stage */

const view = $("#view");
const ctx = view.getContext("2d");
let frame = null, frameSeq = -1, target = null, shown = null, lastPreview = 0;

const LABELS = {
  screen: ["Looking at your screen", "gaze"],
  book: ["Looking at your book", "focus"],
  away: ["Looking away", "caution"],
  absent: ["No one at the desk", "alert"],
};

async function previewLoop() {
  if (!document.hidden) {
    try {
      const p = await (await fetch("preview", { cache: "no-store" })).json();
      if (p.error) {
        showEmpty(p.error);
      } else if (p.seq !== null && p.seq !== frameSeq) {
        const blob = await (await fetch(`frame?seq=${p.seq}`, { cache: "no-store" })).blob();
        // Decode first, then swap: closing the old bitmap before the new one exists let draw()
        // hit a closed image, throw, and stop the loop for good (frozen preview and label).
        const next = await createImageBitmap(blob);
        const old = frame;
        frame = next;
        if (old) old.close();
        frameSeq = p.seq;
        if (Math.abs(next.width / next.height - camAspect) > 0.01) {
          camAspect = next.width / next.height;
          layout();
        }
        target = p.overlay;
        lastPreview = performance.now();
        showEmpty(null);
        wake();
      } else if (p.seq === null) {
        if (frame) { frame.close(); frame = null; frameSeq = -1; target = null; wake(); }
        const off = S && !S.settings.camera_enabled && !(S.cal && S.cal.target);
        showEmpty(off ? "The camera is off" : S && S.cam.error ? S.cam.error : "Starting your camera…");
      }
    } catch (_) { /* ignore */ }
  }
  setTimeout(previewLoop, 30);
}

function showEmpty(text) {
  $("#stage-empty").hidden = !text;
  if (text) $("#stage-empty-text").textContent = text;
}

// The overlay was measured on the very frame shown with it, so the mesh, eyes and face box snap to
// it; easing them made the drawing trail the picture. Only the gaze arrow glides.
function lerpOverlay(a, b, k) {
  if (!a || !b || !a.gaze || !b.gaze) return b;
  return { ...b, gaze: [a.gaze[0] + (b.gaze[0] - a.gaze[0]) * k, a.gaze[1] + (b.gaze[1] - a.gaze[1]) * k] };
}

// Redraw only after a new frame and while the overlay glides (~0.5 s), at most 30 fps.
let settleUntil = 0, drawing = false, lastDraw = 0, paused = false;
function wake() {
  settleUntil = performance.now() + 500;
  if (!drawing && !paused) {
    drawing = true;
    requestAnimationFrame(draw);
  }
}
addEventListener("resize", wake);

// Camera column: the preview keeps the camera's own aspect ratio (uncropped) and is as large as
// the window allows with the history card under it, leaving the side column at least SIDE_MIN.
// The heatmap's cells grow with the column so the year spans the card. Cell size and column width
// depend on each other through the card's height, so a few passes settle them.
const SIDE_MIN = 460;
const narrow = matchMedia("(max-width: 860px)");
let camAspect = 16 / 9;
function layout() {
  const main = $("main");
  const cs = getComputedStyle(main);
  const w = main.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
  const h = main.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
  const history = $(".history-panel");
  const gap = parseFloat(getComputedStyle($(".col-main")).rowGap) || 0;
  const setCell = (colW) => {
    const hs = getComputedStyle(history);
    const chrome = parseFloat(hs.paddingLeft) + parseFloat(hs.paddingRight) + parseFloat(hs.borderLeftWidth) + parseFloat(hs.borderRightWidth);
    const grid = colW - chrome - $(".heat-labels").offsetWidth - 8;
    const cell = Math.max(5, Math.min(20, Math.floor(((grid + 3) / 53 - 3) * 4) / 4));
    main.style.setProperty("--cell", `${cell}px`);
  };
  main.style.setProperty("--cam-ar", String(camAspect));
  if (narrow.matches) {
    setCell(w);
  } else {
    let width = w * 0.6;
    for (let pass = 0; pass < 3; pass++) {
      setCell(width);
      const stageH = Math.max(120, h - history.offsetHeight - gap);
      width = Math.max(240, Math.min(stageH * camAspect, w - parseFloat(cs.columnGap) - SIDE_MIN));
    }
    width = Math.floor(width);
    setCell(width);
    // Cells now fixed: if the card grew, narrow the camera so the column still fits.
    width = Math.floor(Math.min(width, Math.max(120, h - history.offsetHeight - gap) * camAspect));
    main.style.setProperty("--main-w", `${width}px`);
  }
  // The newest weeks sit at the right end of the heatmap.
  const heat = $(".heat-wrap");
  heat.scrollLeft = heat.scrollWidth;
}
new ResizeObserver(layout).observe($("main"));

function draw(now) {
  if (now - lastDraw < 32) {
    requestAnimationFrame(draw);
    return;
  }
  lastDraw = now;
  try {
    paint();
  } catch (e) {
    // Never let one bad frame stop the loop.
    console.error(e);
  }
  if (!paused && performance.now() < settleUntil) requestAnimationFrame(draw);
  else drawing = false;
}

function paint() {
  const dpr = devicePixelRatio || 1;
  const w = view.clientWidth, h = view.clientHeight;
  if (view.width !== Math.round(w * dpr) || view.height !== Math.round(h * dpr)) {
    view.width = Math.round(w * dpr);
    view.height = Math.round(h * dpr);
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  if (frame) {
    // Cover-fit, mirrored like a mirror so moving left moves left.
    const s = Math.max(w / frame.width, h / frame.height);
    const dw = frame.width * s, dh = frame.height * s;
    const ox = (w - dw) / 2, oy = (h - dh) / 2;
    if (!(S && S.settings.hide_camera_image)) {
      ctx.save();
      ctx.translate(w, 0);
      ctx.scale(-1, 1);
      ctx.globalAlpha = 0.92;
      ctx.drawImage(frame, w - ox - dw, oy, dw, dh);
      ctx.restore();
    }
    // Soft vignette keeps the overlay legible.
    const g = ctx.createLinearGradient(0, h * 0.55, 0, h);
    g.addColorStop(0, "rgba(20,23,29,0)");
    g.addColorStop(1, "rgba(20,23,29,.55)");
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, w, h);
    shown = lerpOverlay(shown, target, reduced ? 1 : 0.5);
    if (shown) drawOverlay(shown, (x, y) => [ox + (1 - x) * dw, oy + y * dh], dw);
  }
  updateChip();
}

function drawOverlay(o, P, dw) {
  const css = getComputedStyle(document.documentElement);
  const gaze = css.getPropertyValue("--gaze").trim();
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  if (o.landmarks && o.landmarks.length) {
    ctx.fillStyle = "rgba(155,176,240,.28)";
    for (const [x, y] of o.landmarks) {
      const [px, py] = P(x, y);
      ctx.beginPath();
      ctx.arc(px, py, 1.2, 0, Math.PI * 2);
      ctx.fill();
    }
  }
  const irises = [];
  for (const e of o.eyes || []) {
    if (e.contour && e.contour.length > 2) {
      ctx.beginPath();
      e.contour.forEach(([x, y], i) => {
        const [px, py] = P(x, y);
        i ? ctx.lineTo(px, py) : ctx.moveTo(px, py);
      });
      ctx.closePath();
      ctx.strokeStyle = "rgba(155,176,240,.75)";
      ctx.lineWidth = 1.4;
      ctx.stroke();
    }
    const [ix, iy] = P(e.iris[0], e.iris[1]);
    const r = Math.max(2.5, e.iris_r * dw);
    const glow = ctx.createRadialGradient(ix, iy, 0, ix, iy, r * 2.4);
    glow.addColorStop(0, "rgba(155,176,240,.55)");
    glow.addColorStop(1, "rgba(155,176,240,0)");
    ctx.fillStyle = glow;
    ctx.beginPath();
    ctx.arc(ix, iy, r * 2.4, 0, Math.PI * 2);
    ctx.fill();
    ctx.beginPath();
    ctx.arc(ix, iy, r, 0, Math.PI * 2);
    ctx.strokeStyle = gaze;
    ctx.lineWidth = 1.6;
    ctx.stroke();
    ctx.beginPath();
    ctx.arc(ix, iy, Math.max(1.5, r * 0.35), 0, Math.PI * 2);
    ctx.fillStyle = "#e4e7ec";
    ctx.fill();
    irises.push([ix, iy]);
  }
  if (o.gaze && irises.length) {
    const cx = irises.reduce((a, p) => a + p[0], 0) / irises.length;
    const cy = irises.reduce((a, p) => a + p[1], 0) / irises.length;
    const len = (o.face ? o.face[2] * dw : 120) * 0.9;
    // Mirror x to match the mirrored picture.
    const tx = cx - o.gaze[0] * len, ty = cy + o.gaze[1] * len;
    const g = ctx.createLinearGradient(cx, cy, tx, ty);
    g.addColorStop(0, "rgba(155,176,240,.9)");
    g.addColorStop(1, "rgba(155,176,240,0)");
    ctx.strokeStyle = g;
    ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.moveTo(cx, cy);
    ctx.lineTo(tx, ty);
    ctx.stroke();
  }
}

function updateChip() {
  const chip = $("#chip");
  const o = target;
  const stale = performance.now() - lastPreview > 2500;
  let text = "Waiting for the camera", tone = "idle", conf = "";
  if (S && S.cam.error) {
    text = S.cam.error;
    tone = "alert";
  } else if (o && !stale) {
    const [t, tn] = LABELS[o.label] || ["Reading your position", "idle"];
    text = t;
    tone = tn;
    if (o.label && o.label !== "absent" && o.confidence > 0) conf = `${Math.round(o.confidence * 100)}%`;
    const head = o.head ? `${Math.round(o.head[0])}°  ${Math.round(o.head[1])}°` : "–";
    $("#r-head").textContent = head;
    $("#r-gaze").textContent = o.gaze_deg ? `${Math.round(o.gaze_deg[0])}°  ${Math.round(o.gaze_deg[1])}°` : "–";
    $("#r-eyes").textContent = o.eyes && o.eyes.length ? (o.eyes.length === 2 ? "both found" : "one found") : "not found";
    $("#r-ms").textContent = `${Math.round(o.l1_ms)} ms`;
  }
  if (chip.dataset.tone !== tone) chip.dataset.tone = tone;
  if ($("#chip-text").textContent !== text) $("#chip-text").textContent = text;
  $("#chip-conf").textContent = conf;
}

/* ------------------------------------------------------------ session buttons */

$("#start").onclick = () => send("toggle");
$("#brb").onclick = () => send("brb");
$("#skip").onclick = () => send("skip");
$("#camera").onchange = (e) => send("settings", { patch: { camera_index: Number(e.target.value) } });

/* ------------------------------------------------------------ settings sheet */

const ICON_X = `<i class="ph" aria-hidden="true">${GLYPH.x}</i>`;

const sheet = {
  open: false,
  sec: "session",
  show(sec) {
    if (sec) this.sec = sec;
    this.open = true;
    $("#scrim").hidden = false;
    requestAnimationFrame(() => {
      $("#scrim").classList.add("show");
      $("#sheet").classList.add("open");
    });
    $("#sheet").setAttribute("aria-hidden", "false");
    this.build();
  },
  hide() {
    recorder.end();
    this.open = false;
    $("#scrim").classList.remove("show");
    $("#sheet").classList.remove("open");
    $("#sheet").setAttribute("aria-hidden", "true");
    setTimeout(() => { if (!this.open) $("#scrim").hidden = true; }, 300);
  },
  build() {
    recorder.end();
    document.querySelectorAll("#sheet-nav button").forEach((b) => b.classList.toggle("on", b.dataset.sec === this.sec));
    $("#sheet-body").innerHTML = SECTIONS[this.sec]();
    $("#sheet-body").scrollTop = 0;
    this.refresh(true);
  },
  // Push live values into controls without clobbering what the user is editing.
  refresh(force) {
    if (!S) return;
    const set = S.settings;
    document.querySelectorAll("#sheet-body [data-k]").forEach((el) => {
      if (!force && el === document.activeElement) return;
      const v = set[el.dataset.k];
      if (el.type === "checkbox") el.checked = !!v;
      else if (el.dataset.kind === "sound") el.value = soundKey(v);
      else if (el.dataset.kind === "meme") el.value = memeKey(v);
      else if (v !== undefined && el.value !== String(v)) el.value = v;
    });
    document.querySelectorAll("#sheet-body [data-list]").forEach((el) => {
      const items = set[el.dataset.list] || [];
      const key = items.join("|");
      if (el.dataset.key === key) return;
      el.dataset.key = key;
      el.innerHTML = items.length
        ? items.map((d) => `<span class="tag">${esc(d)}<button data-rm="${esc(d)}" aria-label="Remove ${esc(d)}">${ICON_X}</button></span>`).join("")
        : `<span class="help" style="color:var(--muted)">Nothing here yet.</span>`;
    });
    document.querySelectorAll("#sheet-body [data-bind]").forEach((el) => {
      if (el === recorder.btn || (el === recorder.held && performance.now() < recorder.heldUntil)) return;
      const h = S.hotkeys.find((x) => x.id === el.dataset.bind);
      const text = (h && h.keys) || "Not set";
      if (el.textContent !== text) el.textContent = text;
    });
    const cs = $("#cal-summary");
    if (cs) {
      const c = S.cam.calibrated;
      cs.innerHTML = ["screen", "book", "away"].map((t) => `<span class="${c[t] ? "ok" : ""}">${t[0].toUpperCase() + t.slice(1)}: ${c[t] ? `${c[t]} samples` : "not yet"}</span>`).join("");
    }
  },
};

const soundKey = (v) => (v && v.Builtin !== undefined ? `b:${v.Builtin}` : v && v.File !== undefined ? `f:${v.File}` : "");
const memeKey = (v) => (typeof v === "string" ? v : v && v.Builtin !== undefined ? `b:${v.Builtin}` : "");

const num = (k, label, min, max, unit, help = "") => `
  <div class="row"><label for="f-${k}">${label}${help ? `<small>${help}</small>` : ""}</label>
  <span class="num"><input id="f-${k}" data-k="${k}" type="number" min="${min}" max="${max}" step="1"><span>${unit}</span></span></div>`;
const sw = (k, label, help = "") => `
  <div class="row"><label for="f-${k}">${label}${help ? `<small>${help}</small>` : ""}</label>
  <span class="switch"><input id="f-${k}" data-k="${k}" type="checkbox"><i></i></span></div>`;
const list = (k, placeholder) => `
  <div class="tags" data-list="${k}"></div>
  <div class="add"><input class="field" data-add="${k}" placeholder="${placeholder}"><button class="secondary" data-addbtn="${k}">Add</button></div>`;

const SECTIONS = {
  session: () => `
    <div class="group"><h4>Focus blocks</h4><p class="help">New lengths apply from the next block.</p>
      ${num("focus_min", "Focus", 1, 240, "min")}
      ${num("short_break_min", "Short break", 1, 60, "min")}
      ${num("long_break_min", "Long break", 1, 120, "min")}
      ${num("long_break_every", "Long break after", 1, 12, "blocks")}
      ${sw("auto_start_breaks", "Start breaks on their own")}
      ${sw("auto_start_focus", "Start the next focus block on its own")}
      ${num("daily_goal_min", "Daily goal", 10, 900, "min")}
    </div>
    <div class="group"><h4>Stepping away</h4><p class="help">${keyHint("brb", "", " or ")}“Step away” pauses everything without alarms. Come back in time or the alarm rings.</p>
      ${num("brb_secs", "Time allowed", 15, 1800, "sec")}
      ${sw("brb_auto_resume", "Resume when the camera sees me working again")}
    </div>
    <div class="group"><div class="btns"><button class="secondary" data-act="reset">Reset timer</button></div></div>`,
  camera: () => `
    <div class="group"><h4>Calibration</h4><p class="help">Teach LockIn what your screen, your book and looking away look like from your camera. About 15 seconds. Redo it if you move the laptop.</p>
      <div class="cal-summary" id="cal-summary"></div>
      <div class="btns"><button class="primary" data-act="calibrate">Calibrate</button><button class="secondary" data-act="cal-reset">Forget calibration</button></div>
    </div>
    <div class="group"><h4>When to check</h4>
      ${sw("camera_enabled", "Use the camera during focus blocks")}
      <div class="row"><label for="f-mode">How often</label>
        <select class="field" id="f-mode" data-k="camera_mode"><option value="Interval">Every few seconds (saves power)</option><option value="Always">Continuously, 2 per second</option></select></div>
      ${num("check_interval_secs", "Check every", 1, 30, "sec")}
      ${sw("hide_camera_image", "Hide the camera picture", "Only the tracking lines show here, handy when sharing your screen")}
      <div class="row"><label for="f-gaze_mode">Decide by<small>For testing: “Gaze only” ignores head pose and distance and uses just where your eyes point</small></label>
        <select class="field" id="f-gaze_mode" data-k="gaze_mode"><option value="Combined">Gaze, head and position</option><option value="GazeOnly">Gaze only</option></select></div>
      <p class="help">Frames stay in memory and are never saved. The camera turns off during breaks.</p>
    </div>
    <div class="group"><h4>When to alert</h4>
      ${num("absent_secs", "Away from the desk for", 5, 600, "sec")}
      ${num("away_secs", "Looking away for", 5, 600, "sec")}
      ${num("screen_secs", "On the screen for", 10, 3600, "sec", "Then the open site is checked against your lists")}
      ${sw("phone_detection", "Look for a phone while my head is down")}
      ${num("phone_check_secs", "Phone check every", 10, 600, "sec")}
      ${sw("posture_hints", "Nudge me when I lean into the screen")}
    </div>`,
  sites: () => `
    <div class="group"><h4>Blocked</h4><p class="help">First visit shows a warning. The next one, or staying too long, rings the alarm. App names like discord.exe work too.</p>
      ${list("blocked_sites", "x.com or discord.exe")}
      ${num("strikes_before_alarm", "Alarm on visit", 1, 10, "")}
      ${num("blocked_dwell_secs", "Alarm anyway after staying", 5, 600, "sec")}
    </div>
    <div class="group"><h4>Ask me first</h4><p class="help">Sites that can be study or distraction. LockIn asks once per session.</p>${list("ask_sites", "youtube.com")}</div>
    <div class="group"><h4>Always fine</h4>${list("allowed_sites", "notion.so")}</div>
    <div class="group">${sw("screen_enabled", "Watch the active app and tab during focus blocks", "Only the app name, window title and address bar are read")}</div>`,
  penalties: () => `
    <div class="group"><h4>Extra time</h4>
      ${sw("penalty_add_time", "Add time to the focus block")}
      ${num("penalty_add_secs", "Per distraction", 10, 900, "sec")}
      ${sw("penalty_escalate", "Double it each time in the same block")}
      ${num("penalty_max_add_secs", "At most per block", 10, 3600, "sec")}
    </div>
    <div class="group"><h4>Other penalties</h4>
      ${sw("penalty_pause_timer", "Pause the timer until I'm back")}
      ${sw("penalty_score", "Take points off today's score")}
      ${num("penalty_score_points", "Points", 1, 50, "")}
      ${sw("penalty_reset_streak", "Reset the clean-block streak")}
      ${sw("penalty_brb_overdue", "Count a late return from a pause")}
    </div>`,
  alerts: () => `
    <div class="group"><h4>Sound</h4>
      <div class="row"><label for="f-sound">Alarm</label><select class="field" id="f-sound" data-k="sound" data-kind="sound">${S.sounds.map(([k, n]) => `<option value="${esc(k)}">${esc(n)}</option>`).join("")}</select></div>
      <div class="row"><label for="f-volume">Volume</label><input id="f-volume" data-k="volume" type="range" min="0" max="1" step="0.05"></div>
      ${sw("alarm_loop", "Keep ringing until I'm back")}
      ${sw("warn_sound", "Soft chime for warnings and questions")}
      <div class="btns"><button class="secondary" data-act="test-sound">Play</button><button class="secondary" data-act="stop-sound">Stop</button></div>
    </div>
    <div class="group"><h4>Picture</h4>
      <div class="row"><label for="f-meme">On alerts</label><select class="field" id="f-meme" data-k="meme" data-kind="meme">${S.memes.map(([k, n]) => `<option value="${esc(k)}">${esc(n)}</option>`).join("")}</select></div>
      <div class="btns"><button class="secondary" data-act="preview-alert">Show an example</button><button class="secondary" data-act="open-memes">Open my memes folder</button></div>
      <p class="help" style="margin-top:10px">Drop GIF, PNG or JPG pictures and MP3 or WAV sounds into that folder.</p>
    </div>
    <div class="group">${sw("brainrot", "Brainrot wording", "Alerts get a lot less polite")}</div>`,
  keys: () => `
    <div class="group"><h4>Shortcuts</h4><p class="help">They work anywhere in Windows. Click one, then press the keys, scroll the wheel, or press the middle or a side button (Mouse 4 / 5). Esc cancels, Backspace removes it.</p>
      ${S.hotkeys.map((h) => `<div class="row"><label>${esc(h.label)}${h.taken ? `<small class="taken">Another app already uses this shortcut</small>` : ""}</label><button class="secondary bind" data-bind="${h.id}">${esc(h.keys || "Not set")}</button></div>`).join("")}
      <p class="help" style="margin-top:10px">A bare wheel or side button stops doing its usual job everywhere while bound; hold Ctrl or Alt with it to keep both.</p>
      <div class="btns"><button class="secondary" data-act="keys-default">Restore the defaults</button></div>
    </div>
    <div class="group"><h4>Windows</h4>
      ${sw("launch_at_startup", "Start LockIn when I sign in", "Starts the copy of LockIn you opened last, so a newer build takes over by itself")}
      ${S.startup_error ? `<p class="help taken">${esc(S.startup_error)}</p>` : ""}
      <p class="help">LockIn stays in the notification area, not on the taskbar. Minimising this window tucks it away there.</p>
    </div>
    <div class="group"><div class="btns"><button class="secondary" data-act="quit">Quit LockIn</button></div></div>`,
  pill: () => `
    <div class="group"><h4>Look</h4>
      <div class="row"><label for="f-pill_theme">Theme</label>
        <select class="field" id="f-pill_theme" data-k="pill_theme"><option value="Slate">Slate</option><option value="Midnight">Midnight</option><option value="Glass">Glass</option></select></div>
      <div class="row"><label for="f-pill_opacity">Opacity<small>It turns fully visible while your mouse is over it</small></label><input id="f-pill_opacity" data-k="pill_opacity" type="range" min="0.3" max="1" step="0.02"></div>
    </div>
    <div class="group"><h4>Ball</h4><p class="help">When idle the bar folds into a ball. Blue normally, amber while you look away, red on a blocked site or alarm.</p>
      ${sw("pill_fold", "Fold into a ball when idle", "It opens again under the mouse or for an alert")}
      ${sw("pill_clock", "Clock around the ball", "A ring that fills as the block runs, with a hand at its end")}
      ${num("pill_fold_secs", "Fold after", 2, 120, "sec")}
    </div>
    <div class="group"><h4>Behaviour</h4>
      ${sw("overlay_click_through", "Clicks pass through the timer bar", `${keyHint("click_through", "Turn off with ", ". ")}Always off again when LockIn starts`)}
      <div class="btns"><button class="secondary" data-act="pill-home">Move the timer bar back to the top</button><button class="secondary" data-act="calc">Open the calculator</button></div>
    </div>`,
};

/** The action's shortcut wrapped in `before`/`after` for help text; empty when it has none. */
function keyHint(id, before = "", after = "") {
  const h = S && S.hotkeys.find((x) => x.id === id);
  return h && h.keys ? `${before}${esc(h.keys)}${after}` : "";
}

function parseControl(el) {
  if (el.type === "checkbox") return el.checked;
  if (el.type === "number" || el.type === "range") return Number(el.value);
  if (el.dataset.kind === "sound") return el.value.startsWith("b:") ? { Builtin: el.value.slice(2) } : { File: el.value.slice(2) };
  if (el.dataset.kind === "meme") return el.value.startsWith("b:") ? { Builtin: el.value.slice(2) } : el.value;
  return el.value;
}

$("#sheet-body").addEventListener("change", (e) => {
  const el = e.target;
  if (!el.dataset.k) return;
  let v = parseControl(el);
  if (el.type === "number") {
    v = Math.min(Number(el.max), Math.max(Number(el.min), Math.round(v || 0)));
    el.value = v;
  }
  S.settings[el.dataset.k] = v;
  send("settings", { patch: { [el.dataset.k]: v } });
});
$("#sheet-body").addEventListener("input", (e) => {
  if (e.target.type === "range") send("settings", { patch: { [e.target.dataset.k]: Number(e.target.value) } });
});
$("#sheet-body").addEventListener("click", (e) => {
  const rm = e.target.closest("[data-rm]");
  if (rm) {
    const listEl = rm.closest("[data-list]");
    const k = listEl.dataset.list;
    const items = S.settings[k].filter((x) => x !== rm.dataset.rm);
    S.settings[k] = items;
    send("settings", { patch: { [k]: items } });
    listEl.dataset.key = "";
    sheet.refresh();
    return;
  }
  const bind = e.target.closest("[data-bind]");
  if (bind) return recorder.start(bind);
  const add = e.target.closest("[data-addbtn]");
  if (add) return addItem(add.dataset.addbtn);
  const act = e.target.closest("[data-act]");
  if (!act) return;
  const a = act.dataset.act;
  if (a === "calibrate") { sheet.hide(); cal.open(); return; }
  if (a === "cal-reset") { send("cal-reset"); toast("Calibration cleared"); return; }
  if (a === "preview-alert") toast("Example alert shown");
  send(a);
});
$("#sheet-body").addEventListener("keydown", (e) => {
  if (e.key === "Enter" && e.target.dataset.add) addItem(e.target.dataset.add);
});

function addItem(k) {
  const input = document.querySelector(`[data-add="${k}"]`);
  let v = input.value.trim().toLowerCase();
  if (!v) return;
  v = v.replace(/^[a-z]+:\/\//, "").replace(/^www\./, "").split(/[/?#]/)[0];
  const items = S.settings[k];
  if (!items.includes(v)) items.push(v);
  input.value = "";
  send("settings", { patch: { [k]: items } });
  document.querySelector(`[data-list="${k}"]`).dataset.key = "";
  sheet.refresh();
}

/* ------------------------------------------------------------ shortcut recorder */

// Matches hotkeys.rs: wheel pseudo codes. Middle and side buttons are captured by LockIn's mouse
// hook (the page never saw Mouse 4) and arrive through the state as `bind_captured`.
const WHEEL_UP = 0x10000, WHEEL_DOWN = 0x10001;
const MODIFIER_CODES = new Set([16, 17, 18, 91, 92, 93]);
const mods = (e) => ({ ctrl: e.ctrlKey, alt: e.altKey, shift: e.shiftKey, win: e.metaKey });
const sameBind = (a, b) => a && b && a.key === b.key && !!a.ctrl === !!b.ctrl && !!a.alt === !!b.alt && !!a.shift === !!b.shift && !!a.win === !!b.win;

const recorder = {
  id: null,
  btn: null,
  // Just-saved button keeps its text until the app's new label arrives.
  held: null,
  heldUntil: 0,
  start(btn) {
    this.end();
    this.id = btn.dataset.bind;
    this.btn = btn;
    // Only buttons pressed from now on count.
    this.seq = S.bind_captured.seq;
    btn.classList.add("recording");
    btn.textContent = "Press a shortcut…";
    // LockIn lets go of every shortcut meanwhile, so the one being pressed reaches this page.
    send("bind-capture", { on: true });
  },
  // Called on every state poll.
  poll() {
    const c = S.bind_captured;
    if (this.id && c.seq !== this.seq && c.bind) this.commit(c.bind);
  },
  end() {
    if (!this.id) return;
    this.btn.classList.remove("recording");
    this.id = this.btn = null;
    send("bind-capture", { on: false });
    sheet.refresh();
  },
  commit(binding) {
    const binds = { ...S.settings.keybinds };
    // One shortcut, one action: take it away from whichever action had it.
    if (binding) for (const k of Object.keys(binds)) if (k !== this.id && sameBind(binds[k], binding)) binds[k] = null;
    binds[this.id] = binding;
    S.settings.keybinds = binds;
    send("settings", { patch: { keybinds: binds } });
    this.btn.textContent = binding ? "Saved" : "Not set";
    this.held = this.btn;
    this.heldUntil = performance.now() + 700;
    this.end();
  },
};

document.addEventListener("keydown", (e) => {
  if (!recorder.id) return;
  e.preventDefault();
  e.stopImmediatePropagation();
  const m = mods(e);
  const anyMod = m.ctrl || m.alt || m.shift || m.win;
  if (MODIFIER_CODES.has(e.keyCode)) {
    recorder.btn.textContent = `${[m.ctrl && "Ctrl", m.alt && "Alt", m.shift && "Shift", m.win && "Win"].filter(Boolean).join("+")}+…`;
    return;
  }
  if (e.key === "Escape" && !anyMod) return recorder.end();
  if ((e.key === "Backspace" || e.key === "Delete") && !anyMod) return recorder.commit(null);
  // A bare key would stop typing it anywhere; function keys are the exception.
  const fkey = e.keyCode >= 112 && e.keyCode <= 135;
  if (!fkey && !(m.ctrl || m.alt || m.win)) {
    recorder.btn.textContent = "Hold Ctrl, Alt or Win with it";
    return;
  }
  recorder.commit({ ...m, key: e.keyCode });
}, true);

document.addEventListener("wheel", (e) => {
  if (!recorder.id) return;
  e.preventDefault();
  recorder.commit({ ...mods(e), key: e.deltaY > 0 ? WHEEL_DOWN : WHEEL_UP });
}, { capture: true, passive: false });

document.addEventListener("mousedown", (e) => {
  if (recorder.id && e.button === 0 && e.target !== recorder.btn) recorder.end();
}, true);

// Side buttons would otherwise navigate the page back or forward.
for (const type of ["mouseup", "auxclick"]) {
  document.addEventListener(type, (e) => { if (e.button === 3 || e.button === 4) e.preventDefault(); }, true);
}

$("#open-settings").onclick = () => sheet.show();
$("#close-settings").onclick = () => sheet.hide();
$("#scrim").onclick = () => sheet.hide();
$("#sheet-nav").onclick = (e) => {
  const b = e.target.closest("button");
  if (b) { sheet.sec = b.dataset.sec; sheet.build(); }
};
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    if (tour.active) tour.end();
    else if (!$("#cal").hidden) cal.close();
    else if (!$("#feedback").hidden) feedback.close();
    else if (sheet.open) sheet.hide();
  }
});

/* ------------------------------------------------------------ calibration */

const CAL = {
  screen: ["Look at your screen", "This window steps aside and the timer bar turns into a ball. Follow it across the screen with your eyes. When it rests in the middle and breathes, tip your head down toward the keyboard and back up, eyes still on the ball."],
  book: ["Look at your book", "Read a few lines the way you normally study."],
  away: ["Look away", "Look around the room, at the wall, out of the window. Not at the desk."],
};
const ORDER = ["screen", "book", "away"];
// Ticks for the reading/away steps (the screen step's notes play natively with the ball).
const BEAT = 1000;

let audio = null;
function tone(freq, dur = 0.24, gain = 0.12) {
  const vol = S ? S.settings.volume : 0.8;
  if (!vol || reduced) return;
  audio = audio || new AudioContext();
  const t0 = audio.currentTime;
  const o = audio.createOscillator(), g = audio.createGain();
  o.type = "sine";
  o.frequency.value = freq;
  g.gain.setValueAtTime(0, t0);
  g.gain.linearRampToValueAtTime(gain * vol, t0 + 0.012);
  g.gain.exponentialRampToValueAtTime(0.0001, t0 + dur);
  o.connect(g).connect(audio.destination);
  o.start(t0);
  o.stop(t0 + dur + 0.03);
}
const chord = () => [523.25, 659.25, 783.99].forEach((f, i) => setTimeout(() => tone(f, 0.5, 0.09), i * 70));

// Restart a CSS animation class so it plays again on every beat.
function pulse(el, cls) {
  el.classList.remove(cls);
  void el.offsetWidth;
  el.classList.add(cls);
}

const cal = {
  step: 0,
  running: false,
  timers: [],
  open() {
    this.step = 0;
    this.running = false;
    $("#cal").hidden = false;
    paused = true; // nothing animates behind the modal
    this.paint();
  },
  close() {
    $("#cal").hidden = true;
    this.stopBeats();
    if (this.running) send("cal-cancel");
    this.running = false;
    paused = false;
    wake();
  },
  stopBeats() {
    this.timers.forEach(clearTimeout);
    this.timers = [];
  },
  paint() {
    const t = ORDER[this.step];
    document.querySelectorAll("#cal-steps li").forEach((li, i) => {
      li.classList.toggle("on", i === this.step);
      li.classList.toggle("done", i < this.step);
    });
    $("#cal-title").textContent = CAL[t][0];
    $("#cal-body").textContent = CAL[t][1];
    $("#cal-go").textContent = "Start";
    $("#cal-go").disabled = false;
    $("#cal-count").textContent = "5";
    $("#cal-prog").style.strokeDashoffset = 326.7;
  },
  go() {
    const t = ORDER[this.step];
    this.running = true;
    this.sawProgress = false;
    $("#cal-go").disabled = true;
    send("calibrate", { target: t });
    this.stopBeats();
    // Screen: the timer bar folds into the ball and tours the whole monitor (native side).
    if (t !== "screen") {
      // Keep eyes on the book or the room; the ring keeps time.
      for (let i = 0; i < 5; i++) {
        this.timers.push(setTimeout(() => {
          pulse($(".cal-ring"), "beat");
          tone(i === 4 ? 659.25 : 440, 0.12, 0.08);
        }, i * BEAT));
      }
    }
  },
  update() {
    if (!S || !this.running) return;
    const c = S.cal;
    const t = ORDER[this.step];
    if (c.target === t) {
      this.sawProgress = true;
      $("#cal-prog").style.strokeDashoffset = 326.7 * (1 - c.progress);
      $("#cal-count").textContent = Math.max(0, Math.ceil(5 * (1 - c.progress)));
      if (t !== "screen") $("#cal-body").textContent = c.samples ? `${c.samples} clear looks so far. Keep going.` : CAL[t][1];
    }
    if (this.sawProgress && c.target === null) {
      // This step finished.
      this.running = false;
      this.stopBeats();
      if (c.error) {
        $("#cal-body").textContent = c.error;
        $("#cal-go").textContent = "Try again";
        $("#cal-go").disabled = false;
        return;
      }
      if (this.step < ORDER.length - 1) {
        this.step++;
        this.paint();
      } else {
        chord();
        this.close();
        toast("Calibrated. Watch the label under your face to check it.");
      }
    }
  },
};
$("#calibrate").onclick = () => cal.open();
$("#cal-go").onclick = () => cal.go();
$("#cal-cancel").onclick = () => cal.close();

/* ------------------------------------------------------------ feedback */

const feedback = {
  // State `feedback.seq` at the moment Send was pressed; the next one carries our result.
  waitFrom: null,
  open() {
    $("#feedback-error").hidden = true;
    $("#feedback").hidden = false;
    $("#feedback-message").focus();
  },
  close() {
    $("#feedback").hidden = true;
    this.waitFrom = null;
  },
  // Called on every state poll.
  update() {
    const f = S.feedback;
    $("#open-feedback").hidden = !f.enabled;
    $("#feedback-message").maxLength = f.max;
    if (this.waitFrom !== null && f.seq !== this.waitFrom && !f.sending) {
      this.waitFrom = null;
      if (f.error) {
        $("#feedback-error").textContent = f.error;
        $("#feedback-error").hidden = false;
      } else {
        $("#feedback-message").value = "";
        this.close();
        toast("Thanks, your feedback was sent");
      }
    }
    const busy = this.waitFrom !== null;
    $("#feedback-send").disabled = busy;
    $("#feedback-send").textContent = busy ? "Sending…" : "Send";
  },
};
$("#open-feedback").onclick = () => feedback.open();
$("#feedback-cancel").onclick = () => feedback.close();
$("#feedback-form").addEventListener("submit", (e) => {
  e.preventDefault();
  const message = $("#feedback-message").value.trim();
  if (!message || feedback.waitFrom !== null) return;
  $("#feedback-error").hidden = true;
  feedback.waitFrom = S.feedback.seq;
  send("feedback", { message, contact: $("#feedback-contact").value.trim() });
  feedback.update();
});
$("#open-github").onclick = () => send("open-github");

/* ------------------------------------------------------------ tour */

const STEPS = [
  { el: "#stage", title: "This is what LockIn sees", text: "Your eyes and where they point are drawn live. The label shows what LockIn thinks you are doing.", keys: [["open", "Open this window"]] },
  { el: "#calibrate", title: "Teach it your desk", text: "Calibrate once: follow a ball across your screen, read your book, then look away. Twenty seconds, and it learns your setup." },
  { el: ".session", title: "Start a focus block", text: "Monitoring only runs while a focus block counts down. The timer bar at the top of your screen shows the same clock.", keys: [["start_pause", "Start or pause"]] },
  { el: "#brb", title: "Stepping away", text: "Getting water? Step away pauses everything without alarms. Come back in time and the timer carries on where it stopped.", keys: [["brb", "Step away, or I'm back"]] },
  { el: "#open-settings", title: "Shortcuts that work anywhere", text: "Blocked sites, penalties, sounds, the timer bar and your own shortcuts live in settings. Replay this tour from the question mark.", keys: [["calculator", "Calculator"], ["click_through", "Timer bar click-through"]] },
];

const tour = {
  active: false,
  i: 0,
  start() {
    if (sheet.open) sheet.hide();
    this.active = true;
    this.i = 0;
    $("#tour").hidden = false;
    this.show();
  },
  end() {
    this.active = false;
    $("#tour").hidden = true;
    send("tour-done");
  },
  show() {
    const s = STEPS[this.i];
    const r = $(s.el).getBoundingClientRect();
    const pad = 8;
    const spot = $("#tour-spot");
    Object.assign(spot.style, { left: `${r.left - pad}px`, top: `${r.top - pad}px`, width: `${r.width + pad * 2}px`, height: `${r.height + pad * 2}px` });
    $("#tour-step").textContent = `${this.i + 1} of ${STEPS.length}`;
    $("#tour-title").textContent = s.title;
    $("#tour-text").textContent = s.text;
    $("#tour-keys").innerHTML = (s.keys || [])
      .map(([id, d]) => [((S && S.hotkeys.find((h) => h.id === id)) || {}).keys, d])
      .filter(([k]) => k)
      .map(([k, d]) => `<span>${k.split("+").map((p) => `<kbd>${esc(p)}</kbd>`).join("")}${esc(d)}</span>`)
      .join("");
    $("#tour-back").hidden = this.i === 0;
    $("#tour-next").textContent = this.i === STEPS.length - 1 ? "Done" : "Next";
    const card = $("#tour-card");
    const cw = 320, ch = card.offsetHeight || 170;
    let top = r.bottom + 16;
    if (top + ch > innerHeight - 12) top = Math.max(12, r.top - ch - 16);
    if (r.height > innerHeight * 0.5) top = r.top + 24;
    let left = Math.min(innerWidth - cw - 16, Math.max(16, r.left + r.width / 2 - cw / 2));
    if (r.height > innerHeight * 0.5) left = r.left + 24;
    Object.assign(card.style, { left: `${left}px`, top: `${top}px` });
  },
};
$("#help").onclick = () => tour.start();
$("#tour-next").onclick = () => (tour.i < STEPS.length - 1 ? (tour.i++, tour.show()) : tour.end());
$("#tour-back").onclick = () => { if (tour.i > 0) { tour.i--; tour.show(); } };
$("#tour-skip").onclick = () => tour.end();
addEventListener("resize", () => tour.active && tour.show());

poll();
previewLoop();
wake();
