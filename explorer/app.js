// Irium Explorer — shared API client and utilities
// All API calls target the Irium node RPC endpoint.
// The node URL is read from window.IRIUM_NODE_URL or defaults to the value below.

(function () {
  'use strict';

  // ---------- Configuration ----------
  // Override by setting window.IRIUM_NODE_URL before loading this script,
  // or by setting a <meta name="irium-node" content="https://..."> tag.
  function nodeUrl() {
    if (window.IRIUM_NODE_URL) return window.IRIUM_NODE_URL.replace(/\/$/, '');
    const meta = document.querySelector('meta[name="irium-node"]');
    if (meta) return meta.getAttribute('content').replace(/\/$/, '');
    return 'https://node.iriumlabs.org';
  }

  // ---------- API client ----------
  async function apiFetch(path) {
    const url = nodeUrl() + path;
    const resp = await fetch(url);
    if (!resp.ok) throw new Error(`HTTP ${resp.status} from ${url}`);
    return resp.json();
  }

  window.IriumExplorer = {
    nodeUrl,
    api: {
      stats: () => apiFetch('/explorer/stats'),
      agreements: (page, limit) => apiFetch(`/explorer/agreements?page=${page || 1}&limit=${limit || 20}`),
      agreement: (hash) => apiFetch(`/explorer/agreement/${hash}`),
      proofs: (page, limit, agreementHash) => {
        let url = `/explorer/proofs?page=${page || 1}&limit=${limit || 20}`;
        if (agreementHash) url += `&agreement_hash=${agreementHash}`;
        return apiFetch(url);
      },
      reputation: (pubkey) => apiFetch(`/explorer/reputation/${pubkey}`),
      status: () => apiFetch('/status'),
    },
  };

  // ---------- Utilities ----------
  window.IriumExplorer.fmt = {
    irm: (atoms) => {
      if (atoms == null) return '—';
      const whole = Math.floor(atoms / 1e8);
      const frac = atoms % 1e8;
      if (frac === 0) return `${whole} IRM`;
      return `${whole}.${String(frac).padStart(8, '0').replace(/0+$/, '')} IRM`;
    },
    ts: (unix) => {
      if (!unix) return '—';
      return new Date(unix * 1000).toLocaleString();
    },
    shortHash: (h) => {
      if (!h) return '—';
      return h.length > 16 ? h.slice(0, 8) + '…' + h.slice(-8) : h;
    },
    stateColor: (state) => {
      const colors = {
        draft: '#888',
        proposed: '#888',
        funded: '#2196f3',
        partially_released: '#ff9800',
        released: '#4caf50',
        refunded: '#ff9800',
        expired: '#f44336',
        cancelled: '#f44336',
        disputed_metadata_only: '#f44336',
      };
      return colors[state] || '#888';
    },
    capitalize: (s) => s ? s.charAt(0).toUpperCase() + s.slice(1).replace(/_/g, ' ') : '',
  };

  // ---------- Navigation helpers ----------
  window.IriumExplorer.nav = {
    toAgreement: (hash) => { location.href = `agreement.html?hash=${hash}`; },
    toReputation: (pubkey) => { location.href = `reputation.html?pubkey=${pubkey}`; },
  };
})();
