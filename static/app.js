(function () {
  'use strict';

  const body = document.body;
  const prefix = body ? body.getAttribute('data-prefix') || '' : '';
  const API = {
    upload: '/api/upload',
    presign: (key, dl) => '/api/presign?key=' + encodeURIComponent(key) + (dl ? '&download=1' : ''),
    del: (key) => '/api/objects/' + encodeURIComponent(key),
    delPrefix: (p) => '/api/delete-prefix/' + encodeURIComponent(p),
  };

  const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  function showToast(msg, type) {
    const t = document.getElementById('toast');
    if (!t) return;
    t.textContent = msg;
    t.className = 'toast ' + (type || '');
    t.hidden = false;
    clearTimeout(showToast._timer);
    showToast._timer = setTimeout(() => { t.hidden = true; }, 3000);
  }

  window.addEventListener('toast', (e) => showToast(e.detail));

  // ------------------------------------------------------------------
  // Upload zone
  // ------------------------------------------------------------------
  const zone = document.getElementById('upload-zone');
  const fileInput = document.getElementById('file-input');
  const progress = document.getElementById('upload-progress');
  const bar = document.getElementById('upload-bar');
  const label = document.getElementById('upload-label');

  function humanSize(bytes) {
    const u = ['B', 'KB', 'MB', 'GB', 'TB'];
    let s = bytes, i = 0;
    while (s >= 1024 && i < u.length - 1) { s /= 1024; i++; }
    return (i === 0 ? s : s.toFixed(2)) + ' ' + u[i];
  }

  function uploadFile(file) {
    return new Promise((resolve, reject) => {
      const xhr = new XMLHttpRequest();
      const form = new FormData();
      form.append('file', file);
      xhr.open('POST', API.upload);
      xhr.setRequestHeader('Accept', 'application/json');
      xhr.setRequestHeader('X-Requested-With', 'XMLHttpRequest');
      if (prefix) {
        xhr.setRequestHeader('X-Upload-Prefix', prefix);
      }
      xhr.upload.addEventListener('progress', (e) => {
        if (!e.lengthComputable || !progress || !bar || !label) return;
        const pct = Math.round((e.loaded / e.total) * 100);
        bar.style.width = pct + '%';
        label.textContent = pct + '% · ' + humanSize(e.loaded) + ' / ' + humanSize(e.total);
      });
      xhr.addEventListener('load', () => {
        if (xhr.status >= 200 && xhr.status < 300) {
          try { resolve(JSON.parse(xhr.responseText)); } catch (_) { resolve({}); }
        } else {
          reject(new Error('HTTP ' + xhr.status + ' ' + xhr.responseText));
        }
      });
      xhr.addEventListener('error', () => reject(new Error('network error')));
      xhr.send(form);
    });
  }

  async function uploadAll(files) {
    if (!files || !files.length) return;
    if (progress) progress.hidden = false;
    let ok = 0, fail = 0;
    for (let i = 0; i < files.length; i++) {
      const f = files[i];
      if (bar) bar.style.width = '0%';
      if (label) label.textContent = (i + 1) + '/' + files.length + ' · ' + f.name;
      try {
        await uploadFile(f);
        ok++;
      } catch (e) {
        fail++;
        showToast('Failed: ' + f.name + ' (' + e.message + ')', 'error');
      }
    }
    showToast(ok + ' uploaded' + (fail ? ', ' + fail + ' failed' : ''), fail ? 'error' : 'ok');
    setTimeout(() => location.reload(), 700);
  }

  if (fileInput) {
    fileInput.addEventListener('change', (e) => uploadAll(e.target.files));
  }
  if (zone) {
    ['dragenter', 'dragover'].forEach((ev) => {
      zone.addEventListener(ev, (e) => { e.preventDefault(); zone.classList.add('hot'); });
    });
    ['dragleave', 'drop'].forEach((ev) => {
      zone.addEventListener(ev, (e) => { e.preventDefault(); zone.classList.remove('hot'); });
    });
    zone.addEventListener('drop', (e) => {
      const files = Array.from(e.dataTransfer.files || []);
      uploadAll(files);
    });
    zone.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        if (fileInput) fileInput.click();
      }
    });
  }

  // ------------------------------------------------------------------
  // Native confirm dialog
  // ------------------------------------------------------------------
  const confirmDlg = document.getElementById('confirm-dialog');
  function confirmAction({ title, body, okLabel, danger }) {
    return new Promise((resolve) => {
      if (!confirmDlg || typeof confirmDlg.showModal !== 'function') {
        resolve(window.confirm((title ? title + '\n\n' : '') + (body || '')));
        return;
      }
      const t = confirmDlg.querySelector('#confirm-dialog-title');
      const b = confirmDlg.querySelector('#confirm-dialog-body');
      const ok = confirmDlg.querySelector('#confirm-dialog-ok');
      if (t) t.textContent = title || 'Are you sure?';
      if (b) b.textContent = body || '';
      if (ok) {
        ok.textContent = okLabel || 'Confirm';
        ok.classList.toggle('danger', !!danger);
        ok.classList.toggle('primary', !danger);
      }
      function onClose() {
        confirmDlg.removeEventListener('close', onClose);
        resolve(confirmDlg.returnValue === 'confirm');
      }
      confirmDlg.addEventListener('close', onClose);
      confirmDlg.showModal();
    });
  }

  // ------------------------------------------------------------------
  // Delete handlers (file and prefix)
  // ------------------------------------------------------------------
  document.querySelectorAll('[data-delete]').forEach((btn) => {
    btn.addEventListener('click', async () => {
      const key = btn.getAttribute('data-delete');
      if (!key) return;
      const ok = await confirmAction({
        title: 'Delete file?',
        body: 'This will permanently delete "' + key + '". This cannot be undone.',
        okLabel: 'Delete',
        danger: true,
      });
      if (!ok) return;
      btn.disabled = true;
      try {
        const res = await fetch(API.del(key), { method: 'DELETE' });
        if (!res.ok) throw new Error('HTTP ' + res.status);
        showToast('Deleted ' + key, 'ok');
        const li = btn.closest('li');
        if (li) {
          if (!reduceMotion) {
            li.style.transition = 'opacity 200ms, transform 200ms';
            li.style.opacity = '0';
            li.style.transform = 'translateX(-8px)';
            setTimeout(() => li.remove(), 200);
          } else {
            li.remove();
          }
        }
      } catch (e) {
        btn.disabled = false;
        showToast('Delete failed: ' + e.message, 'error');
      }
    });
  });

  document.querySelectorAll('.delete-prefix-form').forEach((form) => {
    form.addEventListener('submit', async (e) => {
      e.preventDefault();
      const name = form.getAttribute('data-name') || 'folder';
      const ok = await confirmAction({
        title: 'Delete folder?',
        body: 'This will permanently delete the folder "' + name + '" and all of its contents. This cannot be undone.',
        okLabel: 'Delete folder',
        danger: true,
      });
      if (ok) form.submit();
    });
  });

  document.querySelectorAll('[data-move-confirm]').forEach((btn) => {
    btn.addEventListener('click', async () => {
      const msg = btn.getAttribute('data-move-confirm') || 'Move will delete the source object.';
      const ok = await confirmAction({
        title: 'Move object?',
        body: msg,
        okLabel: 'Move',
        danger: true,
      });
      if (ok) {
        const form = btn.closest('form');
        if (form) form.submit();
      }
    });
  });

  // ------------------------------------------------------------------
  // Copy presigned URL
  // ------------------------------------------------------------------
  document.querySelectorAll('.copy-presign').forEach((btn) => {
    btn.addEventListener('click', async () => {
      const url = btn.getAttribute('data-url');
      if (!url) return;
      btn.disabled = true;
      try {
        const res = await fetch(url, { headers: { Accept: 'application/json' } });
        if (!res.ok) throw new Error('HTTP ' + res.status);
        const data = await res.json();
        const u = data.url || url;
        try {
          await navigator.clipboard.writeText(u);
          showToast('Copied presigned URL', 'ok');
        } catch (_) {
          prompt('Copy this URL', u);
        }
      } catch (e) {
        showToast('Presign failed: ' + e.message, 'error');
      } finally {
        btn.disabled = false;
      }
    });
  });

  // ------------------------------------------------------------------
  // New folder dialog
  // ------------------------------------------------------------------
  const newFolderBtn = document.getElementById('new-folder-btn');
  const newFolderDlg = document.getElementById('new-folder-dialog');
  if (newFolderBtn && newFolderDlg) {
    newFolderBtn.addEventListener('click', () => {
      newFolderDlg.showModal();
      const input = newFolderDlg.querySelector('input[name="name"]');
      if (input) setTimeout(() => input.focus(), 0);
    });
  }

  // ------------------------------------------------------------------
  // Search as you type (debounced)
  // ------------------------------------------------------------------
  const searchForm = document.getElementById('search-form');
  const searchInput = document.getElementById('search-input');
  if (searchForm && searchInput) {
    let t = null;
    searchInput.addEventListener('input', () => {
      clearTimeout(t);
      t = setTimeout(() => {
        if (searchInput.value.length === 0 || searchInput.value.length >= 2) {
          searchForm.submit();
        }
      }, 300);
    });
  }

  // ------------------------------------------------------------------
  // Command palette
  // ------------------------------------------------------------------
  const palette = document.getElementById('command-palette');
  const paletteInput = document.getElementById('palette-input');
  const paletteList = document.getElementById('palette-list');
  const paletteBtn = document.getElementById('palette-btn');

  const commands = buildCommands();

  function buildCommands() {
    const cmds = [
      { id: 'home', label: 'Go to root', hint: '/', run: () => location.href = '/' },
      { id: 'json', label: 'View current folder as JSON', hint: 'API', run: () => { const p = new URLSearchParams(location.search); p.set('view', 'json'); location.search = '?' + p.toString(); } },
      { id: 'newfolder', label: 'New folder', hint: 'N', run: () => { if (newFolderBtn) newFolderBtn.click(); } },
      { id: 'upload', label: 'Choose files to upload', hint: '', run: () => { if (fileInput) fileInput.click(); } },
      { id: 'reload', label: 'Reload page', hint: 'R', run: () => location.reload() },
    ];
    document.querySelectorAll('.file-row--file').forEach((row) => {
      const key = row.getAttribute('data-key');
      if (!key) return;
      const name = (key.split('/').pop()) || key;
      cmds.push({ id: 'preview-' + key, label: 'Preview: ' + name, hint: 'browse', run: () => { location.href = '/preview/' + encodeURIComponent(key); } });
      cmds.push({ id: 'presign-' + key, label: 'Get presigned URL: ' + name, hint: 'browse', run: () => copyPresignForKey(key) });
    });
    document.querySelectorAll('.file-row--folder').forEach((row) => {
      const p = row.getAttribute('data-prefix');
      if (!p) return;
      const name = (p.split('/').filter(Boolean).pop()) || p;
      cmds.push({ id: 'open-' + p, label: 'Open folder: ' + name, hint: 'browse', run: () => { location.href = '/browse?prefix=' + encodeURIComponent(p); } });
    });
    return cmds;
  }

  async function copyPresignForKey(key) {
    try {
      const res = await fetch(API.presign(key, false), { headers: { Accept: 'application/json' } });
      if (!res.ok) throw new Error('HTTP ' + res.status);
      const data = await res.json();
      const u = data.url || API.presign(key, false);
      try { await navigator.clipboard.writeText(u); showToast('Copied presigned URL', 'ok'); }
      catch (_) { prompt('Copy this URL', u); }
    } catch (e) { showToast('Presign failed: ' + e.message, 'error'); }
  }

  function fuzzyScore(query, text) {
    if (!query) return 1;
    query = query.toLowerCase();
    text = text.toLowerCase();
    let qi = 0, score = 0, lastMatch = -2;
    for (let i = 0; i < text.length && qi < query.length; i++) {
      if (text[i] === query[qi]) {
        score += (i - lastMatch === 1) ? 3 : 1;
        if (i === 0) score += 2;
        lastMatch = i;
        qi++;
      }
    }
    return qi === query.length ? score - (text.length - lastMatch) * 0.01 : 0;
  }

  function renderPalette(query) {
    if (!paletteList) return;
    const list = commands
      .map((c) => ({ c, s: fuzzyScore(query, c.label) }))
      .filter((x) => x.s > 0)
      .sort((a, b) => b.s - a.s)
      .slice(0, 20);
    if (list.length === 0) {
      paletteList.innerHTML = '<li class="palette__empty">No matches</li>';
      return;
    }
    paletteList.innerHTML = list.map(({ c }, idx) =>
      '<li class="palette__item' + (idx === 0 ? ' is-active' : '') + '" data-cmd="' + c.id + '" role="option" aria-selected="' + (idx === 0) + '"><span class="palette__label">' + escapeHtml(c.label) + '</span><span class="palette__hint">' + escapeHtml(c.hint || '') + '</span></li>'
    ).join('');
  }

  function escapeHtml(s) {
    return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#x27;' }[c]));
  }

  let paletteActiveIndex = 0;
  function paletteSetActive(idx) {
    if (!paletteList) return;
    const items = paletteList.querySelectorAll('.palette__item');
    if (items.length === 0) return;
    paletteActiveIndex = ((idx % items.length) + items.length) % items.length;
    items.forEach((it, i) => {
      const isActive = i === paletteActiveIndex;
      it.classList.toggle('is-active', isActive);
      it.setAttribute('aria-selected', isActive ? 'true' : 'false');
      if (isActive) it.scrollIntoView({ block: 'nearest' });
    });
  }

  function paletteRunIndex(idx) {
    if (!paletteList) return;
    const items = paletteList.querySelectorAll('.palette__item');
    const li = items[idx];
    if (!li) return;
    const id = li.getAttribute('data-cmd');
    const cmd = commands.find((c) => c.id === id);
    palette.close();
    if (cmd) cmd.run();
  }

  function openPalette() {
    if (!palette) return;
    paletteActiveIndex = 0;
    renderPalette('');
    if (paletteInput) {
      paletteInput.value = '';
      palette.showModal();
      setTimeout(() => paletteInput.focus(), 0);
    } else {
      palette.showModal();
    }
  }

  if (paletteBtn) paletteBtn.addEventListener('click', openPalette);
  document.querySelectorAll('[data-open-command]').forEach((b) => b.addEventListener('click', openPalette));

  if (palette && paletteInput) {
    paletteInput.addEventListener('input', () => {
      paletteActiveIndex = 0;
      renderPalette(paletteInput.value);
    });
    paletteInput.addEventListener('keydown', (e) => {
      if (e.key === 'ArrowDown') { e.preventDefault(); paletteSetActive(paletteActiveIndex + 1); }
      else if (e.key === 'ArrowUp') { e.preventDefault(); paletteSetActive(paletteActiveIndex - 1); }
      else if (e.key === 'Enter') { e.preventDefault(); paletteRunIndex(paletteActiveIndex); }
    });
  }

  if (paletteList) {
    paletteList.addEventListener('click', (e) => {
      const li = e.target.closest('.palette__item');
      if (!li) return;
      const items = Array.from(paletteList.querySelectorAll('.palette__item'));
      paletteRunIndex(items.indexOf(li));
    });
    paletteList.addEventListener('mousemove', (e) => {
      const li = e.target.closest('.palette__item');
      if (!li) return;
      const items = Array.from(paletteList.querySelectorAll('.palette__item'));
      const idx = items.indexOf(li);
      if (idx >= 0) paletteSetActive(idx);
    });
  }

  // ------------------------------------------------------------------
  // Global keyboard shortcuts
  // ------------------------------------------------------------------
  document.addEventListener('keydown', (e) => {
    const mod = e.ctrlKey || e.metaKey;
    const target = e.target;
    const inEditable = target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable);

    if (mod && (e.key === 'k' || e.key === 'K')) {
      e.preventDefault();
      openPalette();
      return;
    }
    if (e.key === 'Escape' && palette && palette.open) {
      if (palette.open) palette.close();
    }
    if (inEditable) return;
    if (e.key === '/') {
      e.preventDefault();
      if (searchInput) searchInput.focus();
    } else if (e.key === 'n' || e.key === 'N') {
      e.preventDefault();
      if (newFolderBtn) newFolderBtn.click();
    } else if (e.key === 'r' || e.key === 'R') {
      e.preventDefault();
      location.reload();
    }
  });
})();
