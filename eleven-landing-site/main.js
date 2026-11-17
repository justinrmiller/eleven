// eleven landing site: seven-segment digits, the 0–11 volume knob, a decorative
// spectrum analyzer, OS-aware download links and the latest-release lookup.
// Plain ES2020, no dependencies. Everything degrades to working links without JS.
"use strict";

const REPO = "justinrmiller/eleven";
const SVG_NS = "http://www.w3.org/2000/svg";
const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

/* ---------------------------------------------------------------- 7-segment */

const SEGMENTS = {
  0: "abcdef", 1: "bc", 2: "abdeg", 3: "abcdg", 4: "bcfg",
  5: "acdfg", 6: "acdefg", 7: "abc", 8: "abcdefg", 9: "abcdfg",
  " ": "", "-": "g",
};
const DIGIT_W = 12;
const DIGIT_H = 22;
const COLON_W = 5;

/** Points for a hexagonal segment running horizontally from x1 to x2 at y. */
function hSeg(x1, x2, y, t) {
  const h = t / 2;
  return `${x1},${y} ${x1 + h},${y - h} ${x2 - h},${y - h} ${x2},${y} ${x2 - h},${y + h} ${x1 + h},${y + h}`;
}

/** Points for a hexagonal segment running vertically from y1 to y2 at x. */
function vSeg(y1, y2, x, t) {
  const h = t / 2;
  return `${x},${y1} ${x + h},${y1 + h} ${x + h},${y2 - h} ${x},${y2} ${x - h},${y2 - h} ${x - h},${y1 + h}`;
}

function digitSegments(ox) {
  const t = 2.6, g = 0.6;
  const L = ox + 1.6, R = ox + 10.4, T = 1.6, M = 11, B = 20.4;
  return {
    a: hSeg(L + g, R - g, T, t),
    g: hSeg(L + g, R - g, M, t),
    d: hSeg(L + g, R - g, B, t),
    f: vSeg(T + g, M - g, L, t),
    b: vSeg(T + g, M - g, R, t),
    e: vSeg(M + g, B - g, L, t),
    c: vSeg(M + g, B - g, R, t),
  };
}

/** Draws `text` (digits, spaces, '-' and ':') into an <svg class="seg7">. */
function renderSeg7(svg, text) {
  svg.textContent = "";
  const group = document.createElementNS(SVG_NS, "g");
  group.setAttribute("transform", "translate(1.5 0) skewX(-6)");
  let x = 0;
  for (const ch of text) {
    if (ch === ":") {
      for (const y of [6.5, 15.5]) {
        const dot = document.createElementNS(SVG_NS, "rect");
        dot.setAttribute("x", String(x + 1.2));
        dot.setAttribute("y", String(y - 1.3));
        dot.setAttribute("width", "2.6");
        dot.setAttribute("height", "2.6");
        dot.setAttribute("class", "on");
        group.append(dot);
      }
      x += COLON_W;
      continue;
    }
    const lit = SEGMENTS[ch] ?? "";
    for (const [name, points] of Object.entries(digitSegments(x))) {
      const poly = document.createElementNS(SVG_NS, "polygon");
      poly.setAttribute("points", points);
      poly.setAttribute("class", lit.includes(name) ? "on" : "off");
      group.append(poly);
    }
    x += DIGIT_W + 1;
  }
  const height = Number(svg.getAttribute("height")) || DIGIT_H;
  svg.setAttribute("viewBox", `0 0 ${x + 2} ${DIGIT_H}`);
  svg.setAttribute("width", String(Math.round(((x + 2) / DIGIT_H) * height)));
  svg.append(group);
}

document.querySelectorAll("svg[data-seg7]").forEach((svg) => renderSeg7(svg, svg.dataset.seg7));

/* ------------------------------------------------------------- Analyzer */

const LED_COLORS = [
  [0x18, 0xa0, 0x30], [0x3c, 0xf0, 0x60], [0xe8, 0xe0, 0x30], [0xf0, 0x5a, 0x28],
];

function lerpRgb(a, b, t) {
  return `rgb(${a.map((v, i) => Math.round(v + (b[i] - v) * t)).join(",")})`;
}

