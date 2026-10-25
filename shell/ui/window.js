// DigUp's window on Windows and Linux (tried out on the Mac first), for the searches the panel is too small for: kind
// filters with what the query found of each kind, the results as a grid of pictures (code as a list), the selected
// one in a preview pane with why it matched, and a status bar with indexing. Before a search it shows the home:
// examples to try, what's indexed, the shortcut. The Mac app's search window (Sources/DigUpApp/SearchWindow.swift) is
// the reference; the helper (`digup search-service`) does the work, and results.js has what a result says.
"use strict";

const TYPING_PAUSE = 120;   // ms after the last keystroke until the search runs (the query was embedded meanwhile)
const LIMIT = 120;
const FILTERS = [
  { key: "screenshots", title: "Screenshots", kinds: ["screenshot"], icon: "screenshot" },
  { key: "images", title: "Images", kinds: ["image"], icon: "image" },
  { key: "documents", title: "Documents", kinds: ["pdf", "doc"], icon: "doc" },
  { key: "video", title: "Video", kinds: ["video"], icon: "video" },
  { key: "audio", title: "Audio", kinds: ["audio"], icon: "audio" },
];
const FILTER_OF_KIND = { screenshot: "screenshots", image: "images", pdf: "documents", doc: "documents", video: "video",
                         audio: "audio" };
/// What Move To, Copy To and Delete work on in the first step: media, not documents or code.
const MEDIA = new Set(["screenshot", "image", "video", "audio"]);
/// Its chip, next to All: the four kinds above, with a key of its own (Ctrl+M) so the numbered chips keep theirs.
const MEDIA_FILTER = { key: "media", title: "Media", kinds: [...MEDIA] };
/// The table's columns, in order; Properties is what the other columns don't say (a picture's size, a PDF's pages…).
const COLUMNS = [
  { key: "name", title: "Name" },
  { key: "size", title: "Size" },
  { key: "type", title: "Type" },
  { key: "date", title: "Date" },
  { key: "properties", title: "Properties" },
];
const EXAMPLES = [
  ["a screenshot of an error message", "screenshots"], ["receipt", "documents"], ["whiteboard notes", "images"],
  ["someone laughing", "audio"], ["a dog on the beach", "video"], ["slides about the budget", "documents"],
];
/// What the home counts, in this order: "136 screenshots, 104 images, 25 PDFs…"
const KIND_ORDER = ["screenshot", "image", "pdf", "doc", "audio", "video", "code"];
const COUNTED = { screenshot: ["screenshot", "screenshots"], image: ["image", "images"], pdf: ["PDF", "PDFs"],
                  doc: ["document", "documents"], audio: ["audio file", "audio files"], video: ["video", "videos"],
                  code: ["code file", "code files"] };
const SEARCH_ICON = '<svg viewBox="0 0 24 24"><circle cx="10" cy="10" r="6.5"/><path d="M15 15 L20.5 20.5"/></svg>';

const $ = (id) => document.getElementById(id);
const $query = $("query"), $content = $("content"), $side = $("side"), $kinds = $("kinds"), $menu = $("menu");
const $batch = $("batch"), $batchSummary = $("batch-summary"), $modal = $("modal"), $report = $("report");
const $announce = $("announce");

const state = {
  query: "",
  filter: null,         // a FILTERS key, "code", or null for everything
  text: "",             // what was searched (without "code:")
  ranked: [],           // the strong results, best first
  weaker: [],           // below half the best one's score: folded
  counts: {},           // strong results of each filter for the query, whatever the filter (the chips' numbers)
  meaning: true,        // the results matched by meaning too (false: words only)
  hasResults: false,    // the results of the query in the field are in
  codePhase: null,      // "hint": "code:" alone; "off": no code folders
  selection: null,      // a result's path
  selected: new Set(),  // the files chosen for Move To, Copy To or Delete (their paths; media only)
  anchor: null,         // where a Shift-click or Shift-arrow range starts
  strong: [],           // the strong results of the query, whatever the filter (the chips' numbers)
  busy: null,           // the file operation under way: {id, kind, done, total, bytes, size, path}
  sheet: false,         // a sheet is up: a confirmation, or an operation's progress
  note: null,           // what the status bar says for a moment after an operation
  showWeaker: false,
  showsPreview: true,
  view: "grid",         // how the results are shown: a grid of pictures, or a table (code stays a list)
  sort: null,           // the table's column and direction: { key, desc }; null keeps the best match first
  navigated: false,     // the selection was moved by the arrow keys or a click since the last edit: ← → move it then
  generation: 0,
  timer: null,
  error: null,
  info: { folders: [], hotkey: null, hasCode: false },
  app: {},              // indexing and the model, from the shell (status.rs): line, progress, caveat, counts
  indexed: null,        // files indexed of each kind (the home)
};
let debugPending = false;
let resultSeq = 0;   // ids for the results a screen reader follows through the field

// MARK: Searching

function isCode() {
  return state.filter === "code" || /^code:/i.test(state.query.trim());
}

/// The query or the filter changed: after a short pause in typing, the search (and the same search of one kind, for a
/// filter), all at once. The query's meaning is embedded on every keystroke, so the pause is the whole wait.
function run() {
  const generation = ++state.generation;
  clearTimeout(state.timer);
  state.navigated = false;
  state.error = null;
  state.note = null;       // a search's count is what matters now, not the last operation's line; its report stays
  clearSelection(false);   // a new query or filter starts with nothing chosen
  const typed = state.query.trim();
  const code = isCode();
  const text = /^code:/i.test(typed) ? typed.slice(5).trim() : typed;
  state.codePhase = null;
  if (!typed || (code && (!state.info.hasCode || !text))) {
    Object.assign(state, { ranked: [], weaker: [], counts: {}, hasResults: false, selection: null });
    if (typed) state.codePhase = state.info.hasCode ? "hint" : "off";
    render();
    return;
  }
  ask({ op: "embed_ahead", q: text, code }).catch(() => {});
  // Results of an earlier query stay until these are in; with none to show, a spinner.
  renderBar();
  if (!state.hasResults) renderContent();
  const filter = state.filter === "media" ? MEDIA_FILTER : FILTERS.find((item) => item.key === state.filter);
  state.timer = setTimeout(async () => {
    try {
      const all = await ask({ op: "search", q: text, code, meaning: true, limit: LIMIT });
      const shown = filter && !code
        ? await ask({ op: "search", q: text, code, meaning: true, limit: LIMIT, kinds: filter.kinds }) : all;
      if (generation !== state.generation) return;
      const strong = all.hits.slice(0, all.strong);   // whatever the filter: the chips count all of them
      Object.assign(state, { counts: countsOf(strong), strong, text, meaning: shown.meaning, hasResults: true,
                             showWeaker: false, ranked: shown.hits.slice(0, shown.strong),
                             weaker: shown.hits.slice(shown.strong) });
      if (!rows().some((hit) => hit.path === state.selection)) state.selection = rows()[0]?.path ?? null;
      render();
      if (debugPending) report();
    } catch (error) {
      if (generation !== state.generation) return;
      state.error = String(error);
      renderStatus();
    }
  }, TYPING_PAUSE);
}

function setQuery(text) {
  $query.value = text;
  queryChanged();
}

function queryChanged() {
  closeBig();
  state.query = $query.value;
  $("clear").hidden = !state.query;
  run();
}

function setFilter(key) {
  if (key === state.filter) return;
  state.filter = key;
  if (key !== "code" && /^code:/i.test(state.query.trim())) {
    // Leaving code for files: "code:" goes from the field.
    $query.value = state.query.trim().slice(5).trim();
    state.query = $query.value;
  }
  run();
}

/// Every result shown, in the order it's shown: the strong ones, then the weaker ones once they're unfolded; in the
/// table, each group by the column chosen.
function rows() {
  const sorted = (hits) => (state.view === "table" && state.sort ? sortedHits(hits) : hits);
  return [...sorted(state.ranked), ...(state.showWeaker ? sorted(state.weaker) : [])];
}

function selectedHit() {
  const shown = rows();
  return shown.find((hit) => hit.path === state.selection) ?? shown[0] ?? null;
}

// MARK: Drawing

function render() {
  renderBar();
  renderContent();
  renderSide();
  renderStatus();
  renderBatch();
}

function renderBar() {
  const code = isCode();
  $("glass").innerHTML = code ? ICONS.code : SEARCH_ICON;
  $query.placeholder = state.filter === "code" ? "Describe the code: “retry a request with backoff”, a name in it…"
    : state.info.hasCode ? "Describe it: “zebra in a video”, “code: retry with backoff”…"
    : "Describe it: “receipt from the hardware store”, “zebra in a video”…";
  $kinds.replaceChildren();
  $kinds.append(chip("All", null, null, state.filter === null && !code, () => setFilter(null), 1));
  if (isWindows) {
    const media = state.filter === "media";
    $kinds.append(chip("Media", null, mediaCount(), media, () => setFilter(media ? null : "media"), "M"));
  }
  FILTERS.forEach((filter, index) => {
    const selected = state.filter === filter.key && !code;
    $kinds.append(chip(filter.title, filter.icon, code ? null : state.counts[filter.key], selected,
                       () => setFilter(selected ? null : filter.key), index + 2));
  });
  if (state.info.hasCode) {
    $kinds.append(element("span", "divider"));
    $kinds.append(chip("Code", "code", code ? state.counts.code : null, code,
                       () => setFilter(state.filter === "code" ? null : "code"), 7));
  }
  $("toggle-view").hidden = code;
  updateViewButton();
}

/// All · Screenshots 4 · Images 9 · …: what the query found of each kind; a click shows only that kind.
function chip(title, icon, count, selected, action, number) {
  const button = element("button", "chip" + (selected ? " selected" : ""));
  if (icon) button.insertAdjacentHTML("beforeend", ICONS[icon]);
  button.append(title);
  if (count != null && state.query.trim()) button.append(element("span", "number", count.toLocaleString()));
  button.title = `${title} (${MOD}${number})`;
  button.setAttribute("aria-pressed", String(selected));
  button.onclick = action;
  return button;
}

function renderContent() {
  thumbnails.disconnect();
  $content.replaceChildren();
  $content.scrollTop = 0;
  if (!state.info.folders.length) $content.append(chooseFolders());
  else if (!state.query.trim()) $content.append(home());
  else if (state.codePhase) $content.append(codeHintOrOff());
  else if (!state.hasResults) $content.append(centered(element("div", "spinner")));
  else if (!state.ranked.length && !state.weaker.length) $content.append(nothingFound());
  else if (isCode()) $content.append(codeList());
  else if (state.view === "table") $content.append(resultsTable());
  else $content.append(grid());
  syncActiveDescendant();
}

