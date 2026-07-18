// Bridges the page's window.__done(json) to the main process for the kill-check.
const { contextBridge, ipcRenderer } = require('electron')
contextBridge.exposeInMainWorld('__done', (json) => ipcRenderer.send('killcheck-done', json))
