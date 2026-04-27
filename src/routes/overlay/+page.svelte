<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";

  type Phase =
    | "idle"
    | "recording"
    | "transcribing"
    | "cleaning"
    | "downloading-model";

  const NUM_BARS = 36;
  const BAR_WIDTH = 3;
  const BAR_GAP = 2;
  const BAR_RADIUS = 1.5;
  const MAX_HEIGHT = 36;

  let canvas: HTMLCanvasElement;
  let phase = $state<Phase>("idle");
  let unlisteners: UnlistenFn[] = [];

  // Two arrays: `target` is the latest RMS-driven scroll, `display` smoothly eases toward it.
  const target = new Float32Array(NUM_BARS);
  const display = new Float32Array(NUM_BARS);
  let rafId = 0;
  let lastTime = 0;
  let phaseTime = 0;

  // Latest raw RMS — used to add an instantaneous spike on top of the baseline.
  let latestRms = 0;

  function pushLevel(rms: number) {
    // Perceptual curve so quiet speech (RMS ~0.005) is visible.
    // sqrt(rms * 16) maps 0.005 → 0.28, 0.05 → 0.89, 0.1 → 1.0 (clamped).
    const v = Math.min(1, Math.sqrt(Math.max(0, rms) * 16));
    latestRms = v;
    for (let i = 0; i < NUM_BARS - 1; i++) target[i] = target[i + 1];
    target[NUM_BARS - 1] = v;
  }

  function colorFor(p: Phase): string {
    switch (p) {
      case "recording":
        return "#a5b4fc";
      case "transcribing":
        return "#fbbf24";
      case "cleaning":
        return "#34d399";
      case "downloading-model":
        return "#f472b6";
      default:
        return "#6b7280";
    }
  }

  function tick(now: number) {
    const dt = lastTime ? Math.min(0.05, (now - lastTime) / 1000) : 0;
    lastTime = now;
    phaseTime += dt;

    if (phase === "recording") {
      // Subtle ambient baseline so the user can see the mic is live, scaled
      // up by the most recent RMS so loud speech makes the whole strip swell.
      const ambient = 0.08 + 0.55 * latestRms;
      for (let i = 0; i < NUM_BARS; i++) {
        const wave =
          ambient * (0.6 + 0.4 * Math.sin(phaseTime * 6 + i * 0.55));
        target[i] = Math.max(target[i], wave);
      }
      // Decay the per-frame RMS spike so the wave breathes between events.
      latestRms *= 1 - dt * 4;
    } else if (
      phase === "transcribing" ||
      phase === "cleaning" ||
      phase === "downloading-model"
    ) {
      // Busy state — sine wave so the user sees activity.
      for (let i = 0; i < NUM_BARS; i++) {
        const wave =
          0.45 + 0.4 * Math.sin(phaseTime * 5 + (i * Math.PI * 2) / 8);
        target[i] = Math.max(target[i] * 0.9, wave);
      }
    } else {
      // Idle — let target decay.
      for (let i = 0; i < NUM_BARS; i++) target[i] *= 1 - dt * 0.6;
    }

    // Smooth display toward target.
    const ease = Math.min(1, dt * 14);
    for (let i = 0; i < NUM_BARS; i++) {
      display[i] += (target[i] - display[i]) * ease;
    }

    draw();
    rafId = requestAnimationFrame(tick);
  }

  function draw() {
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
    }
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    const totalWidth = NUM_BARS * BAR_WIDTH + (NUM_BARS - 1) * BAR_GAP;
    const startX = (w - totalWidth) / 2;
    ctx.fillStyle = colorFor(phase);

    for (let i = 0; i < NUM_BARS; i++) {
      const v = display[i];
      const barH = Math.max(2, v * MAX_HEIGHT);
      const x = startX + i * (BAR_WIDTH + BAR_GAP);
      const y = (h - barH) / 2;
      ctx.beginPath();
      ctx.roundRect(x, y, BAR_WIDTH, barH, BAR_RADIUS);
      ctx.fill();
    }
  }

  onMount(async () => {
    unlisteners.push(
      await listen<number>("hearye://level", (e) => pushLevel(e.payload)),
    );
    unlisteners.push(
      await listen<string>("hearye://state", (e) => {
        phase = e.payload as Phase;
        phaseTime = 0;
      }),
    );
    rafId = requestAnimationFrame(tick);
  });

  onDestroy(() => {
    if (rafId) cancelAnimationFrame(rafId);
    for (const u of unlisteners) u();
  });
</script>

<div class="pill">
  <canvas bind:this={canvas}></canvas>
  <button
    class="cancel"
    title="Cancel (Esc)"
    aria-label="Cancel"
    onclick={() => invoke("cancel_recording")}
  >
    ×
  </button>
</div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    background: transparent;
    overflow: hidden;
    user-select: none;
    /* Empty area around the pill must not eat clicks meant for whatever is
       behind the (now larger) window — re-enabled on .pill itself. */
    pointer-events: none;
  }
  .pill {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px 8px 14px;
    background: rgba(18, 20, 24, 0.85);
    backdrop-filter: blur(22px);
    -webkit-backdrop-filter: blur(22px);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 14px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
    width: max-content;
    margin: 36px auto;
    height: 48px;
    box-sizing: border-box;
    pointer-events: auto;
  }
  canvas {
    width: 220px;
    height: 36px;
    display: block;
  }
  .cancel {
    background: transparent;
    border: 0;
    color: #9aa0a6;
    font-size: 18px;
    line-height: 1;
    cursor: pointer;
    padding: 2px 6px;
    border-radius: 6px;
  }
  .cancel:hover {
    color: #f87171;
    background: rgba(255, 255, 255, 0.06);
  }
</style>
