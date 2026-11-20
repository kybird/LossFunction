<#
.SYNOPSIS
    에이전트 세션에서 백필을 띄우는 래퍼 — 사람은 이 창에 마스터 비밀번호만 입력한다.

.DESCRIPTION
    backfill-daily.ps1을 자식 프로세스로 실행하고, 종료 코드를
    data/backfill-status.txt에 기록한다(에이전트는 그 파일만 폴링한다).
    상태 파일에는 시각·종료코드만 실린다 — 시크릿은 어디에도 안 쓴다.

    실패해도 이 래퍼는 재시도하지 않는다(90초 자동 재시도는 Rust 백필에 내장).
    500/403 시 프로세스를 다시 띄우는 것은 금지 — 원칙은 backfill-daily.ps1 머리글 참조.

.EXAMPLE
    # 에이전트가 Start-Process로 이 파일을 띄운다(콘솔 1개, -NoExit):
    powershell -NoExit -ExecutionPolicy Bypass -File scripts\dev\backfill-daily-agent.ps1
#>
param(
    # backfill-daily.ps1로 그대로 전달되는 인자(-Years 5, -Environment mock, ...)
    [Parameter(ValueFromRemainingArguments = $true)]
    $RemainingArgs
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$status = Join-Path $root "data\backfill-status.txt"
$log = Join-Path $root "data\backfill-last-run.log"
New-Item -ItemType Directory -Force -Path (Split-Path $status) | Out-Null
Set-Content -Path $status -Value "start=$(Get-Date -Format o)"

Write-Host "[wrapper] backfill-daily.ps1 실행 — 프롬프트가 뜨면 마스터 비밀번호를 입력하세요." -ForegroundColor Cyan
# 콘솔 출력을 통째로 tee — 스크립트는 시크릿을 출력하지 않으므로 안전(실패 원문 보존용).
# PS 5.1 함정: 2>&1로 넘어온 네이티브 stderr(cargo 진행 상태 등)는 ErrorRecord로 포장되고
# EAP=Stop이 즉시 종료시킨다(raw 2026-11-20 Case 6). 호출 구간만 EAP를 내리고 레코드를
# 문자열로 풀어서 tee한다.
$ErrorActionPreference = "Continue"
& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "backfill-daily.ps1") @RemainingArgs 2>&1 |
    ForEach-Object {
        if ($_ -is [System.Management.Automation.ErrorRecord]) { "$($_.Exception.Message)" }
        else { "$_" }
    } |
    Tee-Object -FilePath $log
$ErrorActionPreference = "Stop"
$code = if ($null -ne $LASTEXITCODE) { $LASTEXITCODE } else { -1 }

Add-Content -Path $status -Value "exit=$code end=$(Get-Date -Format o)"
# Tee-Object는 PS5.1에서 UTF-16LE로 쓴다 — 도구(git bash cat 등) 호환을 위해 UTF-8로 정리.
if (Test-Path $log) { Get-Content -LiteralPath $log | Set-Content -LiteralPath $log -Encoding UTF8 }
Write-Host "`n[wrapper] 백필 종료 코드: $code (상태: $status · 로그: $log) — 이 창은 닫아도 된다." -ForegroundColor Cyan
