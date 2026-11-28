/* Fetch Tabler outline SVGs for the daro "tabler" icon theme.
   Maps each ZedIcon variant (kebab file name) to candidate Tabler names;
   first existing candidate wins. Normalizes to a single-line SVG without
   the tabler class / bounds rect. */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const OUT_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../assets/tabler");
const BASE = "https://cdn.jsdelivr.net/npm/@tabler/icons/icons";

// kebab(ZedIcon variant) -> [candidate tabler icon names...]
const MAP = {
  "archive": ["archive"],
  "arrow_down": ["arrow-down"],
  "arrow_down_right": ["arrow-down-right"],
  "arrow_left": ["arrow-left"],
  "arrow_right": ["arrow-right"],
  "arrow_right_left": ["arrows-exchange", "transform"],
  "arrow_up": ["arrow-up"],
  "arrow_up_right": ["arrow-up-right"],
  "blocks": ["blocks"],
  "bolt_outlined": ["bolt"],
  "box": ["box"],
  "box_open": ["box-multiple", "package"],
  "chart_bar": ["chart-bar"],
  "check": ["check"],
  "chevron_down": ["chevron-down"],
  "chevron_left": ["chevron-left"],
  "chevron_right": ["chevron-right"],
  "chevron_up": ["chevron-up"],
  "chevron_up_down": ["chevrons-up-down", "selector"],
  "circle": ["circle"],
  "circle_help": ["help", "circle-dashed"],
  "clock": ["clock"],
  "close": ["x"],
  "code": ["code"],
  "columns": ["layout-columns", "columns"],
  "copy": ["copy"],
  "cursor_i_beam": ["text-span", "text-spans", "pointer"],
  "dash": ["minus"],
  "database": ["database"],
  "detail_view": ["layout-sidebar-left", "layout-distribute-vertical"],
  "detail_view_active": ["filled:layout-distribute-vertical", "layout-distribute-vertical"],
  "download": ["download"],
  "ellipsis": ["dots"],
  "expand_vertical": ["arrows-vertical"],
  "eye": ["eye"],
  "eye_off": ["eye-off"],
  "file": ["file"],
  "file_code": ["file-code"],
  "file_plus": ["file-plus"],
  "filter": ["filter"],
  "filter_funnel": ["filter"],
  "fold_vertical": ["arrows-vertical"],
  "folder": ["folder"],
  "folder_open": ["folder-open"],
  "form_view": ["forms", "layout-align-left"],
  "form_view_active": ["filled:forms", "forms"],
  // generic_close / maximize / minimize / restore 不在这里：标题栏窗口按钮是功能件，
  // 任何主题都用 Zed 那张，ZedIcon::is_window_control 也会挡掉。
  "github": ["brand-github"],
  "grid_view": ["layout-grid"],
  "grid_view_active": ["filled:layout-grid", "layout-grid"],
  "hash": ["hash"],
  "history_rerun": ["history"],
  "image": ["photo", "picture"],
  "info": ["info-circle", "info"],
  "key": ["key"],
  "layers": ["layers", "layers-intersect"],
  "left_panel": ["layout-sidebar"],
  "left_panel_active": ["filled:layout-sidebar", "layout-sidebar"],
  "link": ["link"],
  "list_todo": ["list-check", "checklist"],
  "list_tree": ["list-tree"],
  "list_view": ["layout-list", "list"],
  "list_view_active": ["filled:layout-list", "layout-list"],
  "lock": ["lock"],
  "magnifying_glass": ["search"],
  "maximize": ["arrows-maximize"],
  "menu": ["menu-2", "menu"],
  "minimize": ["arrows-minimize"],
  "moon": ["moon"],
  "pencil": ["pencil"],
  "person": ["user"],
  "pin": ["pinned", "pin"],
  "play_filled": ["filled:player-play"],
  "play_outlined": ["player-play"],
  "plug": ["plug"],
  "plus": ["plus"],
  "power": ["power"],
  "rotate_cw": ["rotate-clockwise"],
  "right_panel": ["layout-sidebar-right", "sidebar"],
  "right_panel_active": ["filled:layout-sidebar-right", "layout-sidebar-right"],
  "save": ["device-floppy", "floppy"],
  "server": ["server"],
  "settings": ["settings"],
  "share": ["transform"],
  "skip_back": ["player-skip-back"],
  "skip_forward": ["player-skip-forward"],
  "sort": ["arrows-sort"],
  "sparkle": ["sparkles", "sparkle"],
  "square_minus": ["square-minus"],
  "star": ["star"],
  "star_filled": ["filled:star"],
  "stop": ["player-stop"],
  "sun": ["sun"],
  "table": ["table"],
  "terminal": ["terminal"],
  "text_snippet": ["file-text"],
  "text_wrap": ["text-wrap"],
  "trash": ["trash"],
  "unpin": ["pinned-off", "pin-off"],
  "upload": ["upload"],
  "user_group": ["users", "user-group"],
  "warning": ["alert-triangle"],
  "x_circle": ["circle-x"],
  "wand": ["wand"],
};

function normalize(svg) {
  let s = svg.replace(/\s+/g, " ");
  s = s.replace(/\sclass="[^"]*"/g, "");
  s = s.replace(/<path stroke="none" d="M0 0h24v24H0z"[^>]*\/>/g, "");
  s = s.replace(/>\s+</g, "><");
  return s.trim();
}

async function fetchText(url) {
  const res = await fetch(url);
  if (!res.ok) return null;
  return await res.text();
}

(async () => {
  fs.mkdirSync(OUT_DIR, { recursive: true });
  const entries = Object.entries(MAP);
  const results = await Promise.all(
    entries.map(async ([kebab, candidates]) => {
      for (const cand of candidates) {
        const filled = cand.startsWith("filled:");
        const name = filled ? cand.slice(7) : cand;
        const dir = filled ? "filled" : "outline";
        const body = await fetchText(`${BASE}/${dir}/${name}.svg`);
        if (body && body.startsWith("<svg")) {
          return { kebab, name, dir, svg: normalize(body) };
        }
      }
      return { kebab, name: null };
    })
  );
  const missing = results.filter((r) => !r.name);
  for (const r of results) {
    if (r.name) fs.writeFileSync(path.join(OUT_DIR, `${r.kebab}.svg`), r.svg + "\n");
  }
  console.log(`written: ${results.length - missing.length}/${entries.length}`);
  if (missing.length) console.log("missing:", missing.map((m) => m.kebab).join(", "));
  const aliased = results.filter((r) => r.name && r.name !== r.kebab);
  console.log("renamed:", aliased.map((r) => `${r.kebab}<-${r.name}`).join(", "));
})();
