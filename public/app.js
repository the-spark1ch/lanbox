const $ = (id) => document.getElementById(id);

let cwd = "/";

function setProgress(pct, text) {
  const v = Math.max(0, Math.min(100, Number.isFinite(pct) ? pct : 0));
  $("bar").style.width = v.toFixed(1) + "%";
  $("progtext").textContent = text || "";
}

function formatBytes(n) {
  if (!Number.isFinite(n)) return "";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(i === 0 ? 0 : 1)} ${u[i]}`;
}

function formatTime(ms) {
  return new Date(ms).toLocaleString();
}

async function apiList(dir) {
  const r = await fetch("/api/list?dir=" + encodeURIComponent(dir), { cache: "no-store" });
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || "List error");
  return j;
}

async function apiMkdir(dir, name) {
  const r = await fetch("/api/mkdir", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ dir, name }),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || "Mkdir error");
  return j;
}

async function apiDelete(pathStr) {
  const r = await fetch("/api/delete?path=" + encodeURIComponent(pathStr), { method: "DELETE" });
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || "Delete error");
  return j;
}

async function apiRename(pathStr, name) {
  const r = await fetch("/api/rename", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ path: pathStr, name }),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || "Rename error");
  return j;
}

function safeFilesArray(listLike) {
  const arr = Array.from(listLike || []);
  return arr.filter((f) => f && typeof f.name === "string" && f.name.length);
}

function filesLabel(files) {
  const n = files.length;
  if (!n) return "Файлы не выбраны";
  if (n === 1) return files[0].name || "1 файл";
  return `Выбрано файлов: ${n}`;
}

function uploadOneXHR(file, dir, uploadedBefore, totalBytes) {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    const u = "/api/upload?dir=" + encodeURIComponent(dir) + "&name=" + encodeURIComponent(file.name);
    xhr.open("POST", u, true);
    xhr.setRequestHeader("Content-Type", "application/octet-stream");

    xhr.upload.onprogress = (e) => {
      if (!e.lengthComputable) {
        setProgress(30, "Загрузка...");
        return;
      }
      const overallLoaded = uploadedBefore + e.loaded;
      const pct = totalBytes > 0 ? (overallLoaded / totalBytes) * 100 : 0;
      setProgress(pct, `Загрузка: ${pct.toFixed(0)}% (${formatBytes(overallLoaded)} / ${formatBytes(totalBytes)})`);
    };

    xhr.onerror = () => reject(new Error("Network error"));
    xhr.onabort = () => reject(new Error("Aborted"));

    xhr.onload = () => {
      let j = null;
      try {
        j = JSON.parse(xhr.responseText || "{}");
      } catch {}
      if (xhr.status >= 200 && xhr.status < 300) return resolve(j);
      const msg = j && j.error ? j.error : `Upload error (HTTP ${xhr.status})`;
      reject(new Error(msg));
    };

    xhr.send(file);
  });
}

function setCwd(dir) {
  cwd = dir || "/";
  $("cwd").textContent = cwd;
}

function isImageName(name) {
  const n = (name || "").toLowerCase();
  return /\.(png|jpe?g|gif|webp|bmp|svg)$/.test(n);
}

function fileExt(name) {
  const s = (name || "").trim();
  const base = s.split("/").pop() || s;
  const i = base.lastIndexOf(".");
  if (i <= 0 || i === base.length - 1) return "";
  return base.slice(i + 1).toUpperCase();
}

function makeThumb(it) {
  const wrap = document.createElement("div");
  wrap.className = "thumb";

  if (it.type === "dir") {
    const t = document.createElement("div");
    t.className = "thumb-text";
    t.textContent = "📁";
    wrap.appendChild(t);
    return wrap;
  }

  if (it.type === "file" && isImageName(it.name)) {
    const img = document.createElement("img");
    img.className = "thumb-img";
    img.loading = "lazy";
    img.alt = it.name || "";
    img.src = it.url;
    wrap.appendChild(img);
    return wrap;
  }

  const ext = fileExt(it.name) || "FILE";
  const t = document.createElement("div");
  t.className = "thumb-text";
  t.textContent = ext;
  wrap.appendChild(t);
  return wrap;
}

function makeInfoBlock(it) {
  const info = document.createElement("div");
  info.className = "file-info";

  const name = document.createElement("div");
  name.className = "file-name";

  const pill = document.createElement("span");
  pill.className = "pill";
  pill.textContent = it.type;

  if (it.type === "dir") {
    const a = document.createElement("a");
    a.className = "file-link";
    a.href = "#";
    a.textContent = it.name;
    a.onclick = async (e) => {
      e.preventDefault();
      await navigate(it.path);
    };
    name.appendChild(a);
    name.appendChild(pill);
  } else {
    const a = document.createElement("a");
    a.className = "file-link";
    a.href = it.url;
    a.target = "_blank";
    a.rel = "noreferrer";
    a.textContent = it.name;
    name.appendChild(a);
  }

  const meta = document.createElement("div");
  meta.className = "file-meta";
  meta.textContent = it.type === "dir" ? `${formatTime(it.mtimeMs)}` : `${formatBytes(it.size)} • ${formatTime(it.mtimeMs)}`;

  info.appendChild(name);
  info.appendChild(meta);

  return info;
}

async function doRename(it, btn) {
  const currentName = it && it.name ? String(it.name) : "";
  const newName = (prompt("Новое имя:", currentName) || "").trim();

  if (!newName || newName === currentName) return;

  if (btn) btn.disabled = true;
  $("uploadBtn").disabled = true;
  $("mkdirBtn").disabled = true;

  try {
    setProgress(0, "");
    await apiRename(it.path, newName);
    await refresh();
  } catch (e) {
    setProgress(0, e.message);
  } finally {
    if (btn) btn.disabled = false;
    $("uploadBtn").disabled = false;
    $("mkdirBtn").disabled = false;
  }
}

function makeActions(it) {
  const actions = document.createElement("div");
  actions.className = "actions";

  const openBtn = document.createElement(it.type === "dir" ? "button" : "a");
  openBtn.className = "btn secondary";
  openBtn.textContent = it.type === "dir" ? "Открыть" : "Скачать";

  if (it.type === "dir") {
    openBtn.onclick = async () => navigate(it.path);
  } else {
    openBtn.href = it.url;
    openBtn.download = it.name || "";
  }

  const renBtn = document.createElement("button");
  renBtn.className = "btn";
  renBtn.textContent = "Переименовать";
  renBtn.onclick = async () => {
    await doRename(it, renBtn);
  };

  const delBtn = document.createElement("button");
  delBtn.className = "btn danger";
  delBtn.textContent = "Удалить";
  delBtn.onclick = async () => {
    delBtn.disabled = true;
    try {
      setProgress(0, "");
      await apiDelete(it.path);
      await refresh();
    } catch (e) {
      setProgress(0, e.message);
    } finally {
      delBtn.disabled = false;
    }
  };

  actions.appendChild(openBtn);
  actions.appendChild(renBtn);
  actions.appendChild(delBtn);

  return actions;
}

function render(listResp) {
  setCwd(listResp.dir);

  const list = $("list");
  list.innerHTML = "";

  if (listResp.parent) {
    const item = document.createElement("div");
    item.className = "file-item";

    const left = document.createElement("div");
    left.className = "file-left";

    const thumb = document.createElement("div");
    thumb.className = "thumb";
    const t = document.createElement("div");
    t.className = "thumb-text";
    t.textContent = "📁";
    thumb.appendChild(t);
    left.appendChild(thumb);

    const info = document.createElement("div");
    info.className = "file-info";

    const name = document.createElement("div");
    name.className = "file-name";
    name.innerHTML = `<a class="file-link" href="#">..</a><span class="pill">dir</span>`;

    const meta = document.createElement("div");
    meta.className = "file-meta";
    meta.textContent = "Назад";

    info.appendChild(name);
    info.appendChild(meta);
    left.appendChild(info);

    const actions = document.createElement("div");
    actions.className = "actions";

    const openBtn = document.createElement("button");
    openBtn.className = "btn secondary";
    openBtn.textContent = "Открыть";
    openBtn.onclick = async () => {
      await navigate(listResp.parent);
    };

    actions.appendChild(openBtn);

    item.appendChild(left);
    item.appendChild(actions);
    list.appendChild(item);
  }

  if (!listResp.items.length) {
    const empty = document.createElement("div");
    empty.className = "progtext";
    empty.textContent = "Пусто";
    list.appendChild(empty);
    return;
  }

  for (const it of listResp.items) {
    const item = document.createElement("div");
    item.className = "file-item";

    const left = document.createElement("div");
    left.className = "file-left";

    left.appendChild(makeThumb(it));
    left.appendChild(makeInfoBlock(it));

    item.appendChild(left);
    item.appendChild(makeActions(it));

    list.appendChild(item);
  }
}

async function refresh() {
  const data = await apiList(cwd);
  render(data);
}

async function navigate(dir) {
  const data = await apiList(dir);
  render(data);
  history.replaceState(null, "", "#dir=" + encodeURIComponent(cwd));
}

function readHashDir() {
  const h = location.hash || "";
  const m = /#dir=([^&]+)/.exec(h);
  if (!m) return "/";
  try {
    return decodeURIComponent(m[1]);
  } catch {
    return "/";
  }
}

async function uploadMany(files) {
  const list = safeFilesArray(files);
  if (!list.length) {
    setProgress(0, "Выберите файлы");
    return;
  }

  $("uploadBtn").disabled = true;
  $("mkdirBtn").disabled = true;

  const totalBytes = list.reduce((s, f) => s + (Number.isFinite(f.size) ? f.size : 0), 0);
  let uploadedBefore = 0;

  try {
    for (let i = 0; i < list.length; i++) {
      const f = list[i];
      setProgress(totalBytes ? (uploadedBefore / totalBytes) * 100 : 0, `Файл ${i + 1}/${list.length}: ${f.name}`);
      await uploadOneXHR(f, cwd, uploadedBefore, totalBytes);
      uploadedBefore += f.size || 0;
    }
    setProgress(100, "Готово");
    $("file").value = "";
    $("filename").textContent = "Файлы не выбраны";
    setTimeout(() => setProgress(0, ""), 700);
    await refresh();
  } catch (e) {
    setProgress(0, e.message);
  } finally {
    $("uploadBtn").disabled = false;
    $("mkdirBtn").disabled = false;
  }
}

$("file").addEventListener("change", () => {
  const fl = safeFilesArray($("file").files);
  $("filename").textContent = filesLabel(fl);
});

$("uploadBtn").onclick = async () => {
  await uploadMany($("file").files);
};

$("mkdirBtn").onclick = async () => {
  const name = ($("folderName").value || "").trim();
  if (!name) {
    setProgress(0, "Введите имя папки");
    return;
  }
  $("uploadBtn").disabled = true;
  $("mkdirBtn").disabled = true;
  try {
    setProgress(0, "");
    await apiMkdir(cwd, name);
    $("folderName").value = "";
    await refresh();
  } catch (e) {
    setProgress(0, e.message);
  } finally {
    $("uploadBtn").disabled = false;
    $("mkdirBtn").disabled = false;
  }
};

document.addEventListener("dragover", (e) => {
  e.preventDefault();
});

document.addEventListener("drop", async (e) => {
  e.preventDefault();
  if ($("uploadBtn").disabled) return;
  const dt = e.dataTransfer;
  if (!dt || !dt.files) return;
  await uploadMany(dt.files);
});

window.addEventListener("hashchange", async () => {
  const d = readHashDir();
  try {
    await navigate(d);
  } catch {
    await navigate("/");
  }
});

(async () => {
  const d = readHashDir();
  try {
    await navigate(d);
  } catch {
    await navigate("/");
  }
})();
