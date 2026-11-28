/* Re-pull the "Zed" icon theme straight from the upstream Zed repository so
   assets/zed holds original artwork only. Same-name files come from
   assets/icons (or assets/icons/file_icons); the handful of daro-only glyphs
   without an upstream twin keep their current redrawn file. */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const OUT_DIR = path.join(ROOT, "assets/zed");
const RAW = "https://raw.githubusercontent.com/zed-industries/zed/main/assets/icons";

// kebab(ZedIcon variant) -> upstream path under assets/icons (null = keep local redraw)
const MAP = {
  "archive": "archive.svg",
  "arrow_down": "arrow_down.svg",
  "arrow_down_right": "arrow_down_right.svg",
  "arrow_left": "arrow_left.svg",
  "arrow_right": "arrow_right.svg",
  "arrow_right_left": "arrow_right_left.svg",
  "arrow_up": "arrow_up.svg",
  "arrow_up_right": "arrow_up_right.svg",
  "blocks": "blocks.svg",
  "bolt_outlined": "bolt_outlined.svg",
  "box": "box.svg",
  "box_open": "box_open.svg",
  "chart_bar": null, // 上游无柱图
  "check": "check.svg",
  "chevron_down": "chevron_down.svg",
  "chevron_left": "chevron_left.svg",
  "chevron_right": "chevron_right.svg",
  "chevron_up": "chevron_up.svg",
  "chevron_up_down": "chevron_up_down.svg",
  "circle": "circle.svg",
  "circle_help": "circle_help.svg",
  "clock": "clock.svg",
  "close": "close.svg",
  "code": "code.svg",
  "columns": "file_tree.svg",
  "copy": "copy.svg",
  "cursor_i_beam": "cursor_i_beam.svg",
  "dash": "dash.svg",
  "database": "file_icons/database.svg",
  "detail_view": null, // 视图四件套取 Zed 的 table/split/notepad 几何，再加 0.1 ↔ 0.8 的选中态内部填充
  "detail_view_active": null,
  "download": "download.svg",
  "ellipsis": "ellipsis.svg",
  "expand_vertical": "expand_vertical.svg",
  "eye": "eye.svg",
  "eye_off": "eye_off.svg",
  "file": "file.svg",
  "file_code": "file_code.svg",
  "file_plus": "square_plus.svg",
  "filter": "filter.svg",
  "filter_funnel": "filter_funnel.svg",
  "fold_vertical": "list_collapse.svg",
  "folder": "folder.svg",
  "folder_open": "folder_open.svg",
  "form_view": null, // 上游 notepad.svg 无选中态配对，按 0.1 ↔ 0.8 自绘
  "form_view_active": null,
  "generic_close": "generic_close.svg",
  "generic_maximize": "generic_maximize.svg",
  "generic_minimize": "generic_minimize.svg",
  "generic_restore": "generic_restore.svg",
  "github": null, // 上游 github.svg 是 Zed 自家徽标，保留 daro 版
  "grid_view": null,
  "grid_view_active": null,
  "hash": "hash.svg",
  "history_rerun": "history_rerun.svg",
  "image": "image.svg",
  "info": "info.svg",
  "key": null, // 上游只有 file_icons 的类型字形，无控件钥匙
  "layers": "blocks.svg",
  "left_panel": "threads_sidebar_left_closed.svg",
  "left_panel_active": "threads_sidebar_left_open.svg",
  "link": "link.svg",
  "list_todo": "list_todo.svg",
  "list_tree": "list_tree.svg",
  "list_view": null, // 上游无纯列表字形：list_todo 带勾选框、list_collapse 带折叠箭头，按 Zed 线稿规范自绘
  "list_view_active": null,
  "lock": "lock.svg",
  "magnifying_glass": "magnifying_glass.svg",
  "maximize": "maximize.svg",
  "menu": "menu.svg",
  "minimize": "minimize.svg",
  "moon": null,
  "pencil": "pencil.svg",
  "person": "person.svg",
  "pin": "pin.svg",
  "play_filled": "play_filled.svg",
  "play_outlined": "play_outlined.svg",
  "plug": null,
  "plus": "plus.svg",
  "power": "power.svg",
  "right_panel": "threads_sidebar_right_closed.svg",
  "right_panel_active": "threads_sidebar_right_open.svg",
  "rotate_cw": "rotate_cw.svg",
  "save": null,
  "server": "server.svg",
  "settings": "settings.svg",
  "share": "share.svg",
  "skip_back": "fast_forward.svg",
  "skip_forward": "fast_forward.svg",
  "sort": "arrow_down.svg",
  "sparkle": "sparkle.svg",
  "square_minus": "square_minus.svg",
  "star": "star.svg",
  "star_filled": "star_filled.svg",
  "stop": "stop.svg",
  "sun": null,
  "table": "table.svg",
  "terminal": "terminal.svg",
  "text_snippet": "file_text_outlined.svg",
  "text_wrap": "text_wrap.svg",
  "trash": "trash.svg",
  "unpin": "unpin.svg",
  "upload": null, // 上游只有 cloud_download，语义不合
  "user_group": "user_group.svg",
  "warning": "warning.svg",
  "wand": "sparkle.svg",
  "x_circle": "x_circle.svg",
};

// gpui 渲染只取 SVG 的 alpha 蒙版再按文字色着色，但写死的灰色会让源码看不清着色意图，
// 统一换成 currentColor。
const recolor = (svg) => svg.replace(/(?:#C6CAD0|#DCE0E5|#000\b|black)/gi, "currentColor");

async function fetchText(url) {
  const res = await fetch(url);
  if (!res.ok) return null;
  const text = await res.text();
  return text.startsWith("<svg") || text.startsWith("<?xml") ? text : null;
}

(async () => {
  const entries = Object.entries(MAP);
  const results = await Promise.all(
    entries.map(async ([kebab, src]) => {
      if (!src) return { kebab, status: "kept" };
      const body = await fetchText(`${RAW}/${src}`);
      if (!body) return { kebab, status: "MISSING", src };
      fs.writeFileSync(path.join(OUT_DIR, `${kebab}.svg`), recolor(body));
      return { kebab, status: src.startsWith("file_icons/") ? "file_icons" : "icons", src };
    })
  );
  const by = (s) => results.filter((r) => r.status === s);
  console.log(`replaced: ${by("icons").length} (icons) + ${by("file_icons").length} (file_icons)`);
  console.log(`kept local redraw: ${by("kept").map((r) => r.kebab).join(", ") || "-"}`);
  const missing = by("MISSING");
  if (missing.length) console.log("MISSING upstream:", missing.map((r) => `${r.kebab}<-${r.src}`).join(", "));
})();
