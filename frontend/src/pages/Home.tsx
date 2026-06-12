
import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router-dom'
import { api } from '../api'
import Card from '../components/Card'
import { satToIrm, timeAgo } from '../lib/fmt'

export default function Home() {
  const { data: status } = useQuery({ queryKey: ['status'], queryFn: api.status, refetchInterval: 5000 })
  const { data: blocks, isLoading } = useQuery({
    queryKey: ['blocks'], queryFn: () => api.blocks(20), refetchInterval: 10000
  })

  return (
    <div className="space-y-6">
      {status && (
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <Card>
            <div className="text-xs text-zinc-500 mb-1">Synced Height</div>
            <div className="mono text-3xl font-bold text-white">{status.synced_height.toLocaleString()}</div>
          </Card>
          <Card>
            <div className="text-xs text-zinc-500 mb-1">Best Block Hash</div>
            <div className="mono text-xs text-zinc-300 truncate">{status.synced_block_hash}</div>
          </Card>
        </div>
      )}

      <Card title="Latest Blocks">
        {isLoading ? (
          <div className="text-zinc-500 text-sm">Loading...</div>
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="text-zinc-500 text-xs uppercase border-b border-zinc-800">
                <th className="pb-2 text-left">Height</th>
                <th className="pb-2 text-left">Age</th>
                <th className="pb-2 text-left">Txns</th>
                <th className="pb-2 text-left">Miner</th>
                <th className="pb-2 text-right">Reward</th>
              </tr>
            </thead>
            <tbody>
              {blocks?.map(b => (
                <tr key={b.height} className="border-b border-zinc-800/50 hover:bg-zinc-800/30 transition-colors">
                  <td className="py-2">
                    <Link to={`/block/height/${b.height}`} className="mono text-sky-400 hover:text-sky-300">
                      {b.height.toLocaleString()}
                    </Link>
                  </td>
                  <td className="py-2 text-zinc-400">{timeAgo(b.timestamp)}</td>
                  <td className="py-2 mono text-zinc-300">{b.tx_count}</td>
                  <td className="py-2">
                    {b.miner_address ? (
                      <Link to={`/address/${b.miner_address}`} className="mono text-xs text-zinc-300 hover:text-zinc-100 truncate max-w-[140px] block">
                        {b.miner_address.slice(0, 16)}...
                      </Link>
                    ) : <span className="text-zinc-600">—</span>}
                  </td>
                  <td className="py-2 mono text-right text-zinc-300">{satToIrm(b.total_reward)} IRM</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>
    </div>
  )
}
