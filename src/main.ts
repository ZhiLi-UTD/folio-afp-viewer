import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  openAfp,
  getHexSlice,
  getResourceBytes,
  getPageLayout,
  type DocumentDto,
  type NodeDto,
  type ResourceDto,
  type PageLayoutDto,
} from "./api";

type View = "structure" | "resources" | "render";

interface State {
  doc: DocumentDto | null;
  view: View;
  selectedNodeIndex: number | null;
  selectedResource: number | null;
  query: string;
  openNodes: Set<number>;
  // Render view
  page: number;
  layout: PageLayoutDto | null;
  zoom: number; // 1 = 100%
  fit: boolean; // when true, zoom is recomputed to fit the viewport
}

const state: State = {
  doc: null,
  view: "structure",
  selectedNodeIndex: null,
  selectedResource: null,
  query: "",
  openNodes: new Set(),
  page: 0,
  layout: null,
  zoom: 1,
  fit: true,
};

const MAX_HEX_BYTES = 2048;

// ---- Element handles ----
const el = {
  empty: byId("empty"),
  workspace: byId("workspace"),
  sourceList: byId("source-list"),
  inspector: byId("inspector"),
  hexTitle: byId("hex-title"),
  hexView: byId("hex-view"),
  fileName: byId("file-name"),
  segmented: byId("segmented"),
  search: byId("search") as HTMLInputElement,
  dropOverlay: byId("drop-overlay"),
  render: byId("render"),
  desk: byId("render-desk"),
  canvas: byId("page-canvas") as HTMLCanvasElement,
  pageIndicator: byId("page-indicator"),
  zoomLabel: byId("zoom-label"),
};

function byId(id: string): HTMLElement {
  const node = document.getElementById(id);
  if (!node) throw new Error(`missing #${id}`);
  return node;
}

// ---- Open flow ----
async function pickAndOpen() {
  const path = await open({
    multiple: false,
    filters: [{ name: "AFP", extensions: ["afp", "lst", "prt", "out"] }],
  });
  if (typeof path === "string") await load(path);
}

async function load(path: string) {
  try {
    const doc = await openAfp(path);
    state.doc = doc;
    state.view = "structure";
    state.selectedNodeIndex = null;
    state.selectedResource = null;
    state.query = "";
    state.openNodes = new Set();
    state.page = 0;
    state.layout = null;
    state.fit = true;
    state.zoom = 1;
    // Auto-expand the top two levels for immediate orientation.
    autoExpand(doc.root, 0, 2);
    el.search.value = "";
    el.fileName.textContent = doc.fileName;
    el.empty.hidden = true;
    el.workspace.hidden = false;
    el.segmented.hidden = false;
    el.search.hidden = false;
    setSegment("structure");
    renderSidebar();
    renderSummary();
  } catch (e) {
    el.fileName.textContent = "";
    alert(String(e));
  }
}

function autoExpand(node: NodeDto, depth: number, max: number) {
  if (node.index !== null) state.openNodes.add(node.index);
  if (depth < max) {
    for (const c of node.children) autoExpand(c, depth + 1, max);
  }
}

// ---- Segmented control ----
function setSegment(view: View) {
  state.view = view;
  el.segmented.querySelectorAll<HTMLElement>(".segmented__item").forEach((b) => {
    b.classList.toggle("is-active", b.dataset.view === view);
  });

  const isRender = view === "render";
  el.workspace.hidden = isRender;
  el.render.hidden = !isRender;
  el.search.style.visibility = isRender ? "hidden" : "visible";

  if (isRender) {
    void showPage(state.page);
  } else {
    el.search.placeholder = view === "structure" ? "Search fields" : "Search resources";
    renderSidebar();
  }
}

// ---- Sidebar rendering ----
function renderSidebar() {
  el.sourceList.classList.remove("fade-in");
  void el.sourceList.offsetWidth; // restart animation
  el.sourceList.classList.add("fade-in");
  el.sourceList.innerHTML = "";
  if (!state.doc) return;
  if (state.view === "structure") renderTree();
  else renderResources();
}

function renderTree() {
  const q = state.query.trim().toLowerCase();
  for (const child of state.doc!.root.children) {
    const row = buildTreeNode(child, 0, q);
    if (row) el.sourceList.appendChild(row);
  }
}