function grid() {
  const box = element("div");
  box.append(cells(state.ranked, "grid"));
  if (state.weaker.length) {
    box.append(weakerToggle());
    if (state.showWeaker) box.append(cells(state.weaker, "grid weaker-results"));
  }
  return box;
}

/// The results as a table: Name, Size, Type, Date and Properties, sorted by a click on a heading. The chosen files are
/// ticked here too, and Move To, Copy To and Delete work on them as in the grid.
function resultsTable() {
  const wrap = element("div", "table-wrap");
  const table = element("table", "results-table");
  table.setAttribute("role", "grid");
  table.setAttribute("aria-label", "Results");
  if (isWindows) table.setAttribute("aria-multiselectable", "true");
  const head = element("thead");
  const headRow = element("tr");
  if (isWindows) {
    const cell = element("th", "check");
    cell.setAttribute("role", "columnheader");
    cell.setAttribute("aria-label", "Select");
    headRow.append(cell);
  }
  for (const column of COLUMNS) {
    const cell = element("th", column.key);
    cell.setAttribute("role", "columnheader");
    cell.setAttribute("aria-sort", state.sort?.key === column.key ? (state.sort.desc ? "descending" : "ascending") : "none");
    const button = element("button", "sort" + (state.sort?.key === column.key ? " sorted" : ""));
    button.append(column.title);
    if (state.sort?.key === column.key) button.append(element("span", "arrow", state.sort.desc ? "↓" : "↑"));
    button.title = `Sort by ${column.title}`;
    button.onmousedown = (event) => event.preventDefault();
    button.onclick = () => sortBy(column.key);
    cell.append(button);
    headRow.append(cell);
  }
  head.append(headRow);
  const body = element("tbody");
  for (const hit of sortedHits(state.ranked)) body.append(tableRow(hit));
  if (state.weaker.length) {
    const row = element("tr", "weaker-row");
    const cell = element("td");
    cell.colSpan = COLUMNS.length + (isWindows ? 1 : 0);
    cell.append(weakerToggle());
    row.append(cell);
    body.append(row);
    if (state.showWeaker) for (const hit of sortedHits(state.weaker)) body.append(tableRow(hit));
  }
  table.append(head, body);
  wrap.append(table);
  return wrap;
}

/// One result in the table: its kind's icon, its name and the place that matched, size, type, date, and what the other
/// columns leave out.
function tableRow(hit) {
  const chosen = state.selected.has(hit.path);
  const row = element("tr", "result-row" + (hit.path === state.selection ? " selected" : "") + (chosen ? " checked" : ""));
  row.dataset.path = hit.path;
  row.setAttribute("role", "row");
  row.setAttribute("aria-selected", String(chosen));
  if (isWindows) {
    const cell = tableCell("check");
    if (MEDIA.has(hit.kind)) cell.append(checkboxFor(hit));
    row.append(cell);
  }
  const nameCell = tableCell("name-column");
  const name = element("div", "table-name");
  const icon = element("span", "type-icon");
  icon.innerHTML = ICONS[hit.kind] || ICONS.doc;
  const text = element("span", "name", "‎" + nameOf(hit.path));
  text.title = nameOf(hit.path);
  name.append(icon, text);
  const place = placeOf(hit);
  if (place) name.append(element("span", "place", place));
  nameCell.append(name);
  const type = typeOf(hit), date = dateOf(hit.modified), properties = propertiesOf(hit);
  const typeCell = tableCell("type", type);
  typeCell.title = type;
  const dateCell = tableCell("date", date);
  dateCell.title = date;
  const propertiesCell = tableCell("properties", properties);
  propertiesCell.title = properties;
  row.append(nameCell, tableCell("size", hit.size > 0 ? bytes(hit.size) : ""), typeCell, dateCell, propertiesCell);
  wire(row, hit);
  return row;
}

function tableCell(className, text) {
  const cell = element("td", className, text);
  cell.setAttribute("role", "gridcell");
  return cell;
}

/// A click on a heading sorts by it; a second click turns the order around.
function sortBy(key) {
  state.sort = state.sort?.key === key ? { key, desc: !state.sort.desc } : { key, desc: false };
  renderContent();
}

/// The results by the table's column, in the direction chosen; without a column, the best match first.
function sortedHits(hits) {
  if (!state.sort) return hits;
  const { key, desc } = state.sort;
  const value = (hit) => {
    switch (key) {
      case "name": return nameOf(hit.path).toLocaleLowerCase();
      case "size": return Number(hit.size) || 0;
      case "type": return typeOf(hit).toLocaleLowerCase();
      case "date": return Number(hit.modified) || 0;
      default: return propertiesOf(hit).toLocaleLowerCase();
    }
  };
  return [...hits].sort((a, b) => {
    const left = value(a), right = value(b);
    const order = typeof left === "number" ? left - right : left.localeCompare(right, undefined, { numeric: true });
    return order ? order * (desc ? -1 : 1) : nameOf(a.path).localeCompare(nameOf(b.path), undefined, { numeric: true });
  });
}

/// The table's Type: the extension as file managers show it, or what kind it is when it has none.
function typeOf(hit) {
  const extension = extensionOf(hit.path);
  return extension ? extension.toUpperCase() : KIND_TITLE[hit.kind] || "File";
}

/// What the other columns don't say: a picture's size, a document's pages, a recording's length, code's lines.
function propertiesOf(hit) {
  const info = hit.info || {};
  const parts = [];
  if (info.width && info.height) parts.push(`${info.width} × ${info.height}`);
  if (info.pages) parts.push(Number(info.pages) === 1 ? "1 page" : `${Number(info.pages).toLocaleString()} pages`);
  else if (info.read_to != null) parts.push("partly read");
  if (info.lines) parts.push(Number(info.lines) === 1 ? "1 line" : `${Number(info.lines).toLocaleString()} lines`);
  if (info.duration) parts.push(clock(Number(info.duration)));
  return parts.join(" · ");
}

function cells(hits, className) {
  const box = element("div", className);
  if (className === "grid" && isWindows) {
    box.setAttribute("role", "listbox");
    box.setAttribute("aria-multiselectable", "true");
  }
  for (const hit of hits) box.append(cell(hit));
  return box;
}

/// A result in the grid: its picture in a fixed box (a picture's own shape never sizes the cell), what kind it is,
/// the page or moment that matched, its name and folder.
function cell(hit) {
  const chosen = state.selected.has(hit.path);
  const item = element("div", "cell" + (hit.path === state.selection ? " selected" : "") + (chosen ? " checked" : ""));
  item.dataset.path = hit.path;
  item.setAttribute("role", "option");
  item.setAttribute("aria-selected", String(chosen));
  item.setAttribute("aria-label", `${KIND_TITLE[hit.kind] || "File"}, ${nameOf(hit.path)}, ${reasonOf(hit)}`);
  const picture = element("div", "picture");
  picture.append(cellPicture(hit));
  if (isWindows && MEDIA.has(hit.kind)) picture.append(checkboxFor(hit));
  if (hit.kind !== "image" && hit.kind !== "screenshot") {
    const badge = element("span", "kind-badge");
    badge.innerHTML = ICONS[hit.kind] || ICONS.doc;
    picture.append(badge);
  }
  const place = placeOf(hit);
  if (place) picture.append(element("span", "place on-image", place));
  item.append(picture, element("div", "name", "‎" + nameOf(hit.path)), folderLine(folderOf(hit)));
  wire(item, hit);
  return item;
}

/// The tick that chooses a file for Move To, Copy To or Delete, in the grid's corner or the table's first column.
/// Its press is its own: it neither starts a drag of the file nor takes the keyboard from the field.
function checkboxFor(hit) {
  const box = element("button", "checkbox");
  box.type = "button";
  box.setAttribute("role", "checkbox");
  box.setAttribute("aria-checked", String(state.selected.has(hit.path)));
  box.setAttribute("aria-label", `Select ${nameOf(hit.path)}`);
  box.innerHTML = ICONS.check;
  box.onmousedown = (event) => { event.stopPropagation(); event.preventDefault(); };
  box.onclick = (event) => { event.stopPropagation(); toggleSelected(hit.path); };
  return box;
}

/// The picture of a cell: the photo or screenshot, the frame or page that matched (from the helper, once the cell
/// scrolls into view), a document's passage in small print, or the kind's icon.
function cellPicture(hit) {
  if (hit.kind === "audio") return bigIcon("audio");
  const what = renderOf(hit);
  if (what) {
    const image = element("img", hit.kind === "pdf" ? "page" : "");
    image.alt = "";
    image.dataset.render = JSON.stringify([...what, Math.round(240 * (window.devicePixelRatio || 1))]);
    thumbnails.observe(image);
    return image;
  }
  if (hit.kind === "doc" && hit.excerpt) {
    const card = element("div", "text-card");
    card.dir = "auto";
    card.innerHTML = marked(hit.excerpt.slice(0, 600));
    return card;
  }
  return bigIcon(hit.kind, true);
}

function bigIcon(kind, quiet) {
  const icon = element("div", "big-icon" + (quiet ? " quiet" : ""));
  icon.innerHTML = ICONS[kind] || ICONS.doc;
  return icon;
}

/// Pictures are asked of the helper as their cells come into view: a search can show a hundred of them.
const thumbnails = new IntersectionObserver((entries) => {
  for (const entry of entries) {
    if (!entry.isIntersecting) continue;
    const image = entry.target;
    thumbnails.unobserve(image);
    const [kind, loc, pixels] = JSON.parse(image.dataset.render);
    const path = image.closest("[data-path]").dataset.path;
    rendered(path, kind, loc, pixels)
      .then((file) => {
        image.src = fileURL(file);
        image.closest("[data-path]").dataset.thumb = file;   // the picture under the pointer when it's dragged out
      })
      .catch(() => image.replaceWith(bigIcon(kind === "image" ? "image" : kind, true)));
  }
}, { root: $content, rootMargin: "300px" });

function folderLine(text, tag = "div") {
  const line = element(tag, "folder");
  const inner = document.createElement("bdi");
  inner.textContent = text;
  line.append(inner);
  line.title = text;
  return line;
}

function weakerToggle() {
  const count = state.weaker.length;
  const toggle = element("button", "weaker-toggle" + (state.showWeaker ? " open" : ""));
  toggle.innerHTML = ICONS.chevron;
  toggle.append(state.showWeaker ? "Weaker matches" : `${count} weaker ${count === 1 ? "match" : "matches"}`);
  toggle.onmousedown = (event) => event.preventDefault();
  toggle.onclick = () => {
    state.showWeaker = !state.showWeaker;
    // The weaker ones folded away take their files out of what's chosen.
    const left = new Set(rows().map((hit) => hit.path));
    for (const path of state.selected) if (!left.has(path)) state.selected.delete(path);
    if (state.anchor && !left.has(state.anchor)) state.anchor = null;
    if (!rows().some((hit) => hit.path === state.selection)) state.selection = rows()[0]?.path ?? null;
    const scroll = $content.scrollTop;
    renderContent();
    $content.scrollTop = scroll;
    renderSide();
    renderBatch();
    renderStatus();
  };
  return toggle;
}

