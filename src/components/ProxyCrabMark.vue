<script setup lang="ts">
import {
  PROXYCRAB_MARK_GRADIENT,
  PROXYCRAB_MARK_PATH,
  PROXYCRAB_MARK_STOPS,
  PROXYCRAB_MARK_VIEWBOX,
} from "../utils/brand-mark";
</script>

<template>
  <svg
    class="proxycrab-mark"
    :viewBox="PROXYCRAB_MARK_VIEWBOX"
    width="18"
    height="18"
    aria-hidden="true"
    focusable="false"
  >
    <defs>
      <linearGradient
        id="proxycrab-mark-fill"
        gradientUnits="userSpaceOnUse"
        :x1="PROXYCRAB_MARK_GRADIENT.x1"
        :y1="PROXYCRAB_MARK_GRADIENT.y1"
        :x2="PROXYCRAB_MARK_GRADIENT.x2"
        :y2="PROXYCRAB_MARK_GRADIENT.y2"
      >
        <stop
          v-for="stop in PROXYCRAB_MARK_STOPS"
          :key="stop.offset"
          :offset="stop.offset"
          :stop-color="stop.color"
        />
      </linearGradient>
    </defs>
    <path fill="url(#proxycrab-mark-fill)" fill-rule="evenodd" :d="PROXYCRAB_MARK_PATH" />
  </svg>
</template>

<style scoped>
.proxycrab-mark {
  display: block;
  flex: none;
}
</style>