/** Returns a DOM fragment for `node` if it or a descendant matches `q`. */
function buildTreeNode(
  node: NodeDto,
  depth: number,
  q: string,
): HTMLElement | null {
  const selfMatch =
    q === "" ||
    node.name.toLowerCase().includes(q) ||
    node.sfid.toLowerCase().includes(q);

  const childEls: HTMLElement[] = [];
  for (const c of node.children) {
    const e = buildTreeNode(c, depth + 1, q);
    if (e) childEls.push(e);
  }
  if (!selfMatch && childEls.length === 0) return null;

  const wrap = document.createElement("div");
  const hasChildren = node.children.length > 0;
  const idx = node.index;
  const isOpen = q !== "" ? true : idx !== null && state.openNodes.has(idx);

  const row = document.createElement("div");
  row.className = "tree-row";
  row.style.paddingLeft = `${depth * 14 + 6}px`;
  if (idx !== null && idx === state.selectedNodeIndex) row.classList.add("is-selected");

  const twist = document.createElement("span");
  twist.className = "tree-row__twist";
  if (!hasChildren) twist.classList.add("is-leaf");
  else if (isOpen) twist.classList.add("is-open");
  twist.textContent = "▶";
  row.appendChild(twist);

  const name = document.createElement("span");
  name.className = "tree-row__name";
  if (!node.known) name.classList.add("is-unknown");
  name.textContent = node.name;
  row.appendChild(name);

  const sfid = document.createElement("span");
  sfid.className = "tree-row__sfid";
  sfid.textContent = node.sfid;
  row.appendChild(sfid);

  const kids = document.createElement("div");
  kids.className = "tree-children";
  kids.hidden = !isOpen;
  for (const e of childEls) kids.appendChild(e);

  twist.addEventListener("click", (ev) => {
    ev.stopPropagation();
    if (!hasChildren || idx === null) return;
    if (state.openNodes.has(idx)) state.openNodes.delete(idx);
    else state.openNodes.add(idx);
    kids.hidden = !kids.hidden;
    twist.classList.toggle("is-open", !kids.hidden);
  });
  row.addEventListener("click", () => {
    if (idx !== null) selectNode(node);
  });

  wrap.appendChild(row);
  wrap.appendChild(kids);
  return wrap;
}

function renderResources() {
  const q = state.query.trim().toLowerCase();
  const list = state.doc!.resources.filter((r) => {
    if (q === "") return true;
    return (
      (r.name ?? "").toLowerCase().includes(q) ||
      r.kind.toLowerCase().includes(q)
    );
  });
  if (list.length === 0) {
    const none = document.createElement("div");
    none.className = "preview__none";
    none.style.margin = "12px";
    none.textContent = "No resources in this document.";
    el.sourceList.appendChild(none);
    return;
  }
  list.forEach((r) => {
    const row = document.createElement("div");
    row.className = "res-row";
    if (r.nodeIndex === state.selectedResource) row.classList.add("is-selected");
    const badge = document.createElement("span");
    badge.className = "res-row__badge";
    badge.textContent = r.kind;
    const name = document.createElement("span");
    name.className = "res-row__name";
    if (!r.name) name.classList.add("is-anon");
    name.textContent = r.name ?? "(unnamed)";
    row.append(badge, name);
    row.addEventListener("click", () => selectResource(r));
    el.sourceList.appendChild(row);
  });
}

// ---- Selection ----
async function selectNode(node: NodeDto) {
  state.selectedNodeIndex = node.index;
  state.selectedResource = null;
  renderSidebar();
  renderInspectorForNode(node);
  await renderHex(node.start, node.end, `Bytes ${node.start}–${node.end}`);
}

async function selectResource(r: ResourceDto) {
  state.selectedResource = r.nodeIndex;
  state.selectedNodeIndex = null;
  renderSidebar();
  renderInspectorForResource(r);
  await renderHex(r.start, r.end, `Resource bytes ${r.start}–${r.end}`);
}

