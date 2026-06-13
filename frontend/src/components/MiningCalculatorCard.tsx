import React, { useState } from 'react'

const UNITS = ['H/s', 'KH/s', 'MH/s', 'GH/s'] as const
const MULTIPLIERS: Record<string, number> = { 'H/s': 1, 'KH/s': 1e3, 'MH/s': 1e6, 'GH/s': 1e9 }

interface Props {
  networkHashrate: number
  blockReward: number
}

export default function MiningCalculatorCard({ networkHashrate, blockReward }: Props) {
  const [hashrate, setHashrate] = useState('100')
  const [unit, setUnit] = useState<typeof UNITS[number]>('MH/s')

  const myHps = (parseFloat(hashrate) || 0) * MULTIPLIERS[unit]
  const share = networkHashrate > 0 ? myHps / networkHashrate : 0
  const blocksPerDay = 86400 / 120
  const myIrmPerDay = share * blocksPerDay * blockReward

  function fmtEarnings(n: number) {
    if (networkHashrate === 0) return '—'
    if (n < 0.0001) return '< 0.0001 IRM/day'
    return n.toFixed(4) + ' IRM/day'
  }

  const SL: React.CSSProperties = {
    display: 'flex', alignItems: 'center', gap: 8,
    fontSize: 10, fontWeight: 700, color: 'rgba(110,198,255,0.55)',
    textTransform: 'uppercase', letterSpacing: '0.16em',
    marginBottom: 14,
  }

  return (
    <div style={{
      background: 'var(--bg-elev-1)',
      border: '1px solid rgba(110,198,255,0.10)',
      borderRadius: 8,
      padding: '18px 20px',
    }}>
      <div style={SL}>Mining Calculator</div>

      <div style={{ display: 'flex', gap: 8, marginBottom: 16 }}>
        <input
          type="number"
          value={hashrate}
          onChange={e => setHashrate(e.target.value)}
          min="0"
          placeholder="0"
          style={{
            flex: 1, background: 'rgba(255,255,255,0.05)',
            border: '1px solid rgba(110,198,255,0.18)',
            borderRadius: 6, padding: '7px 12px',
            fontSize: 14, fontFamily: "'JetBrains Mono', monospace",
            color: '#eef0ff', outline: 'none', minWidth: 0,
          }}
        />
        <select
          value={unit}
          onChange={e => setUnit(e.target.value as typeof UNITS[number])}
          style={{
            background: 'rgba(8,11,22,0.90)',
            border: '1px solid rgba(110,198,255,0.18)',
            borderRadius: 6, padding: '7px 10px',
            fontSize: 12, fontFamily: "'Space Grotesk', sans-serif",
            color: '#eef0ff', cursor: 'pointer', outline: 'none',
          }}
        >
          {UNITS.map(u => <option key={u} value={u} style={{ background: '#0A1226' }}>{u}</option>)}
        </select>
      </div>

      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        borderLeft: '2px solid #a78bfa', paddingLeft: 10, marginBottom: 10,
      }}>
        <span style={{ fontSize: 11, color: 'rgba(255,255,255,0.40)', fontWeight: 500 }}>Protocol</span>
        <span style={{ fontSize: 12, color: '#a78bfa', fontFamily: "'JetBrains Mono', monospace", fontWeight: 600 }}>IRM / LWMA-3 PoW</span>
      </div>

      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        borderLeft: '2px solid #34d399', paddingLeft: 10,
      }}>
        <span style={{ fontSize: 11, color: 'rgba(255,255,255,0.40)', fontWeight: 500 }}>Est. daily earnings</span>
        <span style={{ fontSize: 12, color: '#34d399', fontFamily: "'JetBrains Mono', monospace", fontWeight: 600 }}>
          {fmtEarnings(myIrmPerDay)}
        </span>
      </div>
    </div>
  )
}