/// Code results, one under another: the file, where in its repo, the lines that matched.
function codeList() {
  const box = element("div", "code-list");
  box.setAttribute("role", "listbox");
  box.setAttribute("aria-label", "Code results");
  for (const hit of state.ranked) box.append(codeRow(hit));
  if (state.weaker.length) {
    box.append(weakerToggle());
    if (state.showWeaker) for (const hit of state.weaker) box.append(codeRow(hit, true));
  }
  return box;
}

function codeRow(hit, weaker) {
  const item = element("div", "row" + (hit.path === state.selection ? " selected" : "") + (weaker ? " weaker-results" : ""));
  item.dataset.path = hit.path;
  item.setAttribute("role", "option");
  const icon = element("div", "icon");
  icon.innerHTML = ICONS.code;
  const text = element("div", "text");
  const head = element("div", "head");
  head.append(element("span", "title", "‎" + nameOf(hit.path)));
  const place = placeOf(hit);
  if (place) head.append(element("span", "place", place));
  head.append(folderLine(folderOf(hit), "span"));
  text.append(head);
  if (hit.excerpt) {
    const lines = element("div", "lines");
    lines.innerHTML = marked(hit.excerpt);
    text.append(lines);
  }
  item.append(icon, text);
  wire(item, hit);
  return item;
}

/// A result's clicks: one selects it, two open it, the other button offers what to do with the file. The field keeps
/// the keyboard (as on the Mac: ← → move the selection once you've picked a result).
function wire(item, hit) {
  // Dragged out of the window, the file goes along (to a file manager, a mail, a chat). On Linux, WebKit's own drag with
  // the file's address: GTK starts it from the press, as Wayland wants (a drag started later is refused).
  if (isLinux) {
    item.draggable = true;
    item.ondragstart = (event) => {
      const address = "file://" + hit.path.split("/").map(encodeURIComponent).join("/");
      event.dataTransfer.setData("text/uri-list", address);
      event.dataTransfer.setData("text/plain", hit.path);
      event.dataTransfer.effectAllowed = "copy";
    };
  }
  // Elsewhere: pressed and moved a little, the system's drag (which needs the button down).
  item.onmousedown = (event) => {
    if (event.button !== 0 || isLinux) return;
    event.preventDefault();
    const [x, y] = [event.clientX, event.clientY];
    const moved = (move) => {
      if (Math.hypot(move.clientX - x, move.clientY - y) < 6) return;
      done();
      invoke("drag_out", { path: hit.path, icon: item.dataset.thumb || null }).catch(() => {});
    };
    const done = () => {
      window.removeEventListener("mousemove", moved);
      window.removeEventListener("mouseup", done);
    };
    window.addEventListener("mousemove", moved);
    window.addEventListener("mouseup", done);
  };
  item.onclick = (event) => pick(hit, event);
  item.ondblclick = () => open(hit);
  item.oncontextmenu = (event) => {
    event.preventDefault();
    if (isWindows && MEDIA.has(hit.kind)) {
      // On one of the chosen files: the menu works on all of them; on another: it chooses that one first.
      if (!state.selected.has(hit.path)) { state.selected = new Set([hit.path]); state.anchor = hit.path; }
      select(hit.path);
      paintSelection();
    } else {
      clearSelection();
      select(hit.path);
    }
    const chosen = selectedHits();
    const items = [
      ["Open", () => open(hit)],
      [`Show in ${FILE_MANAGER}`, () => reveal(hit)],
      null,
      ["Copy", () => invoke("copy_file", { path: hit.path }).catch(failed)],
      [chosen.length > 1 ? "Copy Paths" : "Copy Path", copyPaths],
    ];
    if (isWindows && chosen.length) {
      items.push(null, ["Move To…", () => moveOrCopy("move")], ["Copy To…", () => moveOrCopy("copy")],
                 ["Delete (Recycle Bin)", confirmDelete]);
    }
    showMenu(event.clientX, event.clientY, items);
  };
}

function select(path) {
  state.selection = path;
  state.navigated = true;
  for (const item of $content.querySelectorAll("[data-path]")) item.classList.toggle("selected", item.dataset.path === path);
  syncActiveDescendant();
  $content.querySelector(".selected")?.scrollIntoView({ block: "nearest" });
  renderSide();
  if (!$big.hidden) renderBig();
}

/// The result the field's arrows are on, for a screen reader. The field keeps the keyboard (as on the Mac), so it names
/// the chosen option rather than the option taking focus itself.
function syncActiveDescendant() {
  const list = $content.querySelector('[role="listbox"], [role="grid"]');
  if (!list) {
    $query.removeAttribute("aria-activedescendant");
    $query.removeAttribute("aria-controls");
    return;
  }
  if (!list.id) list.id = "results";
  $query.setAttribute("aria-controls", list.id);
  const option = list.querySelector('[role="option"].selected, [role="row"].selected');
  if (!option) {
    $query.removeAttribute("aria-activedescendant");
    return;
  }
  if (!option.id) option.id = `result-${++resultSeq}`;
  $query.setAttribute("aria-activedescendant", option.id);
}

/// Arrows move through the grid: ↑ ↓ by a row, ← → by one.
function move(step) {
  const shown = rows();
  if (!shown.length) return;
  const index = shown.findIndex((hit) => hit.path === state.selection);
  select(shown[Math.max(0, Math.min(shown.length - 1, (index < 0 ? -1 : index) + step))].path);
}

// MARK: Choosing files

/// The media results on show, once each: what Select All and the ranges go through (the whole list, not the part on
/// screen). Documents and code aren't among them.
function operable() {
  const seen = new Set();
  return rows().filter((hit) => {
    if (!isWindows || !MEDIA.has(hit.kind) || seen.has(hit.path)) return false;
    seen.add(hit.path);
    return true;
  });
}

/// The chosen results, in the order they're shown (a path chosen and since gone is left out).
function selectedHits() {
  const byPath = new Map(operable().map((hit) => [hit.path, hit]));
  return [...state.selected].map((path) => byPath.get(path)).filter(Boolean);
}

function selectedSize(hits) {
  return hits.reduce((sum, hit) => sum + (Number(hit.size) || 0), 0);
}

function clearSelection(paint = true) {
  if (!state.selected.size && state.anchor == null) return;
  state.selected.clear();
  state.anchor = null;
  if (paint) paintSelection();
}

function selectAll() {
  state.selected = new Set(operable().map((hit) => hit.path));
  state.anchor = state.selection;
  paintSelection();
}

function toggleSelected(path) {
  if (state.selected.has(path)) state.selected.delete(path);
  else {
    state.selected.add(path);
    state.anchor = path;
  }
  state.navigated = true;
  paintSelection();
}

/// A click on a result: one picks it (and only it), Ctrl adds or takes it away, Shift takes the range from the last
/// one picked. A document or a piece of code can't be moved, copied or deleted: clicking one clears what was chosen.
function pick(hit, event) {
  if (!isWindows || !MEDIA.has(hit.kind)) {
    clearSelection();
    select(hit.path);
    return;
  }
  const list = operable().map((item) => item.path);
  if (event.shiftKey && state.anchor && list.includes(state.anchor) && list.includes(hit.path)) {
    const from = list.indexOf(state.anchor), to = list.indexOf(hit.path);
    if (!event.ctrlKey && !event.metaKey) state.selected.clear();
    for (let index = Math.min(from, to); index <= Math.max(from, to); index += 1) state.selected.add(list[index]);
  } else if (event.ctrlKey || event.metaKey) {
    if (state.selected.has(hit.path)) state.selected.delete(hit.path);
    else state.selected.add(hit.path);
    state.anchor = hit.path;
  } else {
    state.selected = new Set([hit.path]);
    state.anchor = hit.path;
  }
  select(hit.path);
  paintSelection();
}

/// Shift+↑ ↓: the range from where the choosing started to the focus now, in the media list (a row at a time).
function extendSelection(step) {
  const list = operable().map((hit) => hit.path);
  if (!list.length) return;
  const at = list.indexOf(state.selection);
  if (at < 0) return;   // the focus is on something that can't be moved: no range to take yet
  const to = Math.max(0, Math.min(list.length - 1, at + step));
  state.anchor = state.anchor && list.includes(state.anchor) ? state.anchor : list[to];
  const from = Math.max(0, list.indexOf(state.anchor));
  state.selected.clear();
  for (let index = Math.min(from, to); index <= Math.max(from, to); index += 1) state.selected.add(list[index]);
  select(list[to]);
  paintSelection();
}

/// The chosen files' checkboxes and the panel, without rebuilding the grid (which would lose the scroll place).
function paintSelection() {
  for (const item of $content.querySelectorAll("[data-path]")) {
    const chosen = state.selected.has(item.dataset.path);
    item.classList.toggle("checked", chosen);
    item.setAttribute("aria-selected", String(chosen));
    item.querySelector(".checkbox")?.setAttribute("aria-checked", String(chosen));
  }
  renderBatch();
  renderSide();
  renderStatus();
}

function copyPaths() {
  const chosen = selectedHits();
  const paths = chosen.length ? chosen.map((hit) => hit.path) : [state.selection].filter(Boolean);
  if (!paths.length) return;
  navigator.clipboard.writeText(paths.join("\n"))
    .then(() => setNote(paths.length === 1 ? "Copied the path" : `Copied ${paths.length.toLocaleString()} paths`))
    .catch((error) => failed(`The paths couldn't be copied: ${error}`));
}

/// All · Media 14 · Screenshots 4 · …: the media count is the four kinds together.
function mediaCount() {
  return ["screenshots", "images", "video", "audio"].reduce((sum, key) => sum + (state.counts[key] || 0), 0);
}

/// The chips' numbers from a list of strong results.
function countsOf(hits) {
  if (isCode()) return { code: hits.length };
  const counts = {};
  for (const hit of hits) {
    const key = FILTER_OF_KIND[hit.kind];
    if (key) counts[key] = (counts[key] || 0) + 1;
  }
  return counts;
}

// MARK: Moving, copying and deleting

let operationNumber = 0;   // the window's number for its operations, which cancel and the events go by
let operationView = null;  // the sheet's bar, line and Cancel while an operation runs
let sheetReturn = null;    // the control a sheet gives the keyboard back to, if it came from one
let operationSheet = null; // the timer that shows the sheet only if the work takes a moment
let noteTimer = null;

