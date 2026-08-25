<script setup lang="ts">
import AppMain from "../AppMain.vue";
import { installBackend } from "../api/runtime-backend";
import { createTauriBackend } from "../api/tauri-backend";

installBackend(createTauriBackend());
</script>

<template>
  <AppMain />
</template>
