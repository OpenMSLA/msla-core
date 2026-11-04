<p align="center">
  <img src="resources/logo.png" height="64" />
</p>

<h1 align="center">DIY MSLA 3D Printer Core Code</h1>

<img src="https://img.shields.io/endpoint?url=https://ghloc.vercel.app/api/Andcool-Systems/msla-core/badge?filter=.rs$"/>

---
<table align="center">
  <tr>
    <td rowspan="2">
      <img src="resources/irl.jpg">
    </td>
    <td>
      <img src="resources/123.jpg" height="300">
    </td>
  </tr>
  <tr>
    <td>
      <img src="resources/benchy.jpg" width="100%">
    </td>
  </tr>
</table>

## Daemon Features
- Fully async
- Reading `.zip` and `.photon` model files
- Interprets IR into printer commands
- Communicating with [peripheral](https://github.com/Andcool-Systems/msla-peripheral) ESP32 and [control display](https://github.com/Andcool-Systems/msla-display) ESP32 through custom byte- UART-based protocol
- Hosting a REST API for remote print controlling
- Allows to find printers in LAN
- Uses native Linux framebuffer to display layer images

### Project still under heavy dev!

---
**by AndcoolSystems, August 26, 2026**
