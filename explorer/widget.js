// Irium Network Stats Widget — embeddable JS snippet
// Usage: <script src="https://explorer.iriumlabs.org/widget.js" data-node="https://node.iriumlabs.org"></script>
// Renders a compact stats bar wherever the script tag appears.
(function () {
  'use strict';
  const script = document.currentScript;
  const nodeUrl = (script && script.getAttribute('data-node')) || 'https://node.iriumlabs.org';

  function el(tag, attrs, text) {
    const e = document.createElement(tag);
    Object.entries(attrs || {}).forEach(([k, v]) => e.setAttribute(k, v));
    if (text != null) e.textContent = text;
    return e;
  }

  function render(data) {
    const container = el('div', {
      style: 'display:inline-flex;gap:16px;align-items:center;font-family:monospace;font-size:13px;'
             + 'background:#111;color:#eee;padding:6px 12px;border-radius:4px;'
    });
    const items = [
      ['Height', data.chain_height],
      ['Agreements', data.total_agreements],
      ['Proofs', data.total_proofs],
      ['Peers', data.peer_count],
    ];
    items.forEach(([label, value]) => {
      const span = el('span', {});
      const lb = el('span', { style: 'color:#aaa;margin-right:4px;' }, label + ':');
      const vb = el('span', { style: 'color:#4fc3f7;font-weight:bold;' }, String(value));
      span.appendChild(lb);
      span.appendChild(vb);
      container.appendChild(span);
    });
    return container;
  }

  function mount() {
    const placeholder = el('div', { class: 'irium-widget', style: 'display:inline-block;' });
    if (script && script.parentNode) {
      script.parentNode.insertBefore(placeholder, script.nextSibling);
    } else {
      document.body.appendChild(placeholder);
    }
    fetch(nodeUrl + '/explorer/stats')
      .then(r => r.json())
      .then(data => {
        placeholder.innerHTML = '';
        placeholder.appendChild(render(data));
      })
      .catch(() => {
        placeholder.textContent = 'Irium: node unavailable';
        placeholder.style.cssText = 'font-family:monospace;font-size:13px;color:#f44;';
      });
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', mount);
  } else {
    mount();
  }
})();
