const IRM_DECIMALS = 8;
const IRM_FACTOR = 10 ** IRM_DECIMALS;

export function formatIrm(satoshis: number): string {
  return (satoshis / IRM_FACTOR).toFixed(IRM_DECIMALS).replace(/\.?0+$/, "") || "0";
}

export function parseIrm(irm: string): number {
  const n = parseFloat(irm);
  if (isNaN(n)) throw new Error(`Invalid IRM amount: ${irm}`);
  return Math.round(n * IRM_FACTOR);
}

// Irium addresses start with Q and are Base58Check encoded
export function isValidAddress(address: string): boolean {
  if (!address.startsWith("Q")) return false;
  if (address.length < 25 || address.length > 35) return false;
  return /^[Q][1-9A-HJ-NP-Za-km-z]{24,34}$/.test(address);
}

export function irmToSatoshis(irm: number): number {
  return Math.round(irm * IRM_FACTOR);
}

export function satoshisToIrm(satoshis: number): number {
  return satoshis / IRM_FACTOR;
}
