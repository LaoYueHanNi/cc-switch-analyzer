param(
  [Parameter(Mandatory = $true)]
  [string]$PayloadFile
)

$ErrorActionPreference = 'SilentlyContinue'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$logPath = Join-Path $here 'requests.jsonl'
$heartbeatPath = Join-Path $here 'hook-heartbeat.json'

function Get-IsoNow {
  return (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ss+00:00')
}

function Now-Unix {
  return [int64][Math]::Floor((Get-Date).ToUniversalTime().Subtract((Get-Date '1970-01-01')).TotalSeconds)
}

function Write-Heartbeat {
  param($ok, $eventName, $errorMessage)
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

$row = [ordered]@{
  ts_utc = Get-IsoNow
}

$eventName = ''

# Minimal jsonl: ts_utc + hook_event_name + model (+ model_id fallback) + _parse_* on failure.
try {
  if (-not (Test-Path -LiteralPath $PayloadFile)) {
    $row['_parse_error'] = $true
    $row['_parse_msg'] = 'payload file missing'
  } else {
    $bytes = [System.IO.File]::ReadAllBytes($PayloadFile)
    if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
      $tmp = New-Object byte[] ($bytes.Length - 3)
      [Array]::Copy($bytes, 3, $tmp, 0, $tmp.Length)
      $bytes = $tmp
    }
    if ($bytes.Length -eq 0) {
      $row['_parse_error'] = $true
      $row['_parse_msg'] = 'empty stdin'
    } else {
      $text = [System.Text.Encoding]::UTF8.GetString($bytes).Trim().TrimStart([char]0xFEFF)
      $payload = $text | ConvertFrom-Json -ErrorAction Stop
      foreach ($k in @('hook_event_name', 'model', 'model_id')) {
        if ($null -ne $payload.$k -and "$($payload.$k)" -ne '') {
          $row[$k] = $payload.$k
        }
      }
      $hasModel = $row.Contains('model') -and "$($row['model'])" -ne ''
      if (-not $hasModel -and $null -ne $payload.subagent_model -and "$($payload.subagent_model)" -ne '') {
        $row['model'] = $payload.subagent_model
      }
    }
  }
} catch {
  $row['_parse_error'] = $true
  $row['_parse_msg'] = "$($_.Exception.Message)"
}

if ($row.Contains('hook_event_name')) { $eventName = [string]$row['hook_event_name'] }

try {
  $line = ($row | ConvertTo-Json -Compress -Depth 3)
  Add-Content -LiteralPath $logPath -Value $line -Encoding UTF8
  Write-Heartbeat -ok $true -eventName $eventName
} catch {
  Write-Heartbeat -ok $false -eventName $eventName -errorMessage "$($_.Exception.Message)"
}

if ($eventName -eq 'beforeSubmitPrompt') {
  Write-Output '{"continue":true}'
} elseif ($eventName -eq 'preToolUse' -or $eventName -eq 'beforeReadFile' -or $eventName -eq 'subagentStart') {
  Write-Output '{"permission":"allow"}'
}
exit 0
