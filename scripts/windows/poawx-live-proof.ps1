<#
  Phase 24L - Irium PoAW-X Windows local live proof (devnet, loopback only).

  Flow: start an isolated local devnet iriumd -> verify it is up + serving
  /poawx/assignment at genesis -> run poawx-live-proof-harness (builds an
  all-gates block with Irium-native PoW and submits it via real RPC) -> verify
  the node accepted it and height advanced -> stop the node -> confirm the
  production default ~/.irium was NOT created.

  SAFETY: devnet only, loopback only, no public bind, mainnet untouched.

  NOTE ON PATHS: the node fails closed (exit 78) on storage dirs that do not
  resolve under the user's home (%USERPROFILE%) - this is the Phase 24C
  hardening. So the isolated root is created UNDER %USERPROFILE% (NOT C:\...),
  which also keeps it off the production default %USERPROFILE%\.irium.

  Run from the repo root:
    powershell -ExecutionPolicy Bypass -File scripts\windows\poawx-live-proof.ps1
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# --- isolated dirs (under %USERPROFILE%, never the default .irium) ---
$root      = Join-Path $env:USERPROFILE 'irium-poawx-live-proof'
$nodeData  = Join-Path $root 'node\data'
$nodeBlks  = Join-Path $root 'node\blocks'
$nodeState = Join-Path $root 'node\state'
$walletDir = Join-Path $root 'wallet'
$artifacts = Join-Path $root 'artifacts'
foreach ($d in @($nodeData, $nodeBlks, $nodeState, $walletDir, $artifacts)) {
    New-Item -ItemType Directory -Force -Path $d | Out-Null
}
Write-Host "[i] isolated root: $root"

# --- locate binaries (built with: cargo build --release --bin iriumd --bin poawx-live-proof-harness) ---
$iriumd  = Join-Path (Get-Location) 'target\release\iriumd.exe'
$harness = Join-Path (Get-Location) 'target\release\poawx-live-proof-harness.exe'
foreach ($b in @($iriumd, $harness)) {
    if (-not (Test-Path $b)) {
        throw "missing binary: $b  (run: cargo build --release --bin iriumd --bin poawx-live-proof-harness)"
    }
}

# --- node + harness share this environment (gates must match) ---
$env:IRIUM_NETWORK     = 'devnet'
$env:IRIUM_DATA_DIR    = $nodeData
$env:IRIUM_BLOCKS_DIR  = $nodeBlks
$env:IRIUM_STATE_DIR   = $nodeState
$env:IRIUM_NODE_HOST   = '127.0.0.1'
$env:IRIUM_NODE_PORT   = '41011'      # RPC + /poawx/* + /status (harness talks here)
$env:IRIUM_STATUS_HOST = '127.0.0.1'
$env:IRIUM_STATUS_PORT = '41008'      # lightweight status server
$env:IRIUM_P2P_BIND    = '127.0.0.1:41010'  # loopback P2P (no peers; not public)
$env:IRIUM_POAWX_MODE  = 'active'