/// The bar between the results and the status bar: how much is chosen and what can be done with it.
function renderBatch() {
  const items = operable();
  $batch.hidden = !items.length;
  if ($batch.hidden) return;
  const chosen = selectedHits();
  $batchSummary.textContent = chosen.length
    ? `${chosen.length.toLocaleString()} selected · ${bytes(selectedSize(chosen))}`
    : "Select files to copy, move, or delete · F5 copy · F6 move · F8 delete";
  $("batch-all").disabled = chosen.length === items.length;
  $("batch-none").disabled = !chosen.length;
  for (const id of ["batch-paths", "batch-copy", "batch-move", "batch-delete"]) $(id).disabled = !chosen.length;
}

/// What `file_operation` takes: the size and date the search knew, so a file that changed since then is left alone.
function chosenItems(files) {
  return files.map((hit) => ({ path: hit.path, size: hit.size ?? null, mtime: hit.modified ?? null }));
}

/// Whether Settings asks first for this operation (`operation_confirmations`); asking is the safe way when it can't
/// be read.
async function asksFirst(kind) {
  try {
    const answer = await invoke("operation_confirmations");
    return kind === "delete" ? answer.delete !== false : answer.moveCopy !== false;
  } catch {
    return true;
  }
}

/// The names in `destination` that are taken (they'd get a number): the sheet says so only when there are any.
async function destinationConflicts(destination, files) {
  try {
    const answer = await invoke("destination_conflicts", {
      destination,
      paths: files.map((file) => file.path),
    });
    return { count: Number(answer.count) || 0, example: answer.example || null };
  } catch {
    return { count: 0, example: null };
  }
}

/// One picker or sheet at a time: the picker and the confirmations are awaited.
let choosing = false;

/// Move To… / Copy To…: the folder (the system's picker), then, for several files, a sheet saying what will happen.
async function moveOrCopy(kind) {
  const files = selectedHits();
  if (!isWindows || !files.length || state.busy || !$modal.hidden || choosing) return;
  choosing = true;
  try {
    let destination = null;
    try {
      destination = await invoke("pick_destination_folder", {
        title: kind === "move" ? "Move to Folder" : "Copy to Folder",
      });
    } catch (error) {
      failed(error);
      return;
    }
    if (!destination) return;
    // One file (the folder was the choice), or Settings turned the question off: the work starts now.
    if (files.length === 1 || !(await asksFirst(kind))) {
      operate(kind, destination, files);
      return;
    }
    const conflicts = await destinationConflicts(destination, files);
    const box = element("div", "modal-box");
    box.append(element("h2", "", `${kind === "move" ? "Move" : "Copy"} ${countOf(files.length, "file")}?`));
    box.append(element("div", "modal-path", destination));
    const taken = conflicts.count
      ? ` · ${countOf(conflicts.count, "name")} ${conflicts.count === 1 ? "is" : "are"} already there: `
        + `${conflicts.count === 1 ? "it gets" : "they get"} a number`
        + (conflicts.example ? ` (${conflicts.example})` : "") + "."
      : " · nothing is overwritten.";
    box.append(element("p", "", bytes(selectedSize(files)) + taken));
    const actions = element("div", "modal-actions");
    const cancel = element("button", "", "Cancel");
    cancel.onclick = closeModal;
    const go = element("button", "primary", kind === "move" ? "Move" : "Copy");
    go.onclick = () => operate(kind, destination, files);
    actions.append(cancel, go);
    box.append(actions);
    showModal(box);
  } finally {
    choosing = false;
  }
}

/// Delete: to the Recycle Bin, with this sheet in between unless Settings turned the question off.
async function confirmDelete() {
  const files = selectedHits();
  if (!isWindows || !files.length || state.busy || !$modal.hidden || choosing) return;
  choosing = true;
  try {
    if (!(await asksFirst("delete"))) {
      operate("delete", null, files);
      return;
    }
    const box = element("div", "modal-box");
    box.append(element("h2", "", `Move ${files.length === 1 ? "this file" : `${files.length} files`} to the Recycle Bin?`));
    box.append(element("p", "", `${bytes(selectedSize(files))}. You can put ${files.length === 1 ? "it" : "them"} back `
      + `from the Recycle Bin.`));
    const actions = element("div", "modal-actions");
    const cancel = element("button", "", "Cancel");
    cancel.onclick = closeModal;
    const go = element("button", "danger", "Move to Recycle Bin");
    go.onclick = () => operate("delete", null, files);
    actions.append(cancel, go);
    box.append(actions);
    showModal(box);
  } finally {
    choosing = false;
  }
}

/// The second, explicit action after the Recycle Bin refused paths that are too long. It shows the scope by folder,
/// not thousands of names; the report behind it keeps the full list. The first button is Keep, so Enter is the safe way
/// out and Escape closes the sheet.
function confirmPermanent(files) {
  if (!isWindows || !files.length || state.busy || !$modal.hidden) return;
  const box = element("div", "modal-box");
  box.append(element("h2", "", `Delete ${countOf(files.length, "file")} permanently?`));
  const lengths = files.map((item) => item.from.length);
  const shortest = lengths.reduce((least, length) => Math.min(least, length), Infinity);
  const longest = lengths.reduce((most, length) => Math.max(most, length), 0);
  box.append(element("p", "", `${bytes(selectedSize(files))}. ${files.length === 1 ? "Its path is" : "Their paths are"} `
    + `${shortest === longest ? `${longest} characters` : `${shortest}–${longest} characters`}; the Recycle Bin limit is 260.`));
  const groups = new Map();
  for (const item of files) {
    const folder = folderOf({ path: item.from });
    groups.set(folder, (groups.get(folder) || 0) + 1);
  }
  const sorted = [...groups].sort((left, right) => right[1] - left[1]);
  for (const [folder, count] of sorted.slice(0, 5)) {
    box.append(element("div", "modal-path", `${folder} — ${countOf(count, "file")}`));
  }
  if (sorted.length > 5) box.append(element("p", "", `and ${sorted.length - 5} more folders.`));
  box.append(element("p", "", "Permanent deletion cannot be undone. The full list stays in the report below."));
  const actions = element("div", "modal-actions");
  const keep = element("button", "", "Keep Files");
  keep.onclick = closeModal;
  const go = element("button", "danger", `Delete ${countOf(files.length, "file")} Permanently`);
  go.onclick = () => operate("delete_permanent", null,
    files.map((item) => ({ path: item.from, size: item.size ?? null, modified: item.mtime ?? null })));
  actions.append(keep, go);
  box.append(actions);
  showModal(box);
}

function countOf(count, one) {
  return `${count.toLocaleString()} ${count === 1 ? one : `${one}s`}`;
}

async function operate(kind, destination, files) {
  if (state.busy || !files.length) return;
  hideReport();   // this operation's report replaces the last one's
  const id = ++operationNumber;
  state.busy = { id, kind, done: 0, total: files.length, path: null, destination, bytes: null, size: null };
  // A single quick file shouldn't flash a sheet; a copy that takes a while gets its bar and Cancel.
  operationSheet = setTimeout(() => {
    operationSheet = null;
    if (state.busy?.id !== id) return;
    showModal(operationBox());
    renderOperation();
  }, 300);
  try {
    const report = await invoke("file_operation", {
      request: { kind, id, destination, items: chosenItems(files) },
    });
    finishOperation(report);
  } catch (error) {
    clearTimeout(operationSheet);
    operationSheet = null;
    state.busy = null;
    operationView = null;
    hideModal();
    failed(error);
  }
}

function operationBox() {
  const { kind, total, destination } = state.busy;
  const box = element("div", "modal-box");
  box.append(element("h2", "", `${verbOf(kind)} ${countOf(total, "file")}…`));
  if (destination) box.append(element("div", "modal-path", destination));
  const bar = element("div", "modal-progress");
  bar.setAttribute("role", "progressbar");
  bar.setAttribute("aria-label", "Progress");
  bar.setAttribute("aria-valuemin", "0");
  bar.setAttribute("aria-valuemax", "100");
  bar.setAttribute("aria-valuenow", "0");
  const fill = element("span");
  bar.append(fill);
  const line = element("div", "modal-line", `0 of ${total.toLocaleString()}`);
  const cancel = element("button", "", "Cancel");
  cancel.onclick = cancelOperation;
  const actions = element("div", "modal-actions");
  actions.append(cancel);
  box.append(bar, line, actions);
  operationView = { bar, fill, line, cancel };
  return box;
}

function renderOperation() {
  if (!state.busy || !operationView) return;
  const { done, total, path, bytes, size } = state.busy;
  const shown = Math.min(done, total);
  // A copy reports the file it's in by the byte, so the bar moves through a big one too.
  const partial = bytes != null && size ? Math.min(bytes / size, 1) : 0;
  const fraction = total ? Math.min((shown + partial) / total, 1) : 0;
  const percent = Math.round(fraction * 100);
  const text = `${shown.toLocaleString()} of ${total.toLocaleString()}`
    + (path ? ` — ${nameOf(path)}` : "") + (partial > 0 && partial < 1 ? ` (${Math.round(partial * 100)}%)` : "");
  operationView.fill.style.width = `${percent}%`;
  operationView.line.textContent = text;
  operationView.bar.setAttribute("aria-valuenow", String(percent));
  operationView.bar.setAttribute("aria-valuetext", text);
}

function cancelOperation() {
  if (!state.busy || !operationView || operationView.cancel.disabled) return;
  invoke("cancel_file_operation", { id: state.busy.id }).catch(() => {});
  operationView.cancel.disabled = true;
  operationView.cancel.textContent = "Cancelling…";
}

function verbOf(kind) {
  return kind === "move" ? "Moving" : kind === "copy" ? "Copying"
    : kind === "delete_permanent" ? "Deleting permanently" : "Moving to the Recycle Bin";
}

function finishOperation(report) {
  clearTimeout(operationSheet);
  operationSheet = null;
  const files = report.items.filter((item) => item.ok);
  state.busy = null;
  operationView = null;
  if (report.kind === "copy") {
    clearSelection();
    renderBatch();
  } else {
    // The rows of the files that left are gone; the ones that failed or were skipped stay, and stay chosen, so they can
    // be tried again.
    forget(files.map((item) => item.from), report.items.filter((item) => !item.ok).map((item) => item.from));
  }
  hideModal();
  renderReport(report);
  setNote(operationNote(report));
}

/// Moved 10 files · 2 renamed · 1 failed (or Stopped: moved 4 of 12).
function operationNote(report) {
  const verb = report.kind === "move" ? "Moved" : report.kind === "copy" ? "Copied"
    : report.kind === "delete_permanent" ? "Permanently deleted" : "Moved";
  let text = report.cancelled ? `Stopped: ${verb.toLowerCase()} ${report.done} of ${report.total}`
    : `${verb} ${countOf(report.done, "file")}`;
  if (report.kind === "delete") text += " to the Recycle Bin";
  if (report.renamed) text += ` · ${report.renamed} renamed`;
  if (report.failed) text += ` · ${report.failed} failed`;
  if (report.skipped) text += ` · ${report.skipped} skipped`;
  if (report.index === false) text += " · index update failed; resyncing";
  return text;
}