// ---- Inspector ----
function renderInspectorForNode(node: NodeDto) {
  const box = el.inspector;
  box.innerHTML = "";
  box.classList.add("fade-in");
  box.appendChild(h("h2", "insp-title", node.name));
  box.appendChild(h("div", "insp-sub", `SFID ${node.sfid} · ${node.kind}`));

  const g = group("Field");
  g.appendChild(prop("Kind", node.kind));
  g.appendChild(prop("SFID", node.sfid));
  g.appendChild(prop("Known", node.known ? "Yes" : "No (shown as raw)"));
  g.appendChild(prop("Record range", `${node.start} – ${node.end} (${node.end - node.start} bytes)`));
  g.appendChild(
    prop(
      "Data range",
      `${node.dataStart} – ${node.dataEnd} (${node.dataEnd - node.dataStart} bytes)`,
    ),
  );
  g.appendChild(prop("Children", String(node.children.length)));
  box.appendChild(g);
}

function renderInspectorForResource(r: ResourceDto) {
  const box = el.inspector;
  box.innerHTML = "";
  box.classList.add("fade-in");
  box.appendChild(h("h2", "insp-title", r.name ?? "(unnamed resource)"));
  box.appendChild(h("div", "insp-sub", `${r.kind} resource`));

  const g = group("Resource");
  g.appendChild(prop("Kind", r.kind));
  g.appendChild(prop("Name", r.name ?? "—"));
  g.appendChild(prop("Record range", `${r.start} – ${r.end}`));
  box.appendChild(g);

  const previewGroup = group("Preview");
  const holder = document.createElement("div");
  holder.className = "preview";
  previewGroup.appendChild(holder);
  box.appendChild(previewGroup);

  getResourceBytes(state.doc!.docId, r.nodeIndex)
    .then((img) => {
      if (img.format === "jpeg" && img.base64) {
        const image = document.createElement("img");
        image.className = "preview__img";
        image.src = `data:image/jpeg;base64,${img.base64}`;
        image.alt = r.name ?? "image resource";
        holder.appendChild(image);
      } else {
        const none = document.createElement("div");
        none.className = "preview__none";
        none.textContent =
          r.kind === "Image"
            ? "Preview not available for this image compression yet."
            : "No visual preview for this resource type.";
        holder.appendChild(none);
      }
    })
    .catch((e) => {
      holder.appendChild(h("div", "preview__none", String(e)));
    });
}

function renderSummary() {
  if (!state.doc) return;
  const box = el.inspector;
  box.innerHTML = "";
  box.classList.add("fade-in");
  const s = state.doc.summary;
  box.appendChild(h("h2", "insp-title", state.doc.fileName));
  box.appendChild(h("div", "insp-sub", "Document summary"));

  const tiles = document.createElement("div");
  tiles.className = "tiles";
  tiles.appendChild(tile(String(s.pages), "Pages"));
  tiles.appendChild(tile(String(s.fields), "Fields"));
  tiles.appendChild(tile(String(s.resources), "Resources"));
  tiles.appendChild(tile(formatBytes(s.byteSize), "Size"));
  if (s.problems > 0) tiles.appendChild(tile(String(s.problems), "Problems", true));
  box.appendChild(tiles);

  if (state.doc.problems.length > 0) {
    const g = group("Problems");
    const list = document.createElement("div");
    list.className = "problems";
    state.doc.problems.forEach((p) => {
      const row = document.createElement("div");
      row.className = "problem";
      row.append(h("span", "problem__dot", "●"), h("span", "", `${p.message} (byte ${p.at})`));
      list.appendChild(row);
    });
    g.appendChild(list);
    box.appendChild(g);
  } else {
    box.appendChild(h("div", "insp-sub", "No problems found. Select a field to inspect it."));
  }

  el.hexTitle.textContent = "Bytes";
  el.hexView.innerHTML = "";
}

// ---- Hex view ----
async function renderHex(start: number, end: number, title: string) {
  if (!state.doc) return;
  const full = end - start;
  const len = Math.min(full, MAX_HEX_BYTES);
  const b64 = await getHexSlice(state.doc.docId, start, len);
  const bytes = base64ToBytes(b64);
  el.hexTitle.textContent =
    full > len ? `${title} · showing first ${len} of ${full}` : title;
  el.hexView.innerHTML = "";
  for (let off = 0; off < bytes.length; off += 16) {
    const slice = bytes.subarray(off, off + 16);
    const line = document.createElement("div");
    line.className = "hex-line";
    line.append(
      h("span", "hex-line__off", (start + off).toString(16).padStart(6, "0")),
      h("span", "hex-line__bytes", hexPairs(slice)),
      h("span", "hex-line__ascii", ascii(slice)),
    );
    el.hexView.appendChild(line);
  }
}