/** Colour of an LED block at relative height t (0 bottom, 1 top), as in the app. */
function ledColor(t) {
  return t < 0.6
    ? lerpRgb(LED_COLORS[0], LED_COLORS[1], t / 0.6)
    : lerpRgb(LED_COLORS[2], LED_COLORS[3], (t - 0.6) / 0.4);
}

const analyzer = (() => {
  const canvas = document.getElementById("analyzer");
  if (!canvas) return null;
  const ctx = canvas.getContext("2d");
  const BLOCK = 3, BLOCK_GAP = 1, BAR_GAP = 3;
  let width = 0, height = 0, bars = 0, barW = 0, rows = 0;
  let colors = [], levels = [], peaks = [], holds = [];
  let gain = 0, clock = 0, last = 0, running = false, visible = false;

  function resize() {
    const dpr = window.devicePixelRatio || 1;
    width = canvas.clientWidth;
    height = canvas.clientHeight;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    bars = Math.max(12, Math.min(40, Math.floor(width / 14)));
    barW = (width - (bars - 1) * BAR_GAP) / bars;
    rows = Math.floor((height + BLOCK_GAP) / (BLOCK + BLOCK_GAP));
    colors = Array.from({ length: rows }, (_, r) => ledColor(r / Math.max(1, rows - 1)));
    levels = new Array(bars).fill(0);
    peaks = new Array(bars).fill(0);
    holds = new Array(bars).fill(0);
  }

  /** A plausible-looking spectrum: falling slope, a kick drum and hi-hats. */
  function target(i, t) {
    const x = i / (bars - 1);
    const shape = 0.92 - 0.5 * x + 0.08 * Math.sin(i * 1.7);
    const wobble = 0.62 + 0.38 * Math.sin(t * (2.1 + (i % 5) * 0.53) + i * 0.9);
    const kick = Math.exp(-(t % 0.5) / 0.11) * Math.exp(-x / 0.2) * 0.5;
    const hat = Math.exp(-((t + 0.125) % 0.25) / 0.04) * x * x * 0.45;
    const noise = (Math.sin(t * 37.1 + i * 12.9) * 0.5 + 0.5) * 0.08;
    return Math.min(1, Math.max(0, gain * (shape * wobble + kick + hat + noise)));
  }

  function draw() {
    ctx.clearRect(0, 0, width, height);
    for (let i = 0; i < bars; i++) {
      const x = i * (barW + BAR_GAP);
      const lit = Math.round(levels[i] * rows);
      for (let r = 0; r < rows; r++) {
        const y = height - (r + 1) * (BLOCK + BLOCK_GAP) + BLOCK_GAP;
        ctx.fillStyle = r < lit ? colors[r] : "#0b1d10";
        ctx.fillRect(x, y, barW, BLOCK);
      }
      const p = Math.round(peaks[i] * rows);
      if (p > 0) {
        ctx.fillStyle = "#c8c8d8";
        ctx.fillRect(x, height - p * (BLOCK + BLOCK_GAP) + BLOCK_GAP, barW, BLOCK);
      }
    }
  }

  function step(dt) {
    clock += dt;
    for (let i = 0; i < bars; i++) {
      const goal = target(i, clock);
      levels[i] = goal > levels[i] ? levels[i] + (goal - levels[i]) * 0.7 : Math.max(goal, levels[i] - dt * 1.8);
      if (levels[i] >= peaks[i]) {
        peaks[i] = levels[i];
        holds[i] = 0.45;
      } else if ((holds[i] -= dt) < 0) {
        peaks[i] = Math.max(levels[i], peaks[i] - dt * 0.7);
      }
    }
  }

  /** One still frame, for reduced motion or while paused. */
  function still() {
    for (let i = 0; i < bars; i++) {
      levels[i] = target(i, 1.37);
      peaks[i] = Math.min(1, levels[i] + (gain > 0 ? 0.06 : 0));
    }
    draw();
  }

  function frame(now) {
    if (!running) return;
    const dt = Math.min(0.05, (now - last) / 1000 || 0);
    last = now;
    step(dt);
    draw();
    requestAnimationFrame(frame);
  }

  function update() {
    const shouldRun = visible && !document.hidden && !reducedMotion.matches;
    if (shouldRun && !running) {
      running = true;
      last = performance.now();
      requestAnimationFrame(frame);
    } else if (!shouldRun) {
      running = false;
      still();
    }
  }

  resize();
  new ResizeObserver(() => { resize(); if (!running) still(); }).observe(canvas);
  new IntersectionObserver(([entry]) => { visible = entry.isIntersecting; update(); }).observe(canvas);
  document.addEventListener("visibilitychange", update);
  reducedMotion.addEventListener?.("change", update);

  return {
    setGain(g) {
      gain = g;
      if (!running) still();
    },
  };
})();