/// How many lines of a list show before "Show all": a batch of thousands opens on its summary, not on a screen of
/// names, and the full list is one click away.
const REPORT_SHOWN = 8;

/// What happened, under the results rather than in a sheet: renamed names and files that didn't go stay here until the
/// next operation, without a click to put them away. A clean operation has only the status bar's line.
function renderReport(report) {
  const renamed = report.items.filter((item) => item.ok && item.renamed);
  const problems = report.items.filter((item) => !item.ok);
  const failedIndex = report.index === false;
  if (!renamed.length && !problems.length && !failedIndex) {
    hideReport();
    return;
  }
  $report.replaceChildren();
  $report.append(element("div", "report-head", operationNote(report)));
  if (failedIndex) {
    $report.append(element("div", "report-why",
      "The index couldn't follow the files that changed; DigUp is rescanning your folders."));
  }
  if (renamed.length) {
    $report.append(element("div", "report-label", "Renamed"),
      element("div", "report-why", "The names were taken, so DigUp added a number."));
    $report.append(reportList(renamed, (item) =>
      element("li", "name", `${nameOf(item.from)} → ${nameOf(item.to)}`)));
  }
  if (problems.length) {
    $report.append(element("div", "report-label", "Not done"));
    $report.append(reportList(problems, (item) => {
      const line = element("li");
      line.append(element("div", "name", nameOf(item.from)), element("div", "why", item.error || "it wasn't done"));
      return line;
    }));
  }
  const permanent = problems.filter((item) => item.reason === "path_too_long");
  if (permanent.length) {
    const retry = element("button", "report-more",
      `Delete ${countOf(permanent.length, "file")} Permanently…`);
    retry.onclick = () => confirmPermanent(permanent);
    $report.append(retry);
  }
  $report.hidden = false;
}

/// A report's list: the first few lines, and "Show all" for a batch too big to read at a glance. The full list is made
/// once, on the click, so a move of thousands doesn't put thousands of rows on the page for nothing.
function reportList(items, line) {
  const list = element("ul", "report-list");
  const fill = (count) => {
    for (const item of items.slice(list.childElementCount, count)) list.append(line(item));
  };
  fill(REPORT_SHOWN);
  if (items.length <= REPORT_SHOWN) return list;
  const wrap = element("div");
  const more = element("button", "report-more", `Show all ${items.length.toLocaleString()}`);
  let shown = REPORT_SHOWN;
  const CHUNK = 500;
  more.onclick = () => {
    // A batch of thousands is appended a few hundred at a time, so the click doesn't build the whole list in one task.
    shown = Math.min(items.length, shown + CHUNK);
    fill(shown);
    $report.classList.add("expanded");
    if (shown === items.length) more.remove();
    else more.textContent = `Show ${Math.min(CHUNK, items.length - shown).toLocaleString()} more`;
  };
  wrap.append(list, more);
  return wrap;
}

/// Puts the last operation's details away: a new search or operation starts clean.
function hideReport() {
  $report.hidden = true;
  $report.classList.remove("expanded");
  $report.replaceChildren();
}

/// Takes the files that moved or were deleted out of the results: their rows, the chips' numbers, the selection. The
/// place on screen stays where it was.
function forget(paths, stay = []) {
  if (!paths.length) return;
  const gone = new Set(paths);
  const before = rows();
  const at = before.findIndex((hit) => hit.path === state.selection);
  const scroll = $content.scrollTop;
  const keep = (hit) => !gone.has(hit.path);
  state.ranked = state.ranked.filter(keep);
  state.weaker = state.weaker.filter(keep);
  state.strong = state.strong.filter(keep);
  state.counts = countsOf(state.strong);
  const after = rows();
  state.selected = new Set(stay.filter((path) => after.some((hit) => hit.path === path)));
  state.anchor = stay[stay.length - 1] ?? null;
  state.selection = state.selection && !gone.has(state.selection) ? state.selection
    : after[Math.min(Math.max(at, 0), after.length - 1)]?.path ?? null;
  renderBar();
  renderContent();
  $content.scrollTop = scroll;
  renderSide();
  renderStatus();
  renderBatch();
  if (!$big.hidden) renderBig();
}

/// For a moment after an operation, the status bar's right side says what happened.
function setNote(text) {
  announce(text);
  state.note = text;
  clearTimeout(noteTimer);
  noteTimer = setTimeout(() => {
    state.note = null;
    renderStatus();
  }, 10000);
  renderStatus();
}

/// Says a line once, for a screen reader: the status bar's note, an error. Empty first, so the same line twice is a
/// change again.
function announce(text) {
  $announce.textContent = "";
  requestAnimationFrame(() => { $announce.textContent = text; });
}

// MARK: Sheets

function showModal(box) {
  state.sheet = true;
  // Where the keyboard goes back to: a button in the report or the bar the sheet was opened from. The field is its home
  // anyway, and the option it points at keeps its place.
  if ($modal.hidden) {
    const at = document.activeElement;
    sheetReturn = at instanceof HTMLElement && at !== document.body && at !== $query ? at : null;
  }
  const title = box.querySelector("h2");
  if (title) title.id = "modal-title";
  $modal.replaceChildren(box);
  $modal.hidden = false;
  box.querySelector("button.primary, button.danger, button")?.focus({ preventScroll: true });
}

function hideModal() {
  const back = sheetReturn;
  sheetReturn = null;
  $modal.hidden = true;
  $modal.replaceChildren();
  state.sheet = false;
  // Once the caller has put the report back: its button may have been replaced under the sheet.
  requestAnimationFrame(() => {
    if (back?.isConnected) back.focus({ preventScroll: true });
    else $query.focus();
  });
}

function closeModal() {
  if (state.busy) return;   // an operation under way closes its own sheet, with what happened
  hideModal();
}

/// The grid's columns right now (↑ ↓ move by a row); a list has one.
function columns() {
  const grid = $content.querySelector(".grid");
  return grid ? getComputedStyle(grid).gridTemplateColumns.split(" ").length : 1;
}

function open(hit) {
  openResult(hit).catch(failed);
}

function reveal(hit) {
  invoke("reveal", { path: hit.path }).catch(failed);
}

function failed(error) {
  announce(String(error));
  state.error = String(error);
  renderStatus();
}

// MARK: What shows instead of results

/// Nothing typed yet: the app's mark, a pointer to the field above, examples to click (each with the kind it finds),
/// what's indexed, and the shortcut. A click anywhere else puts the cursor in the field.
function home() {
  const box = element("div", "center home");
  const examples = element("div", "examples");
  const shown = [...EXAMPLES, ...(state.info.hasCode ? [["code: where the settings are saved", "code"]] : [])];
  for (const [text, kind] of shown) {
    const example = element("button", "example");
    example.innerHTML = ICONS[FILTERS.find((filter) => filter.key === kind)?.icon ?? kind];
    example.append(text);
    example.title = `Search for “${text}”`;
    example.onmousedown = (event) => event.preventDefault();
    example.onclick = () => setQuery(text);
    examples.append(example);
  }
  box.append(brandMark(), element("h1", "", "Describe it. Dig it up."),
             element("p", "", "Type what you remember in the search field above: what's in it, what it looks or sounds "
                     + "like, or words from it."),
             element("div", "try", "TRY"), examples, about());
  box.onmousedown = (event) => { if (!event.target.closest("button")) event.preventDefault(); };
  box.onclick = (event) => { if (!event.target.closest("button")) $query.focus(); };
  return box;
}

/// What's in the index ("136 screenshots, 104 images… in Pictures and Documents") and the shortcut.
function about() {
  const box = element("div", "about");
  const kinds = state.indexed && KIND_ORDER.filter((kind) => state.indexed[kind] > 0);
  if (kinds?.length) {
    const line = element("div", "line");
    const counts = element("div", "kind-counts");
    for (const kind of kinds) {
      const item = element("span");
      item.innerHTML = ICONS[kind];
      item.append(state.indexed[kind].toLocaleString());
      counts.append(item);
    }
    counts.title = kinds.map((kind) => counted(kind, state.indexed[kind])).join(", ");
    line.append(counts, element("span", "", `in ${folderNames()}`));
    box.append(line);
  } else if (state.app.searchable > 0) {
    box.append(element("div", "line", `${state.app.searchable.toLocaleString()} files to search in ${folderNames()}`));
  }
  if (state.info.hotkey) {
    const line = element("div", "line");
    const caps = element("span", "keycaps");
    for (const key of keysOf(state.info.hotkey)) caps.append(element("span", "keycap", key));
    line.append(caps, element("span", "", "opens DigUp from any app"));
    box.append(line);
  }
  return box;
}

/// "1 screenshot", "25 PDFs", "6 audio files"
function counted(kind, count) {
  const [one, many] = COUNTED[kind] || ["file", "files"];
  return `${count.toLocaleString()} ${count === 1 ? one : many}`;
}

/// "Pictures, Documents and Videos", or "5 folders".
function folderNames() {
  const names = state.info.folders.map((folder) => folder.split(/[\\/]/).filter(Boolean).pop() || folder);
  return names.length <= 3 ? new Intl.ListFormat("en", { type: "conjunction" }).format(names) : `${names.length} folders`;
}

/// "Ctrl+Shift+Space" as keys; a Mac's "⇧⌘Space" too.
function keysOf(hotkey) {
  if (hotkey.includes("+")) return hotkey.split("+").filter(Boolean);
  const signs = [...hotkey.replace(/[^⌃⌥⇧⌘]/g, "")];
  return [...signs, hotkey.replace(/[⌃⌥⇧⌘]/g, "")].filter(Boolean);
}

function brandMark() {
  const mark = element("div", "mark");
  const icon = element("img");
  icon.src = "icon.png";
  icon.alt = "";
  mark.append(icon);
  return mark;
}

function chooseFolders() {
  const button = element("button", "button primary", "Choose Folders…");
  button.onclick = () => invoke("choose_folders").catch(failed);
  return centered(brandMark(), element("h2", "", "Choose folders to search"),
                  element("p", "", "DigUp only looks where you tell it to."), button);
}

function nothingFound() {
  const icon = element("div", "quiet-icon");
  icon.innerHTML = isCode() ? ICONS.code : SEARCH_ICON;
  return centered(icon, element("h3", "", `No good matches for “${state.query.trim()}”`),
                  element("p", "", isCode() ? "Try saying what the code does, or a name that's in it."
                    : "Try describing what's in it: what you'd see or hear, or words you remember."));
}