// ---- Render view ----
const CSS_DPI = 96; // 1 inch = 96 CSS px at 100%

// Decoded on-page images for the current page, keyed by node index.
const pageImages = new Map<number, HTMLImageElement>();

async function showPage(index: number) {
  if (!state.doc) return;
  try {
    const layout = await getPageLayout(state.doc.docId, index);
    state.layout = layout;
    state.page = index;
    await loadPageImages(layout);
    if (state.fit) state.zoom = computeFitZoom(layout);
    drawPage();
    updateRenderChrome();
  } catch (e) {
    el.pageIndicator.textContent = String(e);
  }
}

/** Fetch and decode every on-page image into `pageImages` before drawing. */
async function loadPageImages(layout: PageLayoutDto) {
  pageImages.clear();
  if (!state.doc || layout.images.length === 0) return;
  await Promise.all(
    layout.images.map(async (img) => {
      const data = await getResourceBytes(state.doc!.docId, img.nodeIndex);
      if (data.format !== "jpeg" || !data.base64) return;
      const el = new Image();
      // Only cache images that actually decode; a broken image would make
      // drawImage throw and abort the whole page render.
      const ok = await new Promise<boolean>((resolve) => {
        el.onload = () => resolve(true);
        el.onerror = () => resolve(false);
        el.src = `data:image/jpeg;base64,${data.base64}`;
      });
      if (ok) pageImages.set(img.nodeIndex, el);
    }),
  );
}

function pageSizeAtZoom(layout: PageLayoutDto, zoom: number): [number, number] {
  const scale = (CSS_DPI * zoom) / layout.unitsPerInch;
  return [layout.widthLu * scale, layout.heightLu * scale];
}

function computeFitZoom(layout: PageLayoutDto): number {
  const pad = 48;
  const availW = Math.max(100, el.desk.clientWidth - pad);
  const availH = Math.max(100, el.desk.clientHeight - pad);
  const [w1, h1] = pageSizeAtZoom(layout, 1);
  return Math.max(0.1, Math.min(availW / w1, availH / h1));
}

// Guard against pathological page size × zoom × DPR blowing up the backing
// store. Cap each dimension and the total pixel count; CSS size is preserved so
// the page still lays out correctly, only the backing resolution is reduced.
const MAX_CANVAS_DIM = 8192;
const MAX_CANVAS_PX = 24_000_000;

function drawPage() {
  const layout = state.layout;
  if (!layout) return;
  const [cssW, cssH] = pageSizeAtZoom(layout, state.zoom);
  const dpr = window.devicePixelRatio || 1;
  const canvas = el.canvas;

  // Clamp backing resolution.
  let backW = cssW * dpr;
  let backH = cssH * dpr;
  const dimScale = Math.min(1, MAX_CANVAS_DIM / Math.max(backW, backH));
  const pxScale = Math.sqrt(Math.min(1, MAX_CANVAS_PX / (backW * backH)));
  const clamp = Math.min(dimScale, pxScale);
  backW *= clamp;
  backH *= clamp;

  canvas.width = Math.max(1, Math.round(backW));
  canvas.height = Math.max(1, Math.round(backH));
  canvas.style.width = `${cssW}px`;
  canvas.style.height = `${cssH}px`;

  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  // Map CSS coordinates onto the (possibly clamped) backing store.
  const sx = canvas.width / cssW;
  const sy = canvas.height / cssH;
  ctx.setTransform(sx, 0, 0, sy, 0, 0);

  // Page background.
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, cssW, cssH);

  const scale = (CSS_DPI * state.zoom) / layout.unitsPerInch;

  // Images first, so text can sit on top.
  for (const img of layout.images) {
    const el = pageImages.get(img.nodeIndex);
    if (el) {
      ctx.drawImage(el, img.x * scale, img.y * scale, img.w * scale, img.h * scale);
    }
  }

  // Text runs, sized from the page's estimated text height.
  const fontPx = (layout.fontSizeLu / layout.unitsPerInch) * CSS_DPI * state.zoom;
  ctx.fillStyle = "#111111";
  ctx.textBaseline = "alphabetic";
  ctx.font = `${fontPx}px -apple-system, "SF Pro Text", system-ui, sans-serif`;
  for (const t of layout.texts) {
    ctx.fillText(t.text, t.x * scale, t.y * scale);
  }
}

