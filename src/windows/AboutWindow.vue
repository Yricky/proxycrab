<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { proxyCrabMarkSvg } from "../utils/brand-mark";

const canvasRef = ref<HTMLCanvasElement | null>(null);

const CRAB_COUNT = 200;
const MAX_CRABS = 600;
const GRAVITY = 2400;
const MAX_DPR = 2;
const POINTER_RADIUS = 160;
const POINTER_FORCE = 70000;

interface Crab {
  x: number;
  y: number;
  vx: number;
  vy: number;
  size: number;
  radius: number;
  angle: number;
  spin: number;
}

function randomBetween(min: number, max: number): number {
  return min + Math.random() * (max - min);
}

function loadSprite(): Promise<HTMLCanvasElement> {
  return new Promise((resolve, reject) => {
    const size = 384;
    const image = new Image();
    image.onload = () => {
      const sprite = document.createElement("canvas");
      sprite.width = size;
      sprite.height = size;
      const spriteContext = sprite.getContext("2d");
      if (!spriteContext) {
        reject(new Error("2d context unavailable"));
        return;
      }
      spriteContext.drawImage(image, 0, 0, size, size);
      resolve(sprite);
    };
    image.onerror = () => reject(new Error("crab sprite failed"));
    image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(proxyCrabMarkSvg(size))}`;
  });
}

let disposeScene: (() => void) | null = null;
let cancelled = false;

onBeforeUnmount(() => {
  cancelled = true;
  disposeScene?.();
});

onMounted(async () => {
  const canvas = canvasRef.value;
  const context = canvas?.getContext("2d");
  if (!canvas || !context) return;
  const cv = canvas;
  const ctx = context;

  let sprite: HTMLCanvasElement;
  try {
    sprite = await loadSprite();
  } catch {
    return;
  }
  if (cancelled) return;

  let width = 0;
  let height = 0;
  let crabs: Crab[] = [];
  let spawned = false;
  let baselineArea = 0;
  let background = "#232428";
  let frame = 0;
  let last = 0;

  const pointer = { x: -9999, y: -9999, active: false };

  function readTheme(): void {
    const value = getComputedStyle(cv).getPropertyValue("--bg-panel").trim();
    if (value) background = value;
  }

  function makeCrab(): Crab {
    const size = 18 + 62 * Math.pow(Math.random(), 1.5);
    const radius = size * 0.46;
    return {
      x: randomBetween(radius, Math.max(radius + 1, width - radius)),
      y: -size - Math.random() * height * 1.6,
      vx: randomBetween(-30, 30),
      vy: randomBetween(0, 80),
      size,
      radius,
      angle: randomBetween(0, Math.PI * 2),
      spin: randomBetween(-0.6, 0.6),
    };
  }

  function spawnInitial(): void {
    crabs = Array.from({ length: CRAB_COUNT }, makeCrab);
  }

  // 以初始窗口像素数为基准，窗口变大时从顶部补螃蟹，变小时随机移除。
  function adjustPopulation(): void {
    if (!baselineArea) return;
    const ratio = (width * height) / baselineArea;
    const target = Math.max(24, Math.min(MAX_CRABS, Math.round(CRAB_COUNT * ratio)));
    if (target > crabs.length) {
      for (let i = crabs.length; i < target; i++) crabs.push(makeCrab());
    } else {
      while (crabs.length > target) {
        crabs.splice(Math.floor(Math.random() * crabs.length), 1);
      }
    }
  }

  function clampToWorld(crab: Crab): void {
    if (crab.x < crab.radius) {
      crab.x = crab.radius;
      crab.vx = Math.abs(crab.vx) * 0.4;
    } else if (crab.x > width - crab.radius) {
      crab.x = width - crab.radius;
      crab.vx = -Math.abs(crab.vx) * 0.4;
    }
    if (crab.y > height - crab.radius) {
      crab.y = height - crab.radius;
      if (crab.vy > 0) crab.vy *= -0.2;
      crab.vx *= 0.94;
      crab.spin *= 0.9;
    }
  }

  function resize(): void {
    const rect = cv.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1) return;
    width = rect.width;
    height = rect.height;
    const dpr = Math.min(window.devicePixelRatio || 1, MAX_DPR);
    cv.width = Math.round(width * dpr);
    cv.height = Math.round(height * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    if (!spawned) {
      spawnInitial();
      spawned = true;
      baselineArea = width * height;
    } else {
      for (const crab of crabs) clampToWorld(crab);
      adjustPopulation();
    }
  }

  function simulate(dt: number): void {
    for (const crab of crabs) {
      crab.vy += GRAVITY * dt;
      crab.vx *= 0.995;
      crab.vy *= 0.995;

      if (pointer.active) {
        const dx = crab.x - pointer.x;
        const dy = crab.y - pointer.y;
        const distanceSq = dx * dx + dy * dy;
        const reach = POINTER_RADIUS + crab.radius;
        if (distanceSq < reach * reach) {
          const distance = Math.sqrt(distanceSq) || 1;
          const falloff = 1 - distance / reach;
          const push = POINTER_FORCE * falloff * falloff * dt;
          crab.vx += (dx / distance) * push;
          crab.vy += (dy / distance) * push;
        }
      }

      crab.x += crab.vx * dt;
      crab.y += crab.vy * dt;
      crab.angle += crab.spin * dt;
    }

    for (let pass = 0; pass < 2; pass++) {
      for (let i = 0; i < crabs.length; i++) {
        const a = crabs[i]!;
        for (let j = i + 1; j < crabs.length; j++) {
          const b = crabs[j]!;
          const dx = b.x - a.x;
          const dy = b.y - a.y;
          const minimum = a.radius + b.radius;
          const distanceSq = dx * dx + dy * dy;
          if (distanceSq >= minimum * minimum || distanceSq < 1e-6) continue;
          const distance = Math.sqrt(distanceSq);
          const nx = dx / distance;
          const ny = dy / distance;
          const overlap = minimum - distance;
          const invA = 1 / (a.radius * a.radius);
          const invB = 1 / (b.radius * b.radius);
          const invSum = invA + invB;
          a.x -= (nx * overlap * invA) / invSum;
          a.y -= (ny * overlap * invA) / invSum;
          b.x += (nx * overlap * invB) / invSum;
          b.y += (ny * overlap * invB) / invSum;
          const rvx = b.vx - a.vx;
          const rvy = b.vy - a.vy;
          const normal = rvx * nx + rvy * ny;
          if (normal < 0) {
            const impulse = (-(1 + 0.08) * normal) / invSum;
            a.vx -= impulse * invA * nx;
            a.vy -= impulse * invA * ny;
            b.vx += impulse * invB * nx;
            b.vy += impulse * invB * ny;
          }
          const tx = -ny;
          const ty = nx;
          const tangent = rvx * tx + rvy * ty;
          const friction = (-tangent * 0.08) / invSum;
          a.vx -= friction * invA * tx;
          a.vy -= friction * invA * ty;
          b.vx += friction * invB * tx;
          b.vy += friction * invB * ty;
        }
      }
      for (const crab of crabs) clampToWorld(crab);
    }
  }

  function drawTitle(): void {
    const text = "ProxyCrab";
    let size = Math.min(170, width * 0.2);
    const font = (value: number) =>
      `800 ${value}px -apple-system, BlinkMacSystemFont, "PingFang SC", sans-serif`;
    ctx.font = font(size);
    const maxWidth = width * 0.82;
    while (ctx.measureText(text).width > maxWidth && size > 24) {
      size *= 0.96;
      ctx.font = font(size);
    }
    const textWidth = ctx.measureText(text).width;
    const x = (width - textWidth) / 2;
    const y = 18 + size * 0.92;
    const gradient = ctx.createLinearGradient(x, 0, x + textWidth, 0);
    gradient.addColorStop(0, "#02C6F5");
    gradient.addColorStop(0.5, "#9ECE95");
    gradient.addColorStop(1, "#FDB927");
    ctx.fillStyle = gradient;
    ctx.fillText(text, x, y);
  }

  function draw(): void {
    ctx.clearRect(0, 0, width, height);
    ctx.fillStyle = background;
    ctx.fillRect(0, 0, width, height);

    const glow = ctx.createRadialGradient(
      width * 0.5,
      height * 0.02,
      0,
      width * 0.5,
      height * 0.02,
      Math.max(width, height) * 0.75,
    );
    glow.addColorStop(0, "rgba(32, 187, 214, 0.12)");
    glow.addColorStop(0.5, "rgba(255, 181, 45, 0.05)");
    glow.addColorStop(1, "rgba(0, 0, 0, 0)");
    ctx.fillStyle = glow;
    ctx.fillRect(0, 0, width, height);

    for (const crab of crabs) {
      ctx.save();
      ctx.translate(crab.x, crab.y);
      ctx.rotate(crab.angle);
      ctx.drawImage(sprite, -crab.size / 2, -crab.size / 2, crab.size, crab.size);
      ctx.restore();
    }

    drawTitle();
  }

  function tick(now: number): void {
    if (!last) last = now;
    const dt = Math.min((now - last) / 1000, 0.033);
    last = now;
    simulate(dt / 2);
    simulate(dt / 2);
    draw();
    frame = requestAnimationFrame(tick);
  }

  function onPointerMove(event: PointerEvent): void {
    const rect = cv.getBoundingClientRect();
    pointer.x = event.clientX - rect.left;
    pointer.y = event.clientY - rect.top;
    pointer.active = true;
  }

  function onPointerLeave(): void {
    pointer.active = false;
  }

  cv.addEventListener("pointermove", onPointerMove);
  cv.addEventListener("pointerleave", onPointerLeave);
  const resizeObserver = new ResizeObserver(resize);
  resizeObserver.observe(cv);
  const themeObserver = new MutationObserver(readTheme);
  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-theme"],
  });

  readTheme();
  resize();
  frame = requestAnimationFrame(tick);

  disposeScene = () => {
    cancelAnimationFrame(frame);
    resizeObserver.disconnect();
    themeObserver.disconnect();
    cv.removeEventListener("pointermove", onPointerMove);
    cv.removeEventListener("pointerleave", onPointerLeave);
  };
});
</script>

<template>
  <canvas ref="canvasRef" class="about-canvas" aria-label="ProxyCrab" />
</template>

<style scoped>
.about-canvas {
  display: block;
  width: 100%;
  height: 100%;
}
</style>
