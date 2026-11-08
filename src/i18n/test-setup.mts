import { setLocale } from "./index";

// node 测试环境无浏览器语言偏好（navigator.language 为 en-US），
// 固定 zh-CN：既有断言的中文文案在恒等映射下保持稳定。
setLocale("zh-CN");
