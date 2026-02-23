# LANBox

LANBox is a lightweight LAN file web server written in pure Node.js
without external libraries or frameworks.\
It provides a simple browser-based interface for transferring, managing,
and sharing files between devices inside a local network.

The project is designed both for everyday users who need a quick file
sharing solution and for developers who want a clean example of a
dependency‑free Node.js web server.

------------------------------------------------------------------------

## Overview

LANBox allows you to start a file server with a single command and
immediately access it from any device connected to the same network.

Typical use cases:

-   transferring files between PC and phone
-   sharing files inside a home or office network
-   temporary local file hosting
-   testing HTTP/file workflows
-   learning how a web server works internally without frameworks

No installation, accounts, or cloud services are required.

------------------------------------------------------------------------

## Key Features

-   Web-based file manager
-   File upload with progress tracking
-   Drag and drop uploads
-   Folder navigation
-   Create directories
-   Rename files and folders
-   Delete files and directories
-   Direct file download
-   Image preview support
-   Works across devices in the same LAN
-   Zero external dependencies

------------------------------------------------------------------------

## Requirements

-   Node.js 18 or newer
-   Any modern web browser

------------------------------------------------------------------------

## Installation

Clone the repository:

    git clone https://github.com/USERNAME/lanbox.git
    cd lanbox

No additional setup or package installation is required.

------------------------------------------------------------------------

## Running the Server

Start LANBox:

    node server.js

After launch, the terminal will display local access addresses.

Open in your browser:

    http://localhost:8080

Or from another device in the same network:

    http://YOUR_LAN_IP:8080

------------------------------------------------------------------------

## How It Works

LANBox uses only built‑in Node.js modules:

-   http
-   fs
-   path
-   os
-   crypto

The server:

-   serves static frontend files
-   exposes a small JSON API
-   streams uploads directly to disk
-   prevents directory traversal
-   normalizes filesystem paths
-   safely handles filenames

The frontend communicates with the server using standard HTTP requests
and binary streaming uploads.

------------------------------------------------------------------------

## API Endpoints

List directory contents:

    GET /api/list?dir=PATH

Upload file:

    POST /api/upload?dir=PATH&name=FILENAME
    Content-Type: application/octet-stream

Create directory:

    POST /api/mkdir

Rename:

    POST /api/rename

Delete:

    DELETE /api/delete?path=TARGET

These endpoints are primarily intended for the built‑in web interface
but can also be used programmatically.

------------------------------------------------------------------------

## Security Notes

LANBox is intended for trusted local networks.

Implemented protections:

-   path normalization
-   directory escape prevention
-   filename sanitization
-   controlled file downloads

Running LANBox directly on the public internet is not recommended
without additional security measures such as authentication, reverse
proxying, or firewall restrictions.

------------------------------------------------------------------------

## Target Users

LANBox is suitable for:

-   everyday users needing fast local file transfer
-   developers wanting a minimal HTTP server example
-   students learning Node.js networking
-   engineers testing local workflows
-   administrators deploying temporary LAN utilities

------------------------------------------------------------------------

## Development Philosophy

LANBox intentionally avoids frameworks and dependencies to remain:

-   transparent
-   understandable
-   portable
-   easy to modify
-   educational

Every part of the server is readable and hackable.

------------------------------------------------------------------------

## Possible Extensions

Examples of future improvements:

-   authentication
-   HTTPS support
-   archive download
-   video streaming
-   search functionality
-   upload limits
-   logging and monitoring

------------------------------------------------------------------------

## License

MIT License

------------------------------------------------------------------------

LANBox exists to make local file sharing simple while remaining
technically clean and educational.