$gates = @{
    'IRIUM_POAWX_ACTIVATION_HEIGHT'                  = '1'
    'IRIUM_POAWX_PUZZLE_DIFFICULTY_BITS'             = '4'   # receipt PoW difficulty
    'IRIUM_POAWX_PUZZLE_BITS'                        = '4'   # assigned-puzzle difficulty
    'IRIUM_POAWX_MULTI_ROLE_REWARD_ACTIVATION_HEIGHT'= '1'
    'IRIUM_POAWX_FAIRNESS_MATRIX_ACTIVATION_HEIGHT'  = '1'
    'IRIUM_POAWX_ANTI_DOMINATION_ACTIVATION_HEIGHT'  = '1'
    'IRIUM_POAWX_ANTI_DOMINATION_REQUIRED'           = '1'
    'IRIUM_POAWX_CANDIDATE_SET_ACTIVATION_HEIGHT'    = '1'
    'IRIUM_POAWX_CANDIDATE_SET_REQUIRED'             = '1'
    'IRIUM_POAWX_ASSIGNMENT_PROOF_ACTIVATION_HEIGHT' = '1'
    'IRIUM_POAWX_ASSIGNMENT_PROOF_REQUIRED'          = '1'
    'IRIUM_POAWX_CANDIDATE_ADMISSION_ACTIVATION_HEIGHT' = '1'
    'IRIUM_POAWX_CANDIDATE_ADMISSION_REQUIRED'       = '1'
    'IRIUM_POAWX_PUZZLE_WORK_ACTIVATION_HEIGHT'      = '1'
    'IRIUM_POAWX_PUZZLE_WORK_REQUIRED'               = '1'
    'IRIUM_POAWX_FINALITY_COMMITTEE_ACTIVATION_HEIGHT' = '1'
    'IRIUM_POAWX_FINALITY_COMMITTEE_REQUIRED'        = '1'
    'IRIUM_POAWX_FINALITY_THRESHOLD_NUM'             = '1'
    'IRIUM_POAWX_FINALITY_THRESHOLD_DEN'             = '1'
    'IRIUM_POAWX_COMMITTED_ADMISSION_ACTIVATION_HEIGHT' = '1'
    'IRIUM_POAWX_COMMITTED_ADMISSION_REQUIRED'       = '1'
    'IRIUM_POAWX_TRUE_VRF_ACTIVATION_HEIGHT'         = '1'
    'IRIUM_POAWX_TRUE_VRF_REQUIRED'                  = '1'
}
foreach ($k in $gates.Keys) { Set-Item -Path "env:$k" -Value $gates[$k] }

# --- snapshot the production default to confirm it stays untouched ---
$defaultIrium = Join-Path $env:USERPROFILE '.irium'
$defaultExistedBefore = Test-Path $defaultIrium

# --- start the node (loopback only) ---
$nodeOut = Join-Path $artifacts 'node.out.log'
$nodeErr = Join-Path $artifacts 'node.err.log'
Write-Host "[i] starting node: $iriumd  (RPC 127.0.0.1:41011)"
$node = Start-Process -FilePath $iriumd -PassThru -NoNewWindow `
    -RedirectStandardOutput $nodeOut -RedirectStandardError $nodeErr

$exit = 1
try {
    # --- wait until /poawx/assignment is served at genesis ---
    $ready = $false
    for ($i = 0; $i -lt 60; $i++) {
        try {
            $asg = Invoke-RestMethod -Uri 'http://127.0.0.1:41011/poawx/assignment' -TimeoutSec 3
            if ($null -ne $asg.height) { $ready = $true; break }
        } catch { Start-Sleep -Milliseconds 500 }
    }
    if (-not $ready) { throw 'node did not serve /poawx/assignment within timeout' }
    Write-Host "[i] /poawx/assignment OK (height=$($asg.height), pow_bits=$($asg.pow_bits))"

    # --- verify the node is using the isolated Windows dirs (not ~/.irium) ---
    if (Test-Path $nodeOut) {
        $iso = Select-String -Path $nodeOut, $nodeErr -SimpleMatch 'irium-poawx-live-proof' -ErrorAction SilentlyContinue
        if ($iso) { Write-Host "[i] node storage banner references the isolated root (good)" }
    }

    # --- run the live-proof harness (builds + mines + submits + verifies) ---
    Write-Host "[i] running harness..."
    & $harness --devnet --rpc-url 'http://127.0.0.1:41011' --work-dir $artifacts
    $exit = $LASTEXITCODE
}
finally {
    if ($node -and -not $node.HasExited) {
        Write-Host "[i] stopping node (PID $($node.Id))"
        Stop-Process -Id $node.Id -Force -ErrorAction SilentlyContinue
    }
}

# --- confirm the production default ~/.irium was not created by this run ---
$defaultExistsAfter = Test-Path $defaultIrium
if ((-not $defaultExistedBefore) -and $defaultExistsAfter) {
    Write-Warning "PROBLEM: $defaultIrium was created during the run (should never happen)"
} else {
    Write-Host "[i] production default $defaultIrium not created by this run (good)"
}

Write-Host "[i] artifacts: $artifacts"
if ($exit -eq 0) {
    Write-Host "[OK] Phase 24L Windows live proof SUCCEEDED"
} else {
    throw "Phase 24L live proof FAILED (harness exit $exit) - see $artifacts and node logs"
}
