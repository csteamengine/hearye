<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";

  type Phase =
    | "idle"
    | "recording"
    | "transcribing"
    | "cleaning"
    | "downloading-model"
    | "loading-model";

  type OverlaySize = "small" | "medium" | "large";

  let overlaySize = $state<OverlaySize>("medium");

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
      case "loading-model":
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
      phase === "downloading-model" ||
      phase === "loading-model"
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
    if (phase !== "idle") {
      rafId = requestAnimationFrame(tick);
    } else {
      rafId = 0;
    }
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

    const pad = 4;
    const available = w - pad * 2;
    const barW = Math.max(BAR_WIDTH, (available - (NUM_BARS - 1) * BAR_GAP) / NUM_BARS);
    const step = (available - barW) / (NUM_BARS - 1);
    ctx.fillStyle = colorFor(phase);

    for (let i = 0; i < NUM_BARS; i++) {
      const v = display[i];
      const barH = Math.max(2, v * MAX_HEIGHT);
      const x = pad + i * step;
      const y = (h - barH) / 2;
      ctx.beginPath();
      ctx.roundRect(x, y, barW, barH, BAR_RADIUS);
      ctx.fill();
    }
  }

  onMount(async () => {
    unlisteners.push(
      await listen<number>("hearye://level", (e) => pushLevel(e.payload)),
    );
    unlisteners.push(
      await listen<string>("hearye://overlay-size", (e) => {
        overlaySize = (e.payload as OverlaySize) || "medium";
      }),
    );
    unlisteners.push(
      await listen<string>("hearye://state", (e) => {
        const newPhase = e.payload as Phase;
        if (newPhase === "recording" || newPhase === "idle") {
          target.fill(0);
          display.fill(0);
          latestRms = 0;
          if (canvas) {
            const ctx = canvas.getContext("2d");
            if (ctx) ctx.clearRect(0, 0, canvas.width, canvas.height);
          }
        }
        phase = newPhase;
        phaseTime = 0;
        if (newPhase !== "idle" && !rafId) {
          lastTime = 0;
          rafId = requestAnimationFrame(tick);
        } else if (newPhase === "idle" && rafId) {
          cancelAnimationFrame(rafId);
          rafId = 0;
        }
      }),
    );
  });

  onDestroy(() => {
    if (rafId) cancelAnimationFrame(rafId);
    for (const u of unlisteners) u();
  });
</script>

<div class="pill {overlaySize}">
  {#if overlaySize === "large"}
    <div class="large-top">
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
    <div class="large-bottom">
      <span class="phase-label">
        {#if phase === "recording"}Recording…
        {:else if phase === "transcribing"}Transcribing…
        {:else if phase === "cleaning"}Cleaning up…
        {:else if phase === "downloading-model"}Downloading model…
        {:else if phase === "loading-model"}Loading model…
        {:else}Ready
        {/if}
      </span>
      <span class="hint">Esc to cancel</span>
    </div>
  {:else}
    <canvas bind:this={canvas}></canvas>
    {#if overlaySize !== "small"}
      <button
        class="cancel"
        title="Cancel (Esc)"
        aria-label="Cancel"
        onclick={() => invoke("cancel_recording")}
      >
        ×
      </button>
    {/if}
  {/if}
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
    margin: 36px auto 0;
    height: 48px;
    box-sizing: border-box;
    pointer-events: auto;
  }
  .pill.small {
    padding: 6px 10px;
    height: 32px;
    border-radius: 10px;
    margin-top: 8px;
  }
  .pill.large {
    flex-direction: column;
    padding: 12px 14px 10px 14px;
    height: auto;
    width: auto;
    margin-left: 24px;
    margin-right: 24px;
    border-radius: 16px;
    gap: 6px;
  }
  .large-top {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
  }
  .large-top canvas {
    flex: 1;
    width: auto;
  }
  .large-bottom {
    display: flex;
    justify-content: space-between;
    width: 100%;
    padding: 0 4px;
  }
  canvas {
    width: 220px;
    height: 36px;
    display: block;
  }
  .pill.small canvas {
    width: 140px;
    height: 20px;
  }
  .pill.large canvas {
    height: 40px;
  }
  .phase-label {
    color: rgba(255, 255, 255, 0.7);
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif;
    font-size: 12px;
    font-weight: 500;
  }
  .hint {
    color: rgba(255, 255, 255, 0.35);
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif;
    font-size: 11px;
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
