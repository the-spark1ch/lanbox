# LANBox (Rust)

LANBox is a lightweight LAN file web server written in **Rust**.  
It provides a simple browser-based interface for transferring, managing, and sharing files between devices inside a local network.

The project keeps the same minimal, transparent approach as the original version: a small server, a small API, and a static frontend.

---

## Overview

LANBox lets you start a file server with a single command and access it from any device on the same network.

Typical use cases:

- transferring files between PC and phone
- sharing files inside a home or office network
- temporary local file hosting
- testing HTTP/file workflows
- a small, readable reference implementation of a file web server

No accounts or cloud services are required.

---

## Key Features

- Web-based file manager
- File upload with progress tracking
- Drag and drop uploads (frontend-dependent)
- Folder navigation
- Create directories
- Rename files and folders
- Delete files and directories
- Direct file download
- Image preview support (frontend-dependent)
- Works across devices in the same LAN

---

## Requirements

- Rust toolchain (stable) + Cargo
- Any modern web browser

---

## Project Layout

Expected folders (defaults):

- `public/` — frontend (static files)
- `uploads/` — uploaded/shared files

---

## Installation

Clone the repository:

```bash
git clone https://github.com/USERNAME/rust-lanbox.git
cd rust-lanbox
```

Build:

```bash
cargo build --release
```

---

## Running the Server

### Quick start (defaults)

```bash
target/release/rust-lanbox
```

Defaults:

- `ROOT=./public`
- `UPLOAD_DIR=./uploads`
- `HOST=0.0.0.0`
- `PORT=8080`

### Run on your LAN IP (accessible from other devices)

Bind to all interfaces:

```bash
HOST=0.0.0.0 PORT=8080 ROOT=./public UPLOAD_DIR=./uploads target/release/rust-lanbox
```

After launch, the server prints both addresses:

- `LAN:  http://<your_lan_ip>:8080/`
- `Bind: http://0.0.0.0:8080/`

Open in your browser:

- Local: `http://localhost:8080`
- From another device in the same network: `http://YOUR_LAN_IP:8080`

---

## Configuration (Environment Variables)

- `ROOT` — absolute or relative path to the frontend folder (default: `./public`)
- `UPLOAD_DIR` — absolute or relative path to uploads folder (default: `./uploads`)
- `HOST` — bind address (default: `0.0.0.0`)
- `PORT` — port number (default: `8080`)

Example:

```bash
ROOT=./public UPLOAD_DIR=./uploads HOST=0.0.0.0 PORT=8080 target/release/rust-lanbox
```

---

## API Endpoints

List directory contents:

```
GET /api/list?dir=PATH
```

Upload file (binary stream):

```
POST /api/upload?dir=PATH&name=FILENAME
Content-Type: application/octet-stream
```

Create directory (JSON):

```
POST /api/mkdir
Body: {"dir":"/","name":"New Folder"}
```

Rename (JSON):

```
POST /api/rename
Body: {"path":"/old.txt","name":"new.txt"}
```

Delete:

```
DELETE /api/delete?path=TARGET
```

These endpoints are primarily intended for the built-in web interface but can also be used programmatically.

---

## Security Notes

LANBox is intended for **trusted local networks**.

Implemented protections include:

- path normalization
- directory escape prevention (anti-traversal)
- filename sanitization
- controlled file downloads

Running LANBox directly on the public internet is not recommended without additional protections (authentication, reverse proxy, firewall rules, etc.).

---

## License

MIT License