function updateRenderChrome() {
  const count = state.layout?.pageCount ?? 1;
  el.pageIndicator.textContent = `Page ${state.page + 1} / ${count}`;
  el.zoomLabel.textContent = `${Math.round(state.zoom * 100)}%`;
}

function changePage(delta: number) {
  if (!state.layout) return;
  const next = state.page + delta;
  if (next < 0 || next >= state.layout.pageCount) return;
  void showPage(next);
}

function setZoom(z: number) {
  state.fit = false;
  state.zoom = Math.max(0.1, Math.min(8, z));
  drawPage();
  updateRenderChrome();
}

function fitToViewport() {
  if (!state.layout) return;
  state.fit = true;
  state.zoom = computeFitZoom(state.layout);
  drawPage();
  updateRenderChrome();
}

function exportPng() {
  if (!state.doc || !state.layout) return;
  const url = el.canvas.toDataURL("image/png");
  const a = document.createElement("a");
  const base = state.doc.fileName.replace(/\.[^.]+$/, "");
  a.href = url;
  a.download = `${base}-page${state.page + 1}.png`;
  a.click();
}

// ---- Small DOM / format helpers ----
function h(tag: string, cls: string, text: string): HTMLElement {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  n.textContent = text;
  return n;
}
function group(label: string): HTMLElement {
  const g = document.createElement("div");
  g.className = "insp-group";
  g.appendChild(h("div", "insp-group__label", label));
  return g;
}
function prop(key: string, val: string): HTMLElement {
  const row = document.createElement("div");
  row.className = "prop";
  row.append(h("span", "prop__key", key), h("span", "prop__val", val));
  return row;
}
function tile(num: string, label: string, warn = false): HTMLElement {
  const t = document.createElement("div");
  t.className = warn ? "tile tile--warn" : "tile";
  t.append(h("div", "tile__num", num), h("div", "tile__label", label));
  return t;
}
function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}
function hexPairs(bytes: Uint8Array): string {
  const parts: string[] = [];
  for (let i = 0; i < 16; i++) {
    parts.push(i < bytes.length ? bytes[i].toString(16).padStart(2, "0") : "  ");
    if (i === 7) parts.push("");
  }
  return parts.join(" ");
}
function ascii(bytes: Uint8Array): string {
  let s = "";
  for (const b of bytes) s += b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : ".";
  return s;
}
function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

// ---- Wire up ----
window.addEventListener("DOMContentLoaded", () => {
  byId("open-btn").addEventListener("click", pickAndOpen);
  byId("empty-open").addEventListener("click", pickAndOpen);
  el.segmented.querySelectorAll<HTMLElement>(".segmented__item").forEach((b) => {
    b.addEventListener("click", () => setSegment(b.dataset.view as View));
  });
  el.search.addEventListener("input", () => {
    state.query = el.search.value;
    renderSidebar();
  });

  // Render controls.
  byId("page-prev").addEventListener("click", () => changePage(-1));
  byId("page-next").addEventListener("click", () => changePage(1));
  byId("zoom-fit").addEventListener("click", fitToViewport);
  byId("zoom-in").addEventListener("click", () => setZoom(state.zoom * 1.25));
  byId("zoom-out").addEventListener("click", () => setZoom(state.zoom / 1.25));
  byId("export-png").addEventListener("click", exportPng);
  window.addEventListener("keydown", (e) => {
    if (state.view !== "render") return;
    if (e.key === "ArrowLeft") changePage(-1);
    else if (e.key === "ArrowRight") changePage(1);
  });
  window.addEventListener("resize", () => {
    if (state.view === "render" && state.fit) fitToViewport();
  });

  // Native file drag-and-drop.
  getCurrentWebview()
    .onDragDropEvent((event) => {
      const p = event.payload;
      if (p.type === "over" || p.type === "enter") el.dropOverlay.hidden = false;
      else if (p.type === "leave") el.dropOverlay.hidden = true;
      else if (p.type === "drop") {
        el.dropOverlay.hidden = true;
        const first = p.paths?.[0];
        if (first) load(first);
      }
    })
    .catch(() => {
      /* drag-drop unavailable (e.g. plain browser dev) — Open button still works */
    });
});
