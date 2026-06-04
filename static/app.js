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
  }

  document.querySelectorAll('[data-delete]').forEach((btn) => {
    btn.addEventListener('click', async () => {
      const key = btn.getAttribute('data-delete');
      if (!key) return;
      if (!confirm('Delete "' + key + '"? This cannot be undone.')) return;
      btn.disabled = true;
      try {
        const res = await fetch(API.del(key), { method: 'DELETE' });
        if (!res.ok) throw new Error('HTTP ' + res.status);
        showToast('Deleted ' + key, 'ok');
        const li = btn.closest('li');
        if (li) li.remove();
      } catch (e) {
        btn.disabled = false;
        showToast('Delete failed: ' + e.message, 'error');
      }
    });
  });

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

  const newFolderBtn = document.getElementById('new-folder-btn');
  const newFolderDlg = document.getElementById('new-folder-dialog');
  const cancelFolder = document.getElementById('cancel-folder');
  if (newFolderBtn && newFolderDlg) {
    newFolderBtn.addEventListener('click', () => newFolderDlg.showModal());
  }
  if (cancelFolder && newFolderDlg) {
    cancelFolder.addEventListener('click', () => newFolderDlg.close());
  }
})();