/// "code:" with nothing after it (what to type), or without code folders (what code search is).
function codeHintOrOff() {
  const icon = element("div", "brand-icon");
  icon.innerHTML = ICONS.code;
  if (state.codePhase === "off") {
    const actions = element("div", "actions");
    const choose = element("button", "primary", "Choose Code Folders…");
    choose.onclick = () => invoke("choose_code_folders").catch(failed);
    actions.append(choose);
    return centered(icon, element("h3", "", "Code search is off"),
                    element("p", "", "DigUp can find code by what it does, in folders you choose for it. Code gets an "
                            + "index of its own and never shows up in other searches."), actions);
  }
  return centered(icon, element("h3", "", "Search your code"),
                  element("p", "", "Say what the code does, or type a name that's in it."));
}

function centered(...children) {
  const box = element("div", "center");
  box.append(...children);
  return box;
}

// MARK: The preview pane

let playing = null;   // the audio of the selected result, while it plays

function renderSide() {
  stopPlaying();
  const shown = state.hasResults && !state.codePhase && state.query.trim();
  const hit = shown ? selectedHit() : null;
  $side.hidden = !state.showsPreview || !hit;
  $side.replaceChildren();
  if (!hit) return;
  $side.append(previewPicture(hit), facts(hit));
  const chosen = shown && isWindows ? selectedHits() : [];
  if (chosen.length > 1) $side.append(selectionSummary(chosen));
  $side.append(why(hit), actions(hit));
  fitPreview();
}

/// The picture takes the room the facts and actions leave. Its max-height is set in pixels: a percentage of the frame's
/// own, content-sized height wouldn't limit it (the picture would be clipped by the frame).
function fitPreview() {
  const box = $side.querySelector(".preview-picture");
  if (!box || !$side.clientHeight) return;
  box.style.setProperty("--preview-height", `${box.clientHeight}px`);
}

// A taller or shorter window, or the pane hidden and shown again, gives the picture a different room.
new ResizeObserver(fitPreview).observe($side);

/// Several files chosen: under the active one's facts, how many, how much, and of what kinds. The files themselves
/// are ticked in the results, so the pane keeps showing the one being looked at.
function selectionSummary(hits) {
  const box = element("div", "selection-summary");
  const head = element("div", "selection-head");
  head.append(element("strong", "", `${hits.length} files`), element("span", "", bytes(selectedSize(hits))));
  const counts = new Map();
  for (const hit of hits) counts.set(hit.kind, (counts.get(hit.kind) || 0) + 1);
  const kinds = [...counts].sort((a, b) => KIND_ORDER.indexOf(a[0]) - KIND_ORDER.indexOf(b[0]))
    .map(([kind, count]) => countOf(count, COUNTED[kind]?.[0] || "file"));
  box.append(head, element("div", "selection-kinds", kinds.join(" · ")));
  return box;
}

/// The big picture: the photo or screenshot, the frame at the moment (it plays from there), the page, the moment in a
/// recording, the passage that matched, or the lines of code.
function previewPicture(hit) {
  const box = element("div", "preview-picture");
  if (hit.kind === "audio") {
    box.append(audioMoment(hit));
    box.style.height = "150px";
    box.style.flex = "none";
    return box;
  }
  if (hit.kind === "code") {
    box.append(codeLines(hit));
    addPlace(box, span(hit));
    return box;
  }
  if (hit.segment === "chunk") {
    box.append(passage(hit));
    const parts = Number(hit.info?.chunks || 0);
    if (hit.loc != null && parts > 1) addPlace(box, `Part ${Math.trunc(hit.loc)} of ${parts}`);
    return box;
  }
  const what = renderOf(hit);
  if (!what) {
    const placeholder = element("div", "placeholder");
    placeholder.innerHTML = ICONS[hit.kind] || ICONS.doc;
    placeholder.ondblclick = () => open(hit);
    box.append(placeholder);
    return box;
  }
  const frame = element("div", "frame");
  const image = element("img");
  image.alt = "";
  frame.append(image);
  // The cell's picture first (already drawn), then the bigger one.
  const small = renders.get(`${hit.path}|${what[0]}|${what[1]}|${Math.round(240 * (window.devicePixelRatio || 1))}`);
  small?.then((file) => { if (!image.dataset.big) image.src = fileURL(file); }).catch(() => {});
  rendered(hit.path, what[0], what[1], Math.round(460 * (window.devicePixelRatio || 1)))
    .then((file) => { image.dataset.big = "1"; image.src = fileURL(file); })
    .catch(() => {});
  image.ondblclick = () => open(hit);
  const place = placeOf(hit);
  if (place) {
    const marks = element("div", "marks" + (hit.kind === "video" ? " dim" : ""));
    marks.append(element("span", "place on-image", place));
    const duration = Number(hit.info?.duration || 0);
    if (hit.kind === "video" && hit.loc != null && duration > 0) {
      marks.append(momentBar(hit.loc, hit.loc_end ?? hit.loc + 3, duration));
    }
    frame.append(marks);
  }
  if (hit.kind === "video") {
    // ▶ plays it here from the moment that matched.
    const play = element("button", "play");
    play.innerHTML = ICONS.play;
    play.title = `Play from ${clock(hit.loc ?? 0)}`;
    play.onclick = () => {
      const video = element("video");
      video.src = `${fileURL(hit.path)}#t=${hit.loc ?? 0}`;
      video.controls = true;
      video.autoplay = true;
      box.replaceChildren(video);
    };
    frame.append(play);
  }
  box.append(frame);
  return box;
}

// MARK: The big preview (Space, as Quick Look on the Mac)

const $big = element("div", "big");
$big.hidden = true;
$big.onclick = (event) => { if (event.target === $big) closeBig(); };
document.body.append($big);

/// Space shows the selected result over the window, big; Space, Esc or a click beside it puts it away. The arrows move
/// through the results under it, and ↩ opens the one shown.
function toggleBig() {
  if (!$big.hidden) return closeBig();
  stopPlaying();
  $big.hidden = false;
  renderBig();
}

function closeBig() {
  $big.hidden = true;
  $big.replaceChildren();   // its video or sound stops with it
}

function renderBig() {
  const hit = state.hasResults && state.query.trim() ? selectedHit() : null;
  if (!hit) return closeBig();
  const sheet = element("div", "sheet");
  const head = element("div", "head");
  const title = element("div", "title");
  title.append(element("span", "name", nameOf(hit.path)));
  const place = placeOf(hit);
  if (place) title.append(element("span", "place", place));
  const opening = element("button", "primary", "Open");
  opening.onclick = () => { closeBig(); open(hit); };
  const close = element("button", "close", "×");
  close.title = "Close (Space)";
  close.onclick = closeBig;
  head.append(title, opening, close);
  sheet.append(head, bigView(hit));
  $big.replaceChildren(sheet);
}

/// The result as big as the window allows: the picture or the page, the video or the recording from the moment, the
/// passage or the lines of code.
function bigView(hit) {
  const view = element("div", "view");
  if (hit.kind === "code" || hit.segment === "chunk") {
    view.classList.add("text");
    view.append(hit.kind === "code" ? codeLines(hit) : passage(hit));
    return view;
  }
  if (hit.kind === "video" || hit.kind === "audio") {
    const media = element(hit.kind === "video" ? "video" : "audio");
    media.src = `${fileURL(hit.path)}#t=${hit.loc ?? 0}`;
    media.controls = true;
    media.autoplay = true;
    view.append(media);
    return view;
  }
  const what = renderOf(hit);
  if (!what) {
    const placeholder = element("div", "placeholder");
    placeholder.innerHTML = ICONS[hit.kind] || ICONS.doc;
    view.append(placeholder);
    return view;
  }
  const image = element("img");
  image.alt = "";
  const extension = hit.path.split(".").pop().toLowerCase();
  if (what[0] === "image" && SHOWN_AS_IS.has(extension)) {
    image.src = fileURL(hit.path);
  } else {
    const pixels = Math.round(Math.max(innerWidth, innerHeight) * (window.devicePixelRatio || 1));
    rendered(hit.path, what[0], what[1], pixels).then((file) => { image.src = fileURL(file); }).catch(() => {});
  }
  image.ondblclick = () => { closeBig(); open(hit); };
  view.append(image);
  return view;
}

function addPlace(box, text) {
  if (!text) return;
  box.style.position = "relative";
  box.append(element("span", "place on-image", text));
}

/// Where a moment sits in a recording: a track with the matched stretch lit (and where playback is).
function momentBar(start, end, duration, playhead) {
  const bar = element("div", "moment-bar");
  const from = Math.max(0, Math.min(1, start / duration)), to = Math.max(from, Math.min(1, end / duration));
  const lit = element("div", "lit");
  lit.style.left = `${from * 100}%`;
  lit.style.width = `${(to - from) * 100}%`;
  bar.append(lit);
  if (playhead != null) {
    const head = element("div", "head");
    head.style.left = `${Math.max(0, Math.min(1, playhead / duration)) * 100}%`;
    bar.append(head);
  }
  bar.title = `At ${clock(start)} of ${clock(duration)}`;
  return bar;
}

/// An audio result: the stretch that matched on the file's timeline, and ▶ to hear it from there.
function audioMoment(hit) {
  const box = element("div", "audio-moment");
  const start = hit.loc ?? 0, end = hit.loc_end ?? start + 30;
  const duration = Number(hit.info?.duration || 0);
  const controls = element("div", "controls");
  const wave = element("div", "wave");
  wave.innerHTML = ICONS.audio;
  const toggle = element("button", "toggle play-icon");
  toggle.innerHTML = ICONS.play;
  toggle.title = `Play from ${clock(start)}`;
  controls.append(wave, toggle);
  const line = element("div", "span", `${clock(start)} – ${clock(end)}` + (duration ? ` of ${clock(duration)}` : ""));
  let bar = duration > 0 ? momentBar(start, end, duration) : null;
  box.append(controls, ...(bar ? [bar] : []), line);
  const showPlaying = () => {
    const on = playing != null && !playing.paused;
    toggle.innerHTML = on ? ICONS.pause : ICONS.play;
    toggle.classList.toggle("play-icon", !on);
  };
  toggle.onclick = () => {
    if (!playing) {
      const audio = new Audio(`${fileURL(hit.path)}#t=${start}`);
      audio.ontimeupdate = () => {
        line.textContent = `${clock(audio.currentTime)} – playing from ${clock(start)}`;
        if (bar) {
          const moved = momentBar(start, end, duration, audio.currentTime);
          bar.replaceWith(moved);
          bar = moved;
        }
      };
      audio.onplay = audio.onpause = showPlaying;
      audio.onended = () => {
        stopPlaying();
        showPlaying();
      };
      playing = audio;
    }
    if (playing.paused) playing.play().catch(() => {});
    else playing.pause();
  };
  return box;
}

function stopPlaying() {
  if (!playing) return;
  playing.pause();
  playing.removeAttribute("src");
  playing = null;
}

