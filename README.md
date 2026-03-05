# LANBox

LANBox is a lightweight LAN file web server written in **Rust**.  
It provides a simple browser-based interface for transferring, managing, and sharing files between devices inside a local network.

The project keeps a minimal design: a small Rust server, a simple API, and a static frontend.

---

## Overview

LANBox lets you start a file server with a single command and access it from any device on the same network.

Typical use cases:

- transferring files between PC and phone
- sharing files inside a home or office network
- temporary local file hosting
- testing HTTP/file workflows
- a small reference implementation of a file web server

No accounts or cloud services are required.

---

## Key Features

- Web-based file manager
- File upload with progress tracking
- Folder navigation
- Create directories
- Rename files and folders
- Delete files and directories
- Direct file download
- Image preview support (frontend dependent)
- Works across devices in the same LAN

---

## Requirements

- Rust toolchain (stable)
- Cargo
- Modern web browser

---

## Project Layout

Expected folders (defaults):

```
lanbox/
├─ src/
│  ├─ main.rs
│  ├─ config.rs
│  ├─ handlers.rs
│  └─ util.rs
├─ public/        # frontend
├─ uploads/       # uploaded files
├─ Cargo.toml
└─ README.md
```

- `public/` — static frontend files (index.html, style.css, app.js)
- `uploads/` — uploaded/shared files

---

## Installation

Clone the repository:

```bash
git clone https://github.com/thespark1ch/lanbox.git
cd lanbox
```

Build:

```bash
cargo build --release
```

---

## Running the Server

### Quick start

```
target/release/lanbox
```

Defaults:

- `ROOT=./public`
- `UPLOAD_DIR=./uploads`
- `HOST=0.0.0.0`
- `PORT=8080`

### Run on LAN (accessible from other devices)

```
HOST=0.0.0.0 PORT=8080 ROOT=./public UPLOAD_DIR=./uploads target/release/lanbox
```

After launch the server prints:

```
LAN:  http://<your_lan_ip>:8080/
Bind: http://0.0.0.0:8080/
```

Open in browser:

```
http://localhost:8080
```

From another device in the same network:

```
http://YOUR_LAN_IP:8080
```

---

## Configuration

Environment variables:

- `ROOT` — frontend directory (default `./public`)
- `UPLOAD_DIR` — uploads directory (default `./uploads`)
- `HOST` — bind address (default `0.0.0.0`)
- `PORT` — port number (default `8080`)

Example:

```
ROOT=./public UPLOAD_DIR=./uploads HOST=0.0.0.0 PORT=8080 target/release/lanbox
```

---

## API

List directory contents

```
GET /api/list?dir=PATH
```

Upload file

```
POST /api/upload?dir=PATH&name=FILENAME
Content-Type: application/octet-stream
```

Create folder

```
POST /api/mkdir
Body: {"dir":"/","name":"New Folder"}
```

Rename

```
POST /api/rename
Body: {"path":"/old.txt","name":"new.txt"}
```

Delete

```
DELETE /api/delete?path=TARGET
```

These endpoints are used by the frontend but can also be used directly.

---

## Security Notes

LANBox is designed for **trusted local networks**.

Basic protections implemented:

- path normalization
- prevention of directory traversal
- filename sanitization
- safe file downloads

Running LANBox directly on the public internet is **not recommended** without authentication or a reverse proxy.

---

## License

MIT