/* ----------------------------------------------------------------- Knob */

(() => {
  const knob = document.getElementById("knob");
  if (!knob) return;
  const ticks = document.getElementById("knob-ticks");
  const needle = document.getElementById("knob-needle");
  const digits = document.getElementById("knob-digits");
  const status = document.getElementById("knob-status");
  const MAX = 11, SWEEP = 270, START = -135;
  const angleFor = (v) => START + (v * SWEEP) / MAX;
  let value = Number(knob.getAttribute("aria-valuenow")) || 0;
  let shownAngle = angleFor(value);
  let tween = 0;

  // Tick marks around the dial, 0..11; the last one is the orange "11".
  const tickEls = [];
  for (let i = 0; i <= MAX; i++) {
    const a = (angleFor(i) * Math.PI) / 180;
    const line = document.createElementNS(SVG_NS, "line");
    line.setAttribute("x1", String(100 + 74 * Math.sin(a)));
    line.setAttribute("y1", String(100 - 74 * Math.cos(a)));
    line.setAttribute("x2", String(100 + 88 * Math.sin(a)));
    line.setAttribute("y2", String(100 - 88 * Math.cos(a)));
    line.setAttribute("stroke-width", "6");
    line.setAttribute("stroke-linecap", "round");
    ticks.append(line);
    tickEls.push(line);
  }

  function setNeedle(angle) {
    needle.setAttribute("transform", `rotate(${angle.toFixed(2)} 100 100)`);
  }

  function animateNeedle(to) {
    cancelAnimationFrame(tween);
    if (reducedMotion.matches) {
      shownAngle = to;
      setNeedle(to);
      return;
    }
    const from = shownAngle, t0 = performance.now(), dur = 140;
    const tick = (now) => {
      const k = Math.min(1, (now - t0) / dur);
      shownAngle = from + (to - from) * (1 - (1 - k) ** 3);
      setNeedle(shownAngle);
      if (k < 1) tween = requestAnimationFrame(tick);
    };
    tween = requestAnimationFrame(tick);
  }

  function statusFor(v) {
    if (v === 0) return "Silence.";
    if (v < 5) return "Turn it up.";
    if (v < 10) return "Getting there.";
    if (v === 10) return "Most players stop here.";
    return "One louder.";
  }

  function set(v) {
    v = Math.max(0, Math.min(MAX, Math.round(v)));
    const changed = v !== value;
    value = v;
    knob.setAttribute("aria-valuenow", String(v));
    knob.setAttribute("aria-valuetext", `${v} of ${MAX}`);
    tickEls.forEach((el, i) => {
      const hot = i === MAX;
      el.setAttribute("stroke", i <= v && v > 0 ? (hot ? "#f05a28" : "#3cf060") : hot ? "#4a2216" : "#173a22");
    });
    renderSeg7(digits, String(v).padStart(2, " "));
    status.textContent = statusFor(v);
    status.classList.toggle("hot", v === MAX);
    analyzer?.setGain(v === MAX ? 1.35 : v * 0.08);
    if (changed) animateNeedle(angleFor(v));
  }

  /** Maps a pointer position to a knob value by its angle around the centre. */
  function valueFromPointer(e) {
    const r = knob.getBoundingClientRect();
    const dx = e.clientX - (r.left + r.width / 2);
    const dy = e.clientY - (r.top + r.height / 2);
    let deg = (Math.atan2(dx, -dy) * 180) / Math.PI; // 0 = up, clockwise positive
    deg = Math.max(START, Math.min(START + SWEEP, deg));
    return ((deg - START) / SWEEP) * MAX;
  }

  knob.addEventListener("pointerdown", (e) => {
    knob.setPointerCapture(e.pointerId);
    knob.focus({ preventScroll: true });
    set(valueFromPointer(e));
  });
  knob.addEventListener("pointermove", (e) => {
    if (knob.hasPointerCapture(e.pointerId)) set(valueFromPointer(e));
  });
  knob.addEventListener("wheel", (e) => {
    e.preventDefault();
    set(value + (e.deltaY < 0 ? 1 : -1));
  }, { passive: false });
  knob.addEventListener("keydown", (e) => {
    const steps = { ArrowUp: 1, ArrowRight: 1, ArrowDown: -1, ArrowLeft: -1, PageUp: 3, PageDown: -3 };
    if (e.key in steps) set(value + steps[e.key]);
    else if (e.key === "Home") set(0);
    else if (e.key === "End") set(MAX);
    else return;
    e.preventDefault();
  });

  value = -1; // force the first render
  set(Number(knob.getAttribute("aria-valuenow")) || 0);
  shownAngle = angleFor(value);
  setNeedle(shownAngle);
})();