/// The part of a document that matched, on a page, the words marked.
function passage(hit) {
  const page = element("div", "passage");
  const found = foundWords(hit.excerpt);
  const show = (text) => {
    page.dir = isRightToLeft(text) ? "rtl" : "ltr";
    page.innerHTML = highlighted(passageAround(text, [...found, ...queryWords(state.text)]), state.text, found);
  };
  show((hit.excerpt ?? "").replace(/[«»]/g, ""));
  ask({ op: "text", segment: hit.segment_id, code: false })
    .then((reply) => { if (reply.text) show(reply.text); })
    .catch(() => {});
  page.ondblclick = () => open(hit);
  return page;
}

/// The lines of code that matched, numbered as in the file: the query's words marked, the line it opens at lit, and
/// scrolled to it. A notebook's cells have no line numbers of their own, so theirs count from 1.
function codeLines(hit) {
  const box = element("div", "code-lines");
  const first = hit.segment === "lines" ? Math.max(1, Math.trunc(hit.loc ?? 1)) : 1;
  const focus = hit.segment === "lines" && hit.focus != null ? Math.trunc(hit.focus) - first : null;
  const found = foundWords(hit.excerpt);
  const show = (text) => {
    const lines = text.split(/\r?\n/).map((line) => line.replaceAll("\t", "    "));
    const width = String(first + Math.max(lines.length, 1)).length;
    box.replaceChildren();
    let firstMarked = null;
    lines.forEach((line, offset) => {
      const row = element("div", "line" + (offset === focus ? " focus" : ""));
      const number = element("span", "number", String(first + offset));
      number.style.width = `${width}ch`;
      const code = element("span");
      code.innerHTML = highlighted(line || " ", state.text, found);
      if (firstMarked == null && code.querySelector("mark")) firstMarked = offset;
      row.append(number, code);
      box.append(row);
    });
    const target = focus ?? firstMarked;
    if (target != null && target > 3) box.children[target - 2]?.scrollIntoView({ block: "start" });
  };
  ask({ op: "text", segment: hit.segment_id, code: true })
    .then((reply) => show(reply.text || (hit.excerpt ?? "").replace(/[«»]/g, "")))
    .catch(() => show((hit.excerpt ?? "").replace(/[«»]/g, "")));
  box.ondblclick = () => open(hit);
  return box;
}

function facts(hit) {
  const box = element("div", "facts");
  const title = element("div", "title", "‎" + nameOf(hit.path));
  title.title = nameOf(hit.path);
  const folder = folderLine(folderOf(hit), "button");
  folder.title = `Show in ${FILE_MANAGER}`;
  folder.onclick = () => reveal(hit);
  box.append(title, folder, element("div", "details", details(hit)));
  return box;
}

/// "Matches the frame at 0:06", and the words it has, if any.
function why(hit) {
  const box = element("div", "why");
  const reason = element("div", "reason" + (matchedMeaning(hit) ? "" : " words"));
  reason.innerHTML = matchedMeaning(hit) ? ICONS.found : ICONS.words;
  reason.append(element("span", "", reasonOf(hit)));
  box.append(reason);
  // Code shows its lines above, the words marked there.
  if (hit.excerpt && hit.kind !== "code") {
    const excerpt = element("div", "excerpt");
    excerpt.dir = "auto";
    excerpt.innerHTML = marked(hit.excerpt);
    box.append(excerpt);
  }
  return box;
}

function actions(hit) {
  const box = element("div", "actions");
  const openButton = element("button", "primary", "Open");
  openButton.onclick = () => open(hit);
  const revealButton = element("button", "", `Show in ${FILE_MANAGER}`);
  revealButton.onclick = () => reveal(hit);
  box.append(openButton, revealButton);
  return box;
}

// MARK: The status bar

function renderStatus() {
  const app = state.app;
  $("status-line").textContent = app.line || "";
  const ring = $("ring");
  ring.hidden = app.progress == null;
  if (app.progress != null) {
    const length = 2 * Math.PI * 4.6;
    ring.innerHTML = `<svg viewBox="0 0 12 12"><circle class="track" cx="6" cy="6" r="4.6"/><circle class="done" cx="6" `
      + `cy="6" r="4.6" stroke-dasharray="${(length * Math.max(0.02, app.progress)).toFixed(2)} ${length.toFixed(2)}"/></svg>`;
  }
  let count = "";
  const chosen = isWindows && state.hasResults && state.query.trim() ? selectedHits() : [];
  if (state.error) count = `DigUp's helper: ${state.error}`;
  else if (state.note) count = state.note;
  else if (chosen.length) count = `${chosen.length.toLocaleString()} selected · ${bytes(selectedSize(chosen))}`;
  else if (state.hasResults && state.query.trim()) {
    const strong = state.ranked.length, weaker = state.weaker.length;
    count = app.caveat || (!state.meaning ? "Words only: meaning search isn't available"
      : `${strong} ${strong === 1 ? "result" : "results"}` + (weaker ? ` · ${weaker} weaker` : ""));
  }
  $("count").textContent = count;
  renderUpdate(app.update);
}

/// An update waiting (updates.rs), as on the Mac: a line on the right of the status bar, then its download.
function renderUpdate(update) {
  const button = $("update");
  button.hidden = !update;
  if (!update) return;
  const text = update.state === "downloading"
    ? `Downloading DigUp ${update.version}…` + (update.percent != null ? ` ${update.percent}%` : "")
    : update.state === "installing" ? `Installing DigUp ${update.version}…` : `DigUp ${update.version} is available`;
  button.innerHTML = '<svg viewBox="0 0 12 12"><circle cx="6" cy="6" r="6" fill="currentColor"/>'
    + '<path d="M6 3v5M3.8 6l2.2 2.2L8.2 6" fill="none" stroke="white" stroke-width="1.4" stroke-linecap="round" '
    + 'stroke-linejoin="round"/></svg>';
  button.append(text);
  button.disabled = update.state !== "available";
}

// MARK: Menus

/// A small menu at (x, y): [title, action, shortcut?, enabled?] items, null for a line between them.
function showMenu(x, y, items) {
  $menu.replaceChildren();
  for (const item of items) {
    if (!item) {
      $menu.append(element("hr"));
      continue;
    }
    const [title, action, shortcut, enabled = true] = item;
    const button = element("button", "", title);
    button.setAttribute("role", "menuitem");
    button.disabled = !enabled;
    if (shortcut) button.append(element("span", "shortcut", shortcut));
    button.onmousedown = (event) => event.preventDefault();
    button.onclick = () => {
      hideMenu();
      action();
    };
    $menu.append(button);
  }
  $menu.hidden = false;
  const { width, height } = $menu.getBoundingClientRect();
  $menu.style.left = `${Math.max(4, Math.min(x, innerWidth - width - 4))}px`;
  $menu.style.top = `${Math.max(4, Math.min(y, innerHeight - height - 4))}px`;
}

function hideMenu() {
  $menu.hidden = true;
}

/// ⋯: what the tray's menu has, for desktops without a tray.
function showMainMenu() {
  const button = $("more").getBoundingClientRect();
  const folders = state.info.folders.length > 0, paused = state.app.paused === true;
  showMenu(button.right - 200, button.bottom + 4, [
    ["Choose Folders…", () => invoke("choose_folders").catch(failed)],
    ["Index Now", () => invoke("index_now").catch(failed), null, folders && state.app.busy === false],
    [paused ? "Resume Indexing" : "Pause Indexing", () => invoke("pause_indexing", { paused: !paused }).catch(failed),
     null, folders],
    null,
    ["Settings…", () => invoke("open_settings").catch(failed), `${MOD},`],
    state.app.update
      ? [`Update to DigUp ${state.app.update.version}…`, () => invoke("offer_update").catch(failed), null,
         state.app.update.state === "available"]
      : ["Check for Updates…", () => invoke("check_for_updates").catch(failed)],
    null,
    ["Quit DigUp", () => invoke("quit_app").catch(failed)],
  ]);
}

document.addEventListener("mousedown", (event) => {
  if (!$menu.hidden && !$menu.contains(event.target)) hideMenu();
});
window.addEventListener("blur", hideMenu);

// MARK: Keys and events

$query.addEventListener("input", queryChanged);
// A click back in the field: ← → move the cursor again (after picking a result, they moved the selection).
$query.addEventListener("mousedown", () => { state.navigated = false; });
$("clear").onmousedown = (event) => event.preventDefault();
$("clear").onclick = () => setQuery("");
$("toggle-view").onmousedown = (event) => event.preventDefault();
$("toggle-view").onclick = () => {
  state.view = state.view === "grid" ? "table" : "grid";
  updateViewButton();
  renderContent();
};
$("toggle-preview").onmousedown = (event) => event.preventDefault();
$("toggle-preview").onclick = () => {
  state.showsPreview = !state.showsPreview;
  updatePreviewButton();
  renderSide();
};
$("settings").onmousedown = (event) => event.preventDefault();
$("settings").onclick = () => invoke("open_settings").catch(failed);
$("settings").title = `Settings (${MOD},)`;
$("more").onmousedown = (event) => event.preventDefault();
$("more").onclick = () => ($menu.hidden ? showMainMenu() : hideMenu());
$("status-line").onclick = () => invoke("choose_folders").catch(failed);
$("update").onclick = () => invoke("offer_update").catch(failed);
$kinds.onmousedown = (event) => event.preventDefault();
// The chosen files' bar: its buttons take the clicks but not the keyboard (typing stays in the field).
$batch.onmousedown = (event) => { if (event.target.closest("button")) event.preventDefault(); };
$("batch-all").onclick = selectAll;
$("batch-none").onclick = () => clearSelection();
$("batch-paths").onclick = copyPaths;
$("batch-copy").onclick = () => moveOrCopy("copy");
$("batch-move").onclick = () => moveOrCopy("move");
$("batch-delete").onclick = confirmDelete;
// A click beside a confirmation puts it away; an operation under way has its own Cancel.
$modal.onmousedown = (event) => { if (event.target === $modal && !state.busy) closeModal(); };

function updatePreviewButton() {
  const button = $("toggle-preview");
  button.setAttribute("aria-pressed", String(state.showsPreview));
  button.title = state.showsPreview ? "Hide Preview" : "Show Preview";
  button.setAttribute("aria-label", button.title);
}

function updateViewButton() {
  const button = $("toggle-view");
  const table = state.view === "table";
  button.setAttribute("aria-pressed", String(table));
  button.title = table ? "Show as Grid" : "Show as Table";
  button.setAttribute("aria-label", button.title);
}

