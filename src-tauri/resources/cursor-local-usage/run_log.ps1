# Cursor local-usage hook entry (Windows).
# Read stdin as RAW BYTES -> temp file -> log_request.ps1
$ErrorActionPreference = 'SilentlyContinue'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$logger = Join-Path $here 'log_request.ps1'
$meta = Join-Path $here 'last_stdin_meta.txt'
$lastPayload = Join-Path $here 'last_payload.json'

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
[System.IO.File]::WriteAllBytes($lastPayload, $bytes)

$payloadFile = Join-Path $here ('payload-' + [guid]::NewGuid().ToString('N') + '.json')
try {
  [System.IO.File]::WriteAllBytes($payloadFile, $bytes)
  $stdout = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $logger -PayloadFile $payloadFile 2>$null
  if ($stdout) { Write-Output $stdout }
} finally {
  if (Test-Path -LiteralPath $payloadFile) {
    Remove-Item -LiteralPath $payloadFile -Force -ErrorAction SilentlyContinue
  }
}
exit 0
