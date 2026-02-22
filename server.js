"use strict";

const http = require("http");
const fs = require("fs");
const fsp = fs.promises;
const path = require("path");
const url = require("url");
const crypto = require("crypto");
const os = require("os");

const ROOT = path.resolve(process.env.ROOT || path.join(process.cwd(), "public"));
const UPLOAD_DIR = path.resolve(process.env.UPLOAD_DIR || path.join(process.cwd(), "uploads"));
const PORT = Number(process.env.PORT || 8080);

function getLanIPv4() {
  const nets = os.networkInterfaces();
  for (const name of Object.keys(nets)) {
    for (const net of nets[name] || []) {
      if (net && net.family === "IPv4" && !net.internal) return net.address;
    }
  }
  return null;
}

const LAN_IP = getLanIPv4() || "127.0.0.1";
const HOST = process.env.HOST || "0.0.0.0";

const MIME = new Map([
  [".html", "text/html; charset=utf-8"],
  [".htm", "text/html; charset=utf-8"],
  [".css", "text/css; charset=utf-8"],
  [".js", "application/javascript; charset=utf-8"],
  [".mjs", "application/javascript; charset=utf-8"],
  [".json", "application/json; charset=utf-8"],
  [".txt", "text/plain; charset=utf-8"],
  [".png", "image/png"],
  [".jpg", "image/jpeg"],
  [".jpeg", "image/jpeg"],
  [".gif", "image/gif"],
  [".webp", "image/webp"],
  [".svg", "image/svg+xml"],
  [".ico", "image/x-icon"],
  [".pdf", "application/pdf"],
  [".zip", "application/zip"],
  [".mp3", "audio/mpeg"],
  [".mp4", "video/mp4"],
  [".webm", "video/webm"],
  [".woff", "font/woff"],
  [".woff2", "font/woff2"],
  [".ttf", "font/ttf"],
]);

function mimeTypeFor(p) {
  return MIME.get(path.extname(p).toLowerCase()) || "application/octet-stream";
}

function send(res, code, headers, body) {
  res.writeHead(code, headers);
  if (body && res.req.method !== "HEAD") res.end(body);
  else res.end();
}

function json(res, code, obj) {
  const b = Buffer.from(JSON.stringify(obj));
  send(res, code, { "Content-Type": "application/json; charset=utf-8", "Content-Length": b.length }, b);
}

function bad(res, code, msg) {
  json(res, code, { error: msg || http.STATUS_CODES[code] || "Error" });
}

function decodePathname(reqUrl) {
  const parsed = url.parse(reqUrl);
  let p = parsed.pathname || "/";
  try {
    p = decodeURIComponent(p);
  } catch {
    return null;
  }
  return p;
}

function getQuery(reqUrl) {
  const parsed = url.parse(reqUrl, true);
  return parsed.query || {};
}

function toSafeAbsolutePath(root, pathname) {
  if (!pathname.startsWith("/")) pathname = "/" + pathname;
  if (pathname.includes("\0")) return null;
  const rel = pathname.replace(/^\/+/, "");
  const normalized = path.normalize(rel);
  const abs = path.resolve(root, normalized);
  if (!abs.startsWith(root + path.sep) && abs !== root) return null;
  return abs;
}

function safeSegment(name) {
  const s = String(name || "").trim();
  if (!s) return null;
  if (s === "." || s === "..") return null;
  if (s.includes("/") || s.includes("\\") || s.includes("\0")) return null;
  const cleaned = s.replace(/[^\w.\-()@\s]/g, "_").trim();
  if (!cleaned) return null;
  return cleaned.slice(0, 180);
}

function normalizeDirParam(dir) {
  let d = typeof dir === "string" ? dir : "";
  try {
    d = decodeURIComponent(d);
  } catch {
    return null;
  }
  d = d.replace(/\\/g, "/").trim();
  if (!d || d === "/") return "/";
  if (!d.startsWith("/")) d = "/" + d;
  d = d.replace(/\/+/g, "/");
  d = d.replace(/\/+$/, "");
  if (d === "") d = "/";
  if (d.includes("\0")) return null;
  return d;
}