document.addEventListener("keydown", (event) => {
  if (event.isComposing) return;   // an input method is composing: its keys
  const modifier = isMac ? event.metaKey : event.ctrlKey;
  const plain = !event.altKey && !event.shiftKey && !event.ctrlKey && !event.metaKey;
  if (!$modal.hidden) {
    // A sheet has the keyboard: its buttons answer Enter and Space; Tab goes round them; Escape puts it away (or asks
    // an operation to stop).
    if (event.key === "Tab") {
      const buttons = [...$modal.querySelectorAll("button:not(:disabled)")];
      event.preventDefault();
      if (!buttons.length) return;
      const at = buttons.indexOf(document.activeElement);
      const next = event.shiftKey ? (at <= 0 ? buttons.length - 1 : at - 1) : (at + 1) % buttons.length;
      buttons[next].focus();
    } else if (event.key === "Escape") {
      event.preventDefault();
      if (state.busy) cancelOperation();
      else closeModal();
    }
    return;
  }
  if (!$menu.hidden && event.key === "Escape") {
    event.preventDefault();
    hideMenu();
    return;
  }
  // A button with the keyboard answers Enter and Space itself (a checkbox's tick, a sheet's, the bar's).
  if (event.target instanceof Element && event.target.closest("button") && (event.key === "Enter" || event.key === " ")) {
    return;
  }
  // Select All: outside the field, where Ctrl+A is the text's (inside it, the field keeps its own).
  if (modifier && !event.altKey && event.key.toLowerCase() === "a") {
    if (!isWindows) return;
    // In the field Ctrl+A is the text's; Ctrl+Shift+A still clears what's chosen.
    if (document.activeElement === $query && !event.shiftKey) return;
    event.preventDefault();
    if (event.shiftKey) clearSelection();
    else selectAll();
    return;
  }
  if (modifier && !event.altKey && !event.shiftKey) {
    const number = Number(event.key);
    if (number >= 1 && number <= 6) {
      event.preventDefault();
      setFilter(number === 1 ? null : FILTERS[number - 2].key);
      return;
    }
    if (number === 7 && state.info.hasCode) {
      event.preventDefault();
      setFilter("code");
      return;
    }
    switch (event.key.toLowerCase()) {
      case ",":
        event.preventDefault();
        invoke("open_settings").catch(failed);
        return;
      case "w":
        event.preventDefault();
        invoke("close_window").catch(() => {});
        return;
      case "c":
        copyResult(event, state.hasResults && state.query.trim() ? selectedHit() : null, state.navigated);
        return;
      case "m":
        // The Media chip (a key of its own: Ctrl+1…7 are the kinds' and don't move).
        if (!isWindows) return;
        event.preventDefault();
        setFilter(state.filter === "media" ? null : "media");
        return;
      case "f":
      case "l":
        event.preventDefault();
        $query.focus();
        $query.select();
        state.navigated = false;
        return;
    }
  }
  switch (event.key) {
    case " ":
      // After the arrows or a click: the big preview. While typing: a space.
      if (!plain || !(state.navigated || !$big.hidden) || !state.hasResults || !state.query.trim() || !selectedHit()) return;
      event.preventDefault();
      toggleBig();
      return;
    case "ArrowDown":
    case "ArrowUp":
      // Shift takes the range from where choosing started to here (in the media list, a row at a time).
      if (event.shiftKey) {
        if (event.altKey || event.ctrlKey || event.metaKey || !isWindows || !rows().length) return;
        event.preventDefault();
        extendSelection(event.key === "ArrowDown" ? 1 : -1);
        return;
      }
      if (!plain || !rows().length) return;
      event.preventDefault();
      move(event.key === "ArrowDown" ? columns() : -columns());
      return;
    case "Insert": {
      // Add the one under the cursor to what's chosen (or take it away) and move on.
      const hit = state.hasResults && state.query.trim() ? selectedHit() : null;
      if (!hit || !isWindows || !MEDIA.has(hit.kind)) return;
      event.preventDefault();
      toggleSelected(hit.path);
      move(1);
      return;
    }
    case "Delete":
      // Only outside the field: there it deletes text.
      if (!isWindows || document.activeElement === $query || !selectedHits().length) return;
      event.preventDefault();
      confirmDelete();
      return;
    case "F5":
    case "F6":
    case "F8":
      // Taken even with nothing chosen: the web view must not reload the window on them.
      if (!isWindows) return;
      event.preventDefault();
      if (!selectedHits().length) return;
      if (event.key === "F5") moveOrCopy("copy");
      else if (event.key === "F6") moveOrCopy("move");
      else confirmDelete();
      return;
    case "ArrowLeft":
    case "ArrowRight":
      if (!plain || !state.navigated || !rows().length) return;
      event.preventDefault();
      move(event.key === "ArrowRight" ? 1 : -1);
      return;
    case "Enter": {
      const hit = state.hasResults && state.query.trim() ? selectedHit() : null;
      if (!hit) return;
      if (modifier && !event.altKey && !event.shiftKey) reveal(hit);
      else if (plain) open(hit);
      else return;
      closeBig();
      event.preventDefault();
      return;
    }
    case "Escape":
      if (state.selected.size) {
        event.preventDefault();
        clearSelection();
        return;
      }
      if (!$big.hidden) {
        event.preventDefault();
        closeBig();
        return;
      }
      if (!$query.value) return;
      event.preventDefault();
      setQuery("");
      return;
  }
  // Typing anywhere goes to the field.
  if (event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey
      && document.activeElement !== $query && !window.getSelection()?.toString()) {
    $query.focus();
  }
});

tauri.event.listen("state", ({ payload }) => {
  const before = state.app;
  state.app = payload;
  // Code search turned on or off (Settings): the Code chip, the examples, a code: search.
  if (payload.hasCode != null && payload.hasCode !== state.info.hasCode) {
    state.info.hasCode = payload.hasCode;
    if (isCode() && state.query.trim()) run(); else render();
  }
  renderStatus();
  if (payload.searchable !== before.searchable || payload.files !== before.files) loadIndexed();
});

// The panel handed its search over (Ctrl+O, "Show all…"): this window was already open.
tauri.event.listen("window-search", ({ payload }) => {
  state.filter = payload.kind || null;
  setQuery(payload.query || "");
  $query.focus();
});

// The files being moved, copied or deleted: the sheet's line moves along. The report comes back from the command
// itself, so `done` here only closes the gap before it.
tauri.event.listen("file-op", ({ payload }) => {
  if (!state.busy || payload.id !== state.busy.id) return;
  if (payload.phase === "progress") {
    state.busy.done = payload.done;
    state.busy.path = payload.path;
    // A copy's byte-by-byte events carry bytes/size; the one after each file doesn't.
    state.busy.bytes = payload.bytes ?? null;
    state.busy.size = payload.size ?? null;
    renderOperation();
  }
});

window.addEventListener("focus", () => ask({ op: "prewarm" }).catch(() => {}));

// Its size, for the next time it opens (the shell keeps it when the window closes).
let sizeTimer = null;
function reportSize() {
  invoke("window_resized", { width: window.innerWidth, height: window.innerHeight }).catch(() => {});
}
window.addEventListener("resize", () => {
  clearTimeout(sizeTimer);
  sizeTimer = setTimeout(reportSize, 300);
});
reportSize();

/// What's indexed of each kind under the chosen folders (the home's counts), at most every two seconds while indexing.
let indexedTimer = null;
function loadIndexed() {
  if (indexedTimer) return;
  indexedTimer = setTimeout(async () => {
    indexedTimer = null;
    try {
      const reply = await ask({ op: "summaries" });
      const kinds = {};
      for (const root of reply.roots || []) {
        for (const [kind, count] of Object.entries(root.kinds || {})) kinds[kind] = (kinds[kind] || 0) + count;
      }
      state.indexed = kinds;
      if (!state.query.trim() && state.info.folders.length) {
        $content.querySelector(".about")?.replaceWith(about());
      }
    } catch {
      // The home goes without the counts.
    }
  }, state.indexed ? 2000 : 0);
}

async function start() {
  const info = await invoke("window_info");
  state.info = { ...state.info, ...info };
  state.app = info.state || {};
  if (info.search) {
    state.filter = info.search.kind || null;
    $query.value = info.search.query || "";
    state.query = $query.value;
    $("clear").hidden = !state.query;
  }
  updatePreviewButton();
  render();
  loadIndexed();
  $query.focus();
  if (state.query) run();
  ask({ op: "prewarm" }).catch(() => {});
  if (info.debugQuery != null) debugSearch(info.debugQuery);
}

// Debug runs (--debug-window): type the query in, and once the results and their pictures are in, report what the
// window shows ("" reports the home).
function debugSearch(query) {
  if (!query) {
    setTimeout(report, 1500);
    return;
  }
  debugPending = true;
  setQuery(query);
}

function report() {
  debugPending = false;
  // With --debug-settings, Settings opens as its button opens it, and Settings reports once its page is up.
  if (state.info.debugSettings) {
    setTimeout(() => $("settings").click(), 500);
    return;
  }
  // --debug-copy: the selected result on the clipboard, as Ctrl+C puts it.
  if (state.info.debugCopy && selectedHit()) {
    invoke("copy_file", { path: selectedHit().path })
      .then(() => "copied", (error) => `not copied: ${error}`)
      .then((copied) => invoke("debug_report", { report: { copied, path: selectedHit()?.path } }));
  }
  // Dev snapshots (window.html?big): the selected result's big preview, as Space shows it.
  if (new URLSearchParams(location.search).has("big")) toggleBig();
  setTimeout(() => {
    const thumbs = [...$content.querySelectorAll(".cell img")];
    const big = $side.querySelector(".frame img, .passage, .code-lines, .audio-moment, .placeholder");
    invoke("debug_report", {
      report: {
        window: true,
        query: $query.value,
        filter: state.filter,
        kinds: [...$kinds.querySelectorAll(".chip")].map((chip) => chip.textContent + (chip.classList.contains("selected") ? " (selected)" : "")),
        results: state.ranked.map((hit) => nameOf(hit.path)),
        weaker: state.weaker.length,
        columns: columns(),
        selected: state.selection && nameOf(state.selection),
        thumbnails: `${thumbs.filter((image) => image.complete && image.naturalWidth > 0).length} of ${thumbs.length} drawn`,
        preview: big && {
          what: big.className || big.tagName.toLowerCase(),
          loaded: big.tagName === "IMG" ? big.complete && big.naturalWidth > 0 : null,
          size: big.tagName === "IMG" ? `${big.naturalWidth}x${big.naturalHeight}` : null,
          text: big.tagName === "IMG" ? null : big.textContent.slice(0, 80),
        },
        facts: [...$side.querySelectorAll(".facts .title, .facts .folder, .facts .details, .why .reason span")].map((node) => node.textContent),
        home: $content.querySelector(".home") && [...$content.querySelectorAll(".about .line")].map((line) => line.textContent),
        status: [$("status-line").textContent, $("count").textContent],
      },
    }).catch(() => {});
  }, 1500);
}

start();
