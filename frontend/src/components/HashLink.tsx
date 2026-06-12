
import { Link } from 'react-router-dom'
import { shortHash } from '../lib/fmt'

interface Props { to: string; hash: string; full?: boolean }

export default function HashLink({ to, hash, full }: Props) {
  return (
    <Link to={to} className="mono text-sky-400 hover:text-sky-300 transition-colors text-sm">
      {full ? hash : shortHash(hash)}
    </Link>
  )
}
