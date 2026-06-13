import { useQuery } from '@tanstack/react-query'
import { api } from '../api'

export default function StatusBar() {
  const { data: status } = useQuery({
    queryKey: ['explorer-status'],
    queryFn: () => api.status(),
    refetchInterval: 10_000,
  })

  return (
    <div style={{
      height: 26,
      borderTop: '1px solid rgba(110,198,255,0.08)',
      background: 'rgba(4,7,18,0.97)',
      display: 'flex',
      alignItems: 'center',
      padding: '0 24px',
      fontFamily: "'JetBrains Mono', monospace",
      fontSize: 11,
      color: 'rgba(255,255,255,0.30)',
      flexShrink: 0,
      gap: 0,
    }}>
      <span style={{ marginRight: 16 }}>
        <span style={{ color: 'rgba(110,198,255,0.55)' }}>IRIUM</span>
        {' · '}explorer.iriumlabs.org
      </span>
      <span style={{ color: 'rgba(110,198,255,0.12)', margin: '0 4px' }}>|</span>
      <span style={{ margin: '0 16px' }}>
        height:{' '}
        <span style={{ color: '#6ec6ff' }}>
          {status ? status.synced_height.toLocaleString() : '—'}
        </span>
      </span>
      <span style={{ color: 'rgba(110,198,255,0.12)', margin: '0 4px' }}>|</span>
      <span style={{ margin: '0 16px' }}>
        network: <span style={{ color: 'rgba(255,255,255,0.50)' }}>mainnet</span>
      </span>
      <span style={{ color: 'rgba(110,198,255,0.12)', margin: '0 4px' }}>|</span>
      <span style={{ margin: '0 16px', display: 'flex', alignItems: 'center', gap: 6 }}>
        <span className="live-dot" style={{
          width: 5, height: 5, borderRadius: '50%',
          background: '#34d399', display: 'inline-block', flexShrink: 0,
        }} />
        synced
      </span>
    </div>
  )
}
