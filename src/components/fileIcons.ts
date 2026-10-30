import type { Component } from "vue";
import {
  Io5CodeWorkingOutline,
  Io5DocumentOutline,
  Io5FilmOutline,
  Io5GlobeOutline,
  Io5ImageOutline,
  Io5ListOutline,
  Io5ShieldCheckmarkOutline,
  Io5TerminalOutline,
} from "vue-icons-plus/io5";
import type { FileKind } from "../utils/file-kind";
import { VscJson } from "vue-icons-plus/vsc";
import { SiLua, SiMarkdown } from "vue-icons-plus/si";

/** 文件树按类型区分的图标：图片、证书、HTML 等一眼可分。 */
export const FILE_KIND_ICONS: Record<FileKind, Component> = {
  lua: SiLua,
  json: VscJson,
  html: Io5GlobeOutline,
  xml: Io5CodeWorkingOutline,
  markdown: SiMarkdown,
  yaml: Io5ListOutline,
  code: Io5TerminalOutline,
  image: Io5ImageOutline,
  media: Io5FilmOutline,
  cert: Io5ShieldCheckmarkOutline,
  text: Io5DocumentOutline,
  file: Io5DocumentOutline,
};