function joinPosix(a, b) {
  if (a.endsWith("/")) a = a.slice(0, -1);
  if (!b.startsWith("/")) b = "/" + b;
  const out = (a + b).replace(/\/+/g, "/");
  return out === "" ? "/" : out;
}

function safeFinalFileName(original) {
  const base = path.basename(String(original || "file"));
  const cleaned = base.replace(/[^\w.\-()@\s]/g, "_").trim().slice(0, 180) || "file";
  const ext = path.extname(cleaned).slice(0, 16);
  const stem = path.basename(cleaned, path.extname(cleaned)).slice(0, 120) || "file";
  const id = crypto.randomBytes(6).toString("hex");
  return `${stem}-${id}${ext || ""}`;
}

async function ensureDirs() {
  await fsp.mkdir(ROOT, { recursive: true }).catch(() => {});
  await fsp.mkdir(UPLOAD_DIR, { recursive: true }).catch(() => {});
}

function readBody(req, limitBytes) {
  return new Promise((resolve, reject) => {
    let total = 0;
    const chunks = [];
    req.on("data", (c) => {
      total += c.length;
      if (total > limitBytes) {
        reject(new Error("Payload too large"));
        req.destroy();
        return;
      }
      chunks.push(c);
    });
    req.on("end", () => resolve(Buffer.concat(chunks)));
    req.on("error", reject);
  });
}

async function listDir(absDir, relDir) {
  const entries = await fsp.readdir(absDir, { withFileTypes: true });
  const out = [];
  for (const e of entries) {
    if (e.name.startsWith(".")) continue;
    const abs = path.join(absDir, e.name);
    const st = await fsp.stat(abs).catch(() => null);
    if (!st) continue;
    const isDir = e.isDirectory();
    const relPath = joinPosix(relDir, e.name);
    out.push({
      name: e.name,
      type: isDir ? "dir" : "file",
      size: isDir ? 0 : st.size,
      mtimeMs: st.mtimeMs,
      path: relPath,
      url: isDir ? null : "/uploads" + relPath.split("/").map(encodeURIComponent).join("/"),
    });
  }
  out.sort((a, b) => {
    if (a.type !== b.type) return a.type === "dir" ? -1 : 1;
    return a.name.localeCompare(b.name, "ru");
  });
  return out;
}

