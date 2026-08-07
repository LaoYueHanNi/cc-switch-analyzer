# Cursor local-usage hook entry (Windows).
# Read stdin as RAW BYTES -> temp file -> log_request.ps1
# IMPORTANT: comments must be ASCII only (PS 5.1 misparses UTF-8 Chinese without BOM).
$ErrorActionPreference = 'SilentlyContinue'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$logger = Join-Path $here 'log_request.ps1'
$meta = Join-Path $here 'last_stdin_meta.txt'
$lastPayload = Join-Path $here 'last_payload.json'
$heartbeatPath = Join-Path $here 'hook-heartbeat.json'

function Now-Unix {
  return [int64][Math]::Floor((Get-Date).ToUniversalTime().Subtract((Get-Date '1970-01-01')).TotalSeconds)
}

function Write-Heartbeat {
  param($ok, $errorMessage, $eventName)
  $hb = [ordered]@{
    version     = 1
    lastOkAt    = if ($ok) { Now-Unix } else { $null }
    lastEvent   = if ($eventName) { $eventName } else { $null }
    writeOk     = $ok
    lastError   = if ($errorMessage) { $errorMessage } else { $null }
    lastErrorAt = if ($errorMessage) { Now-Unix } else { $null }
  }
  try {
    $json = ($hb | ConvertTo-Json -Compress -Depth 3)
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($heartbeatPath, $json, $utf8)
  } catch {}
}

# Pause switch: when .hook-write-disabled exists, stop writing new records.
# Existing requests.jsonl keeps serving attribution / token identification.
$disabledFile = Join-Path $here '.hook-write-disabled'
if (Test-Path -LiteralPath $disabledFile) {
  Write-Heartbeat -ok $true -eventName 'paused'
  exit 0
}

$bytes = New-Object byte[] 0
try {
  $stdin = [Console]::OpenStandardInput()
  $ms = New-Object System.IO.MemoryStream
  $stdin.CopyTo($ms)
  $bytes = $ms.ToArray()
  $ms.Dispose()
} catch {
  $bytes = New-Object byte[] 0
}

if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
  $tmp = New-Object byte[] ($bytes.Length - 3)
  [Array]::Copy($bytes, 3, $tmp, 0, $tmp.Length)
  $bytes = $tmp
}

$len = $bytes.Length
$hex = if ($len -gt 0) { [BitConverter]::ToString($bytes[0..([Math]::Min(11, $len - 1))]) } else { '' }
$preview = ''
if ($len -gt 0) {
  $preview = [System.Text.Encoding]::UTF8.GetString($bytes, 0, [Math]::Min(80, $len)).Replace("`r", ' ').Replace("`n", ' ')
}
"ts=$(Get-Date -Format o); len=$len; hex=$hex; preview=$preview" | Set-Content -LiteralPath $meta -Encoding UTF8

# Debug dump; concurrent hooks may race - ignore failures
try {
  [System.IO.File]::WriteAllBytes($lastPayload, $bytes)
} catch {}

$payloadFile = Join-Path $here ('payload-' + [guid]::NewGuid().ToString('N') + '.json')
try {
  [System.IO.File]::WriteAllBytes($payloadFile, $bytes)
  $stdout = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $logger -PayloadFile $payloadFile 2>$null
  if ($stdout) { Write-Output $stdout }
} catch {
  Write-Heartbeat -ok $false -errorMessage "logger spawn failed: $($_.Exception.Message)"
} finally {
  if (Test-Path -LiteralPath $payloadFile) {
    Remove-Item -LiteralPath $payloadFile -Force -ErrorAction SilentlyContinue
  }
}
exit 0
