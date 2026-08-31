<script setup lang="ts">
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import AppMain from "../AppMain.vue";
import { installBackend } from "../api/runtime-backend";
import { createTauriBackend } from "../api/tauri-backend";
import { setNativeClipboardWriter } from "../utils/clipboard";

installBackend(createTauriBackend());
setNativeClipboardWriter(writeText);
</script>

<template>
  <AppMain />
</template>
