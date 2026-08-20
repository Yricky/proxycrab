<script setup lang="ts">
import App from "../App.vue";
import { installBackend } from "../api/runtime-backend";
import { createTauriBackend } from "../api/tauri-backend";

installBackend(createTauriBackend());
</script>

<template>
  <App />
</template>
