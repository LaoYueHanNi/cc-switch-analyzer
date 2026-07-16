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

function Get-IsoLocal {
  $offset = [TimeZoneInfo]::Local.GetUtcOffset((Get-Date))
  $sign = if ($offset.TotalMinutes -ge 0) { '+' } else { '-' }
  $hh = [Math]::Abs([int]$offset.Hours).ToString('00')
  $mm = [Math]::Abs([int]$offset.Minutes).ToString('00')
  return (Get-Date).ToString("yyyy-MM-ddTHH:mm:ss$sign$hh`:$mm")
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
  ts      = Get-IsoLocal
  ts_utc  = Get-IsoNow
  machine = $env:COMPUTERNAME
}

$eventName = ''

# 1) parse stdin payload into $row
try {
  if (-not (Test-Path -LiteralPath $PayloadFile)) {
    $row['_empty_stdin'] = $true
  } else {
    $bytes = [System.IO.File]::ReadAllBytes($PayloadFile)
    if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
      $tmp = New-Object byte[] ($bytes.Length - 3)
      [Array]::Copy($bytes, 3, $tmp, 0, $tmp.Length)
      $bytes = $tmp
    }
    if ($bytes.Length -eq 0) {
      $row['_empty_stdin'] = $true
    } else {
      $text = [System.Text.Encoding]::UTF8.GetString($bytes).Trim().TrimStart([char]0xFEFF)
      $payload = $text | ConvertFrom-Json -ErrorAction Stop
      $keep = @(
        'hook_event_name', 'conversation_id', 'generation_id', 'session_id',
        'model', 'model_id', 'cursor_version', 'user_email', 'workspace_roots',
        'transcript_path', 'tool_name', 'tool_input', 'tool_output', 'status',
        'context_tokens', 'context_usage_percent', 'context_window_size',
        # subagent / Task
        'subagent_id', 'subagent_type', 'subagent_model', 'parent_conversation_id',
        'tool_call_id', 'is_parallel_worker', 'task', 'description', 'summary',
        'duration_ms', 'message_count', 'tool_call_count', 'loop_count',
        'modified_files', 'agent_transcript_path',
        # session / compact / composer
        'is_background_agent', 'composer_mode', 'reason', 'final_status',
        'error_message', 'trigger', 'git_branch'
      )
      foreach ($k in $keep) {
        if ($null -ne $payload.$k -and "$($payload.$k)" -ne '') {
          $row[$k] = $payload.$k
        }
      }
      # Map subagent_model -> model when model is missing (for attribution)
      $hasModel = $row.Contains('model') -and "$($row['model'])" -ne ''
      if (-not $hasModel -and $row.Contains('subagent_model') -and "$($row['subagent_model'])" -ne '') {
        $row['model'] = $row['subagent_model']
      }
      if ($payload.model_params) {
        $params = @()
        foreach ($p in $payload.model_params) {
          if ($null -ne $p.id) {
            $params += [ordered]@{ id = $p.id; value = $p.value }
          }
        }
        if ($params.Count -gt 0) { $row['model_params'] = $params }
      }
      if ($payload.prompt -and "$($payload.prompt)".Length -gt 0) {
        $row['prompt_chars'] = ("$($payload.prompt)").Length
      }
      if ($payload.text -and "$($payload.text)".Length -gt 0) {
        $row['response_chars'] = ("$($payload.text)").Length
      }
      if ($payload.thought -and "$($payload.thought)".Length -gt 0) {
        $row['thought_chars'] = ("$($payload.thought)").Length
      }
    }
  }
} catch {
  $row['_parse_error'] = $true
  $row['_parse_msg'] = "$($_.Exception.Message)"
}

if ($row.Contains('hook_event_name')) { $eventName = [string]$row['hook_event_name'] }

# 2) append requests.jsonl; on failure write heartbeat; always exit 0
try {
  $line = ($row | ConvertTo-Json -Compress -Depth 6)
  Add-Content -LiteralPath $logPath -Value $line -Encoding UTF8
  Write-Heartbeat -ok $true -eventName $eventName
} catch {
  Write-Heartbeat -ok $false -eventName $eventName -errorMessage "$($_.Exception.Message)"
}

# 3) minimal stdout JSON for Cursor
if ($eventName -eq 'beforeSubmitPrompt' -or $row.Contains('prompt_chars')) {
  Write-Output '{"continue":true}'
} elseif ($eventName -eq 'subagentStart') {
  Write-Output '{"permission":"allow"}'
}
exit 0