async function serveStatic(req, res, absFile, forceDownload) {
  let st;
  try {
    st = await fsp.stat(absFile);
  } catch {
    bad(res, 404, "Not found");
    return;
  }

  if (st.isDirectory()) {
    const index = path.join(absFile, "index.html");
    try {
      const ist = await fsp.stat(index);
      if (ist.isFile()) return serveStatic(req, res, index, false);
    } catch {}
    bad(res, 403, "Directory");
    return;
  }

  if (!st.isFile()) {
    bad(res, 403, "Forbidden");
    return;
  }

  res.setHeader("Content-Type", mimeTypeFor(absFile));
  res.setHeader("Content-Length", st.size);

  if (forceDownload) {
    const fname = path.basename(absFile).replace(/[\r\n"]/g, "_");
    res.setHeader("Content-Disposition", `attachment; filename="${fname}"`);
  }

  res.statusCode = 200;

  if (req.method === "HEAD") {
    res.end();
    return;
  }

  const stream = fs.createReadStream(absFile);
  stream.on("error", () => {
    if (!res.headersSent) bad(res, 500, "Read error");
    else res.destroy();
  });
  stream.pipe(res);
}

async function handleUploadStream(req, res) {
  const q = getQuery(req.url || "");
  const dir = normalizeDirParam(q.dir);
  if (!dir) return bad(res, 400, "Bad dir");

  const nameParam = typeof q.name === "string" ? q.name : "";
  let decodedName = "";
  try {
    decodedName = decodeURIComponent(nameParam);
  } catch {
    decodedName = nameParam;
  }

  const absDir = toSafeAbsolutePath(UPLOAD_DIR, dir);
  if (!absDir) return bad(res, 403, "Forbidden");
  const st = await fsp.stat(absDir).catch(() => null);
  if (!st || !st.isDirectory()) return bad(res, 404, "Not found");

  const original = safeSegment(path.basename(decodedName || "file")) || "file";
  const finalName = safeFinalFileName(original);

  const outPath = path.join(absDir, finalName);
  if (!outPath.startsWith(absDir + path.sep)) return bad(res, 403, "Forbidden");

  let ws;
  try {
    ws = fs.createWriteStream(outPath, { flags: "wx" });
  } catch {
    return bad(res, 500, "Cannot create file");
  }

  let finished = false;

  const cleanup = async () => {
    if (finished) return;
    finished = true;
    try {
      ws.destroy();
    } catch {}
    try {
      await fsp.unlink(outPath);
    } catch {}
  };

  req.on("aborted", cleanup);
  req.on("error", cleanup);
  ws.on("error", cleanup);

  ws.on("close", async () => {
    if (finished) return;
    finished = true;
    const relPath = joinPosix(dir, finalName);
    return json(res, 201, {
      files: [
        {
          name: finalName,
          path: relPath,
          url: "/uploads" + relPath.split("/").map(encodeURIComponent).join("/"),
        },
      ],
    });
  });

  req.pipe(ws);
}

async function handleApi(req, res, pathname) {
  if (req.method === "GET" && pathname === "/api/list") {
    const q = getQuery(req.url || "");
    const dir = normalizeDirParam(q.dir);
    if (!dir) return bad(res, 400, "Bad dir");
    const absDir = toSafeAbsolutePath(UPLOAD_DIR, dir);
    if (!absDir) return bad(res, 403, "Forbidden");
    const st = await fsp.stat(absDir).catch(() => null);
    if (!st) return bad(res, 404, "Not found");
    if (!st.isDirectory()) return bad(res, 400, "Not a directory");
    const items = await listDir(absDir, dir).catch(() => null);
    if (!items) return bad(res, 500, "Cannot list");
    const parent = dir === "/" ? null : dir.split("/").slice(0, -1).join("/") || "/";
    return json(res, 200, { dir, parent, items });
  }

  if (req.method === "POST" && pathname === "/api/mkdir") {
    const body = await readBody(req, 64 * 1024).catch(() => null);
    if (!body) return bad(res, 400, "Bad body");
    let j;
    try {
      j = JSON.parse(body.toString("utf8"));
    } catch {
      return bad(res, 400, "Bad json");
    }
    const dir = normalizeDirParam(j.dir);
    if (!dir) return bad(res, 400, "Bad dir");
    const name = safeSegment(j.name);
    if (!name) return bad(res, 400, "Bad name");

    const absDir = toSafeAbsolutePath(UPLOAD_DIR, dir);
    if (!absDir) return bad(res, 403, "Forbidden");
    const target = path.join(absDir, name);
    if (!target.startsWith(absDir + path.sep) && target !== absDir) return bad(res, 403, "Forbidden");

    try {
      await fsp.mkdir(target, { recursive: false });
    } catch (e) {
      if (e && e.code === "EEXIST") return bad(res, 409, "Already exists");
      return bad(res, 500, "Cannot create");
    }
    return json(res, 201, { ok: true });
  }

  if (req.method === "POST" && pathname === "/api/rename") {
    const body = await readBody(req, 64 * 1024).catch(() => null);
    if (!body) return bad(res, 400, "Bad body");
    let j;
    try {
      j = JSON.parse(body.toString("utf8"));
    } catch {
      return bad(res, 400, "Bad json");
    }

    const p = normalizeDirParam(j.path);
    if (!p) return bad(res, 400, "Bad path");
    if (p === "/") return bad(res, 400, "Cannot rename root");

    const newName = safeSegment(j.name);
    if (!newName) return bad(res, 400, "Bad name");

    const absOld = toSafeAbsolutePath(UPLOAD_DIR, p);
    if (!absOld) return bad(res, 403, "Forbidden");

    const st = await fsp.stat(absOld).catch(() => null);
    if (!st) return bad(res, 404, "Not found");

    const parentRel = p.split("/").slice(0, -1).join("/") || "/";
    const absParent = toSafeAbsolutePath(UPLOAD_DIR, parentRel);
    if (!absParent) return bad(res, 403, "Forbidden");

    const absNew = path.join(absParent, newName);
    if (!absNew.startsWith(absParent + path.sep)) return bad(res, 403, "Forbidden");

    const exists = await fsp.stat(absNew).catch(() => null);
    if (exists) return bad(res, 409, "Already exists");

    try {
      await fsp.rename(absOld, absNew);
    } catch {
      return bad(res, 500, "Cannot rename");
    }

    const newRelPath = joinPosix(parentRel, newName);
    const isDir = st.isDirectory();
    return json(res, 200, {
      ok: true,
      from: p,
      to: newRelPath,
      item: {
        name: newName,
        type: isDir ? "dir" : "file",
        path: newRelPath,
        url: isDir ? null : "/uploads" + newRelPath.split("/").map(encodeURIComponent).join("/"),
      },
    });
  }

  if (req.method === "DELETE" && pathname === "/api/delete") {
    const q = getQuery(req.url || "");
    const p = normalizeDirParam(q.path);
    if (!p) return bad(res, 400, "Bad path");
    if (p === "/") return bad(res, 400, "Cannot delete root");

    const abs = toSafeAbsolutePath(UPLOAD_DIR, p);
    if (!abs) return bad(res, 403, "Forbidden");
    const st = await fsp.stat(abs).catch(() => null);
    if (!st) return bad(res, 404, "Not found");

    try {
      await fsp.rm(abs, { recursive: true, force: true });
    } catch {
      return bad(res, 500, "Cannot delete");
    }
    return json(res, 200, { ok: true });
  }

  if ((req.method === "POST" || req.method === "PUT") && pathname === "/api/upload") {
    const ct = String(req.headers["content-type"] || "").toLowerCase();
    if (ct.startsWith("application/octet-stream") || req.method === "PUT") {
      return handleUploadStream(req, res);
    }
    return bad(res, 415, "Use application/octet-stream");
  }

  bad(res, 404, "Unknown API");
}

async function handler(req, res) {
  const pathname = decodePathname(req.url || "/");
  if (pathname == null) return bad(res, 400, "Bad URL encoding");

  if (pathname.startsWith("/api/")) {
    if (!["GET", "POST", "PUT", "DELETE", "HEAD"].includes(req.method || "GET")) return bad(res, 405, "Method Not Allowed");
    return handleApi(req, res, pathname);
  }

  if (pathname.startsWith("/uploads/") || pathname === "/uploads") {
    const sub = pathname === "/uploads" ? "/" : pathname.slice("/uploads".length);
    const abs = toSafeAbsolutePath(UPLOAD_DIR, sub);
    if (!abs) return bad(res, 403, "Forbidden");
    if (!["GET", "HEAD"].includes(req.method || "GET")) return bad(res, 405, "Method Not Allowed");
    return serveStatic(req, res, abs, true);
  }

  const abs = toSafeAbsolutePath(ROOT, pathname);
  if (!abs) return bad(res, 403, "Forbidden");
  if (!["GET", "HEAD"].includes(req.method || "GET")) return bad(res, 405, "Method Not Allowed");
  return serveStatic(req, res, abs, false);
}

(async () => {
  await ensureDirs();

  const server = http.createServer((req, res) => {
    res.req = req;
    Promise.resolve(handler(req, res)).catch(() => {
      if (!res.headersSent) bad(res, 500, "Internal error");
      else res.destroy();
    });
  });

  server.listen(PORT, HOST, () => {
    console.log(`LAN:  http://${LAN_IP}:${PORT}/`);
    console.log(`Bind: http://${HOST}:${PORT}/`);
    console.log(`public: ${ROOT}`);
    console.log(`uploads: ${UPLOAD_DIR}`);
  });
})();
