import React from 'react'

interface Props {
  label: string
  value: React.ReactNode
  sub?: string
  icon?: React.ReactNode
}

export default function StatCard({ label, value, sub, icon }: Props) {
  return (
    <div style={{
      background: 'var(--bg-elev-1)',
      border: '1px solid rgba(110,198,255,0.10)',
      borderRadius: 8,
      padding: '16px 18px',
    }}>
      <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', marginBottom: 10 }}>
        <span style={{ fontSize: 10, fontWeight: 700, color: 'rgba(255,255,255,0.30)', textTransform: 'uppercase', letterSpacing: '0.13em' }}>
          {label}
        </span>
        {icon && <span style={{ color: 'rgba(110,198,255,0.50)' }}>{icon}</span>}
      </div>
      <div className="mono" style={{ fontSize: 17, fontWeight: 800, color: '#eef0ff', lineHeight: 1.2, fontVariantNumeric: 'tabular-nums' }}>
        {value}
      </div>
      {sub && <div style={{ fontSize: 11, color: 'rgba(255,255,255,0.25)', marginTop: 6 }}>{sub}</div>}
    </div>
  )
}
