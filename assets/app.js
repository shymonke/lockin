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
  $("#start").textContent = t.running ? "Pause" : (t.started ? "Resume" : (focus ? "Start focus" : "Start break"));
  const brb = $("#brb");
  brb.textContent = t.brb !== null || S.alarm ? "I'm back" : "Step away";
  brb.disabled = !(t.brb !== null || S.alarm || (t.running && focus));
  brb.title = t.brb !== null || S.alarm ? "Ctrl+Alt+B" : `Pause for ${Math.round(S.settings.brb_secs / 60 * 10) / 10} min without alarms (Ctrl+Alt+B)`;

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

  cal.update();
  if (sheet.open) sheet.refresh();
  if (!drawing) updateChip();
}

const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));

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
        if (frame) frame.close();
        frame = await createImageBitmap(blob);
        frameSeq = p.seq;
        target = p.overlay;
        lastPreview = performance.now();
        showEmpty(null);
        wake();
      } else if (p.seq === null) {
        showEmpty(S && S.cam.error ? S.cam.error : "Starting your camera…");
      }
    } catch (_) { /* ignore */ }
  }
  setTimeout(previewLoop, 66);
}

function showEmpty(text) {
  $("#stage-empty").hidden = !text;
  if (text) $("#stage-empty-text").textContent = text;
}

// Overlay values glide toward the newest measurement for calm motion.
function lerpOverlay(a, b, k) {
  if (!a || !b) return b && JSON.parse(JSON.stringify(b));
  const mix = (x, y) => x + (y - x) * k;
  const pts = (p, q) => (p && q && p.length === q.length ? p.map((v, i) => [mix(v[0], q[i][0]), mix(v[1], q[i][1])]) : q);
  const out = JSON.parse(JSON.stringify(b));
  if (a.face && b.face) out.face = a.face.map((v, i) => mix(v, b.face[i]));
  if (a.gaze && b.gaze) out.gaze = [mix(a.gaze[0], b.gaze[0]), mix(a.gaze[1], b.gaze[1])];
  out.landmarks = pts(a.landmarks, b.landmarks);
  if (a.eyes && b.eyes && a.eyes.length === b.eyes.length) {
    out.eyes = b.eyes.map((e, i) => ({ ...e, contour: pts(a.eyes[i].contour, e.contour), iris: [mix(a.eyes[i].iris[0], e.iris[0]), mix(a.eyes[i].iris[1], e.iris[1])] }));
  }
  return out;
}

// Redraw only after a new frame and while the overlay glides (~0.5 s), at most 30 fps.
let settleUntil = 0, drawing = false, lastDraw = 0;
function wake() {
  settleUntil = performance.now() + 500;
  if (!drawing) {
    drawing = true;
    requestAnimationFrame(draw);
  }
}
addEventListener("resize", wake);

function draw(now) {
  if (now - lastDraw < 32) {
    requestAnimationFrame(draw);
    return;
  }
  lastDraw = now;
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
    shown = lerpOverlay(shown, target, reduced ? 1 : 0.3);
    if (shown) drawOverlay(shown, (x, y) => [ox + (1 - x) * dw, oy + y * dh], dw);
  }
  updateChip();
  if (performance.now() < settleUntil) requestAnimationFrame(draw);
  else drawing = false;
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

const ICON_X = `<svg viewBox="0 0 24 24"><path d="M6 6l12 12M18 6 6 18"/></svg>`;

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
    this.open = false;
    $("#scrim").classList.remove("show");
    $("#sheet").classList.remove("open");
    $("#sheet").setAttribute("aria-hidden", "true");
    setTimeout(() => { if (!this.open) $("#scrim").hidden = true; }, 300);
  },
  build() {
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
    <div class="group"><h4>Stepping away</h4><p class="help">Ctrl+Alt+B or “Step away” pauses everything without alarms. Come back in time or the alarm rings.</p>
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
    <div class="group"><h4>Shortcuts</h4><p class="help">Work anywhere in Windows.</p>
      <div class="keys">${S.hotkeys.map(([k, d]) => `<kbd>Ctrl + Alt + ${esc(k)}</kbd><span>${esc(d)}</span>`).join("")}</div>
    </div>
    <div class="group">${sw("overlay_click_through", "Clicks pass through the timer bar", "Turn off again with Ctrl+Alt+L")}
      <div class="btns"><button class="secondary" data-act="pill-home">Move the timer bar back to the top</button><button class="secondary" data-act="calc">Open the calculator</button></div>
    </div>
    <div class="group"><div class="btns"><button class="secondary" data-act="quit">Quit LockIn</button></div></div>`,
};

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
    else if (sheet.open) sheet.hide();
  }
});

/* ------------------------------------------------------------ calibration */

const CAL = {
  screen: ["Look at your screen", "Follow the dot with your eyes. Keep your head where it normally is."],
  book: ["Look at your book", "Read a few lines the way you normally study."],
  away: ["Look away", "Look around the room, at the wall, out of the window. Not at the desk."],
};
const ORDER = ["screen", "book", "away"];
const DOTS = [[.5, .45], [.15, .15], [.85, .15], [.85, .8], [.15, .8], [.5, .45]];

const cal = {
  step: 0,
  running: false,
  open() {
    this.step = 0;
    this.running = false;
    $("#cal").hidden = false;
    this.paint();
  },
  close() {
    $("#cal").hidden = true;
    $("#cal").classList.remove("dotting");
    $("#cal-dot").hidden = true;
    clearInterval(this.dotTimer);
    if (this.running) send("cal-cancel");
    this.running = false;
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
    if (t === "screen") {
      // Hide the card and walk a dot around the window.
      $("#cal").classList.add("dotting");
      const dot = $("#cal-dot");
      dot.hidden = false;
      let i = 0;
      const place = () => {
        const [x, y] = DOTS[Math.min(i, DOTS.length - 1)];
        dot.style.left = `${x * innerWidth}px`;
        dot.style.top = `${y * innerHeight}px`;
        i++;
      };
      place();
      this.dotTimer = setInterval(place, 1000);
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
      clearInterval(this.dotTimer);
      $("#cal").classList.remove("dotting");
      $("#cal-dot").hidden = true;
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
        this.close();
        toast("Calibrated. Watch the label under your face to check it.");
      }
    }
  },
};
$("#calibrate").onclick = () => cal.open();
$("#cal-go").onclick = () => cal.go();
$("#cal-cancel").onclick = () => cal.close();

/* ------------------------------------------------------------ tour */

const STEPS = [
  { el: "#stage", title: "This is what LockIn sees", text: "Your eyes and where they point are drawn live. The label shows what LockIn thinks you are doing." },
  { el: "#calibrate", title: "Teach it your desk", text: "Calibrate once: look at the screen, then your book, then away. Fifteen seconds, and it learns your setup." },
  { el: ".session", title: "Start a focus block", text: "Monitoring only runs while a focus block counts down. The small timer bar at the top of your screen shows the same clock." },
  { el: "#brb", title: "Stepping away", text: "Getting water? Step away pauses everything without alarms. Press I'm back in time and the timer carries on where it stopped." },
  { el: "#open-settings", title: "Everything else", text: "Blocked sites, penalties, sounds and shortcuts. Replay this tour from the question mark." },
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