/* ------------------------------------------------------ Platform tweaks */

function detectOS() {
  const ua = navigator.userAgent;
  const platform = navigator.userAgentData?.platform || navigator.platform || "";
  if (/Android|iPhone|iPad|iPod/i.test(ua)) return "mobile";
  if (/mac/i.test(platform) || /Mac OS X/.test(ua)) return "mac";
  if (/linux|x11|cros/i.test(platform) || /Linux|X11/.test(ua)) return "linux";
  if (/win/i.test(platform)) return "windows";
  return "other";
}

const os = detectOS();
const heroDownload = document.getElementById("hero-download");

if (os === "mac") {
  heroDownload.textContent = "Download for Mac";
  heroDownload.dataset.asset = "mac-dmg";
  document.querySelectorAll("kbd[data-mod]").forEach((k) => { k.textContent = "Cmd"; });
  const note = document.querySelector(".mod-note");
  if (note) note.textContent = "Shown for macOS; on Linux, use Ctrl where you see Cmd.";
} else if (os === "linux") {
  heroDownload.textContent = "Download for Linux";
} else {
  heroDownload.textContent = "Download for Linux or macOS";
}
document.querySelectorAll(`[data-platform="${os}"]`).forEach((el) => el.classList.add("recommended"));

/* --------------------------------------------------------- Latest release */

const ASSET_PATTERNS = {
  "mac-dmg": /-macos-universal\.dmg$/,
  "mac-zip": /-macos-universal\.zip$/,
  "deb-amd64": /_amd64\.deb$/,
  "deb-arm64": /_arm64\.deb$/,
  "tar-x86_64": /-linux-x86_64\.tar\.gz$/,
  "tar-aarch64": /-linux-aarch64\.tar\.gz$/,
  sums: /^SHA256SUMS\.txt$/,
};
const CACHE_KEY = "eleven-latest-release";

async function fetchLatestRelease() {
  try {
    const cached = sessionStorage.getItem(CACHE_KEY);
    if (cached) return JSON.parse(cached);
  } catch { /* storage unavailable: fall through to the network */ }

  const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, {
    headers: { Accept: "application/vnd.github+json" },
  });
  if (!res.ok) throw new Error(`GitHub API ${res.status}`);
  const json = await res.json();
  const release = {
    tag: json.tag_name,
    published: json.published_at,
    assets: (json.assets || []).map((a) => ({ name: a.name, url: a.browser_download_url })),
  };
  try { sessionStorage.setItem(CACHE_KEY, JSON.stringify(release)); } catch { /* ignore */ }
  return release;
}

function applyRelease(release) {
  document.querySelectorAll("[data-asset]").forEach((link) => {
    const pattern = ASSET_PATTERNS[link.dataset.asset];
    const asset = pattern && release.assets.find((a) => pattern.test(a.name));
    if (asset) link.href = asset.url;
  });
  if (release.tag) {
    document.querySelectorAll("[data-version]").forEach((el) => { el.textContent = release.tag; });
  }
  if (release.published) {
    const date = new Date(release.published).toLocaleDateString(undefined, {
      year: "numeric", month: "short", day: "numeric",
    });
    document.querySelectorAll("[data-release-date]").forEach((el) => { el.textContent = `, released ${date}`; });
  }
}

// Links already point at the GitHub releases page, so a failure here is harmless.
fetchLatestRelease().then(applyRelease).catch(() => {});
